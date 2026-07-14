// 총괄 대시보드 루트(shell 레이아웃, v2-53). roster 5초 폴 + heartbeat pulse + health 5초 폴을 소유하고
// 헤더/사이드바 로스터/통계/피드/타임라인/푸터/목표 모달을 배치한다. 확정 목업 dashboard-mockup.html 이식.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { Agent, BrokerHealth, Task, TaskEventMsg } from './api'
import { fetchHealth, fetchRoster } from './api'
import { buildRoster } from './activity'
import Header from './components/Header'
import StatTiles from './components/StatTiles'
import Roster from './components/Roster'
import Feed from './components/Feed'
import GoalForm from './components/GoalForm'
import PresenceTimeline from './components/PresenceTimeline'
import Footer from './components/Footer'

// 로컬(총괄) 세션인지 원격 관전인지 클라이언트에서 판정한다(loopback 여부).
const LOOPBACK_HOSTS = ['127.0.0.1', 'localhost', '[::1]', '::1']
const remoteViewer = !LOOPBACK_HOSTS.includes(location.hostname)

const PULSE_MS = 750
const HEALTH_POLL_MS = 5000
// seenStateRef 상한(최근 이만큼의 task 만 dedup 판정에 씀). 대시보드를 수일~수주 열어두면
// 무상한 Map 은 계속 자라므로, 넘으면 가장 오래 전에 넣은 항목부터 비운다.
const SEEN_STATE_CAP = 500

// transientBusy(SSE 실시간 동작 오버레이, 이슈 #94) 타이밍 상수.
// - MIN: 완료가 와도 이보다 짧게 일했으면 스피너를 이 시간까지 붙잡아 눈에 보이게 한다(빠른 task 펄스 가시화).
// - MAX: 종결 이벤트를 SSE가 놓쳐도(lease 만료 requeue 등은 이벤트가 없다) 무한히 켜져 있지 않도록 하는 안전망
//   상한이다. 서버 roster busy(5초 폴, BUSY_FRESH_SECS=5분)가 이미 기준선이라 transient는 그 사이 공백만 메운다.
const TRANSIENT_BUSY_MIN_MS = 4000
const TRANSIENT_BUSY_MAX_MS = 120000

// 브라우저 알림 옵트인 여부를 세션 간 기억(권한이 유지될 때만 유효).
const NOTIFY_PREF_KEY = 'tuna-notify'
const notifySupported = typeof Notification !== 'undefined'

// 테마 선호 저장 키. 저장값이 있으면 dataset.theme로 OS 미디어쿼리를 오버라이드한다(목업 script).
const THEME_KEY = 'tuna-theme'
type Theme = 'light' | 'dark'

function savedTheme(): Theme | null {
  const v = localStorage.getItem(THEME_KEY)
  return v === 'light' || v === 'dark' ? v : null
}

function initialTheme(): Theme {
  return savedTheme() ?? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
}

// task id 축약(알림 본문용).
function shortId(id: string): string {
  return 't-' + id.slice(0, 4)
}

// 종료 task 알림 본문용 한 줄 요약(완료=결과, 실패=상태 메시지 앞부분).
function terminalSnippet(task: Task): string {
  const result = task.artifacts?.[0]?.parts?.[0]?.text
  const status = task.statusMessage?.parts?.[0]?.text
  const raw = task.state === 'failed' ? status ?? '' : result ?? ''
  return raw.replace(/\s+/g, ' ').trim().slice(0, 100)
}

export default function App() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [brokerOk, setBrokerOk] = useState(false)
  const [sseOpen, setSseOpen] = useState(false)
  // uuid -> 방금 heartbeat 가 갱신돼 짧게 pulse 를 보여줄지 여부.
  const [pulses, setPulses] = useState<Record<string, boolean>>({})
  // 브라우저 알림 옵트인 상태. ref 는 handleEvent(안정 콜백) 안에서 최신값을 읽기 위한 미러.
  const [notifyOn, setNotifyOn] = useState(false)
  const notifyOnRef = useRef(false)
  // taskId -> 이 세션에서 마지막으로 관측한 state(완료/실패 전이 알림의 중복·재생 발화 방지용).
  const seenStateRef = useRef<Map<string, string>>(new Map())

  const prevHbRef = useRef<Record<string, string>>({})
  const pulseTimersRef = useRef<number[]>([])

  // transientBusy: SSE 라이브로 관측한 "지금 동작 중" 오버레이(이슈 #94 FN 해소). 서버 roster busy는
  // 5초 폴이라 빠른 task 를 놓치므로, working/종결 SSE 이벤트로 즉시 스피너를 켜고 끈다. ref=최신값을
  // 동기로 읽기 위한 미러(핸들러가 setState 콜백 안에서 타이머를 잡는 부수효과를 피하려는 목적, 위
  // pulses 패턴과 동일 계열). state=렌더 트리거.
  const transientBusyRef = useRef<Record<string, { taskId: string; since: number }>>({})
  const [transientBusy, setTransientBusy] = useState<Record<string, { taskId: string; since: number }>>({})
  // taskId -> 활성 타이머(안전망 만료 또는 지연 제거). task당 하나만 유지하고 상태 전이 때 기존 것을
  // clearTimeout으로 확정 정리한다 - 대시보드는 수일~수주 열어두므로 발화 대기 타이머를 쌓지 않는다(gemini).
  const transientTimersRef = useRef<Map<string, number>>(new Map())

  const applyTransientBusy = useCallback((next: Record<string, { taskId: string; since: number }>) => {
    transientBusyRef.current = next
    setTransientBusy(next)
  }, [])

  // uuid의 transientBusy 항목을 taskId가 일치할 때만 제거한다(그 사이 같은 uuid에 새 task가 붙었으면
  // 손대지 않는다 - 늦게 도착한 지연 타이머가 최신 항목을 잘못 지우는 것 방지).
  const removeTransientBusy = useCallback(
    (uuid: string, taskId: string) => {
      const cur = transientBusyRef.current
      const entry = cur[uuid]
      if (!entry || entry.taskId !== taskId) return
      // delete 대신 구조분해 omit(동적 키 delete 지양, DeepSource JS).
      const { [uuid]: _omit, ...next } = cur
      void _omit
      applyTransientBusy(next)
    },
    [applyTransientBusy],
  )

  // taskId의 기존 타이머를 확정 정리한다(있으면 clearTimeout + 맵에서 제거).
  const clearTransientTimer = useCallback((taskId: string) => {
    const timers = transientTimersRef.current
    const t = timers.get(taskId)
    if (t !== undefined) {
      window.clearTimeout(t)
      timers.delete(taskId)
    }
  }, [])

  // taskId의 타이머를 교체 등록한다(기존 것 정리 후). 발화 시 자기 항목을 맵에서 지운다.
  const setTransientTimer = useCallback(
    (taskId: string, cb: () => void, ms: number) => {
      clearTransientTimer(taskId)
      const t = window.setTimeout(() => {
        transientTimersRef.current.delete(taskId)
        cb()
      }, ms)
      transientTimersRef.current.set(taskId, t)
    },
    [clearTransientTimer],
  )

  // 언마운트 시 잔여 transient 타이머 정리(누수 방지).
  useEffect(() => {
    const timers = transientTimersRef.current
    return () => {
      timers.forEach((t) => window.clearTimeout(t))
      timers.clear()
    }
  }, [])

  // 브로커 헬스(버전·상태별 카운트·헬스 게이지·스캐너). Header/StatTiles/Footer가 공유(HealthPanel 폴 이관).
  const [health, setHealth] = useState<BrokerHealth | null>(null)
  const [healthOk, setHealthOk] = useState(true)

  // 테마(라이트/다크). 저장값 또는 OS로 초기화. 토글 시 dataset.theme 강제 + 저장.
  const [theme, setTheme] = useState<Theme>(initialTheme)
  useEffect(() => {
    // 저장값이 있을 때만 dataset을 강제한다(없으면 CSS @media가 OS를 따른다).
    const s = savedTheme()
    if (s) document.documentElement.dataset.theme = s
  }, [])
  const toggleTheme = useCallback(() => {
    setTheme((cur) => {
      const next: Theme = cur === 'dark' ? 'light' : 'dark'
      document.documentElement.dataset.theme = next
      localStorage.setItem(THEME_KEY, next)
      return next
    })
  }, [])

  // 목표 제출 모달 오픈 여부.
  const [modalOpen, setModalOpen] = useState(false)
  const openGoal = useCallback(() => setModalOpen(true), [])
  const closeGoal = useCallback(() => setModalOpen(false), [])

  // 마운트 시 알림 선호 복원(권한이 아직 granted 일 때만 유효).
  useEffect(() => {
    if (!notifySupported) return
    if (localStorage.getItem(NOTIFY_PREF_KEY) === '1' && Notification.permission === 'granted') {
      setNotifyOn(true)
      notifyOnRef.current = true
    }
  }, [])

  // 알림 토글: 켤 때 권한이 없으면 요청하고, 승인돼야 켠다. 끄면 즉시 off + 선호 저장.
  const toggleNotify = useCallback(() => {
    if (!notifySupported) return
    if (notifyOnRef.current) {
      setNotifyOn(false)
      notifyOnRef.current = false
      localStorage.setItem(NOTIFY_PREF_KEY, '0')
      return
    }
    const enable = () => {
      setNotifyOn(true)
      notifyOnRef.current = true
      localStorage.setItem(NOTIFY_PREF_KEY, '1')
    }
    if (Notification.permission === 'granted') {
      enable()
      return
    }
    Notification.requestPermission()
      .then((perm) => {
        if (perm === 'granted') enable()
      })
      .catch((err) => console.error('[notify] 권한 요청 실패.', err))
  }, [])

  // roster 를 5초 주기로 폴링해 로스터 패널/goal 폼/통계 타일이 공유한다.
  useEffect(() => {
    let cancelled = false
    // in-flight 폴 추적: 매 폴마다 직전 요청 abort → 브로커가 느려 응답이 뒤섞이면 늦게 온 옛
    // 스냅샷이 최신을 덮는 레이스 방지(PresenceTimeline.tsx와 동일 패턴).
    let inflight: AbortController | null = null

    const load = () => {
      inflight?.abort()
      const controller = new AbortController()
      inflight = controller
      fetchRoster(controller.signal)
        .then((list) => {
          if (cancelled || controller.signal.aborted) return
          setAgents(list)
          setBrokerOk(true)

          // 이전 폴과 비교해 last_heartbeat 가 바뀐 uuid 를 찾아 짧게 pulse 를 켠다.
          const prev = prevHbRef.current
          const nextHb: Record<string, string> = {}
          const toPulse: string[] = []
          list.forEach((a) => {
            nextHb[a.uuid] = a.last_heartbeat
            if (prev[a.uuid] && prev[a.uuid] !== a.last_heartbeat) {
              toPulse.push(a.uuid)
            }
          })
          prevHbRef.current = nextHb

          if (toPulse.length > 0) {
            setPulses((p) => {
              const next = { ...p }
              toPulse.forEach((uuid) => {
                next[uuid] = true
              })
              return next
            })
            toPulse.forEach((uuid) => {
              const timer = window.setTimeout(() => {
                setPulses((p) => {
                  if (!p[uuid]) return p
                  const next = { ...p }
                  delete next[uuid]
                  return next
                })
                // 발화한 타이머 자신의 id 를 목록에서 제거(무한 누적 방지 - 대시보드는 수일~수주 열어둔다).
                pulseTimersRef.current = pulseTimersRef.current.filter((t) => t !== timer)
              }, PULSE_MS)
              pulseTimersRef.current.push(timer)
            })
          }
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          if (cancelled) return
          setBrokerOk(false)
          console.error('[roster] 조회 실패.', err)
        })
    }

    load()
    const timer = window.setInterval(load, 5000)
    return () => {
      cancelled = true
      inflight?.abort()
      window.clearInterval(timer)
      pulseTimersRef.current.forEach(window.clearTimeout)
      pulseTimersRef.current = []
    }
  }, [])

  // health 를 5초 주기로 폴링한다(HealthPanel 에서 이관). fail-visible: 최신 폴 실패는 healthOk=false 로 표면화.
  useEffect(() => {
    let cancelled = false
    // in-flight 폴 추적: roster 폴과 동일 패턴(PresenceTimeline.tsx 참고).
    let inflight: AbortController | null = null
    const load = () => {
      inflight?.abort()
      const controller = new AbortController()
      inflight = controller
      fetchHealth(controller.signal)
        .then((h) => {
          if (cancelled || controller.signal.aborted) return
          setHealth(h)
          setHealthOk(true)
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          if (cancelled) return
          setHealthOk(false)
          console.error('[health] 조회 실패.', err)
        })
    }
    load()
    const timer = window.setInterval(load, HEALTH_POLL_MS)
    return () => {
      cancelled = true
      inflight?.abort()
      window.clearInterval(timer)
    }
  }, [])

  // Feed 의 useEffect 재실행(SSE 재구독)을 막기 위해 안정적인 콜백으로 감싼다.
  const handleConnected = useCallback((v: boolean) => setSseOpen(v), [])
  const handleEvent = useCallback((msg: TaskEventMsg) => {
    const id = msg.task.id
    const state = msg.task.state
    // 알림: 이 세션에서 non-terminal 로 관측했던 task 가 완료/실패로 전이할 때만 발화한다.
    // 과거 task 재생(?replay)·재접속 스냅샷은 prev 가 없거나 이미 terminal 이라 발화하지 않는다.
    const prevState = seenStateRef.current.get(id)
    const terminalStates = ['completed', 'failed', 'canceled']
    const wasNonTerminal = prevState !== undefined && !terminalStates.includes(prevState)
    const isTerminal = state === 'completed' || state === 'failed'
    if (
      notifyOnRef.current &&
      isTerminal &&
      wasNonTerminal &&
      notifySupported &&
      Notification.permission === 'granted'
    ) {
      const title = state === 'completed' ? 'task 완료' : 'task 실패'
      const snippet = terminalSnippet(msg.task)
      const body = shortId(id) + (snippet ? ' · ' + snippet : '')
      try {
        // tag=id: 같은 task 재발화 시 브라우저가 알림을 겹쳐 하나로 유지한다.
        new Notification(title, { body, tag: id })
      } catch (err) {
        console.error('[notify] 알림 생성 실패.', err)
      }
    }
    const seen = seenStateRef.current
    // 재삽입: Map.set은 기존 키의 삽입 순서를 유지하므로, delete 후 set으로 갱신 항목을 맨 뒤(최신)로
    // 옮긴다. 그래야 keys().next()(가장 오래된)가 최근 갱신을 반영한 LRU가 된다(gemini/coderabbit).
    seen.delete(id)
    seen.set(id, state)
    // 삽입 순서 Map 이므로 keys().next() = 가장 오래된 항목. 상한 초과분만 정리(매회 최대 1건 유입).
    while (seen.size > SEEN_STATE_CAP) {
      const oldest = seen.keys().next().value
      if (oldest === undefined) break
      seen.delete(oldest)
    }

    // transientBusy: working이면 켜고(+안전망 만료 타이머), 종결이면 최소 표시시간을 지켜 끈다(#94 FN).
    // 타이머는 taskId당 하나(setTransientTimer가 기존 것을 clear 후 교체) - 종결 시점에 안전망
    // 타이머가 즉시 정리되어 장기 상주 대시보드에 발화 대기 타이머가 누적되지 않는다(gemini).
    const toAgent = msg.task.toAgent
    if (state === 'working') {
      applyTransientBusy({ ...transientBusyRef.current, [toAgent]: { taskId: id, since: Date.now() } })
      setTransientTimer(id, () => removeTransientBusy(toAgent, id), TRANSIENT_BUSY_MAX_MS)
    } else if (terminalStates.includes(state)) {
      // 최신 entry가 다른 task로 바뀌었어도 이 task의 안전망 타이머는 정리한다(발화해도 no-op이지만
      // 대기 자체를 남기지 않는다).
      const entry = transientBusyRef.current[toAgent]
      if (!entry || entry.taskId !== id) {
        clearTransientTimer(id)
      } else {
        const elapsed = Date.now() - entry.since
        if (elapsed >= TRANSIENT_BUSY_MIN_MS) {
          clearTransientTimer(id)
          removeTransientBusy(toAgent, id)
        } else {
          // 너무 빨리 끝났다 - 최소 표시시간(TRANSIENT_BUSY_MIN_MS)까지 지연 제거(안전망 타이머 교체).
          setTransientTimer(id, () => removeTransientBusy(toAgent, id), TRANSIENT_BUSY_MIN_MS - elapsed)
        }
      }
    }
  }, [applyTransientBusy, removeTransientBusy, setTransientTimer, clearTransientTimer])

  // 로스터 = online(heartbeat) 세션 전부. 총감독 = human_input_at 최신(설계 v2-43).
  const { rows: baseRows, autoBossUuid } = useMemo(() => buildRoster(agents), [agents])
  // 유효 busy = 서버 roster busy(5초 폴 기준선) OR transientBusy(SSE 실시간 오버레이, #94 FN 해소).
  const rows = useMemo(
    () => baseRows.map((r) => ({ ...r, busy: r.busy || Boolean(transientBusy[r.uuid]) })),
    [baseRows, transientBusy],
  )

  // 목표 제출 대상 선택(App 소유): 로스터 상세의 "이 세션에 목표"와 GoalForm이 공유한다.
  const [goalTargets, setGoalTargets] = useState<Record<string, boolean>>({})
  // 로스터 행의 "이 세션에 목표": 대상 추가 + 모달 오픈.
  const addGoalTarget = useCallback((uuid: string) => {
    setGoalTargets((prev) => ({ ...prev, [uuid]: true }))
    setModalOpen(true)
  }, [])

  return (
    <div className="dash-root">
      <Header
        version={health?.version ?? null}
        brokerOk={brokerOk}
        sseOpen={sseOpen}
        remoteViewer={remoteViewer}
        notifySupported={notifySupported}
        notifyOn={notifyOn}
        onToggleNotify={toggleNotify}
        theme={theme}
        onToggleTheme={toggleTheme}
        onOpenGoal={openGoal}
      />
      <div className="shell">
        <aside className="sidebar">
          <Roster rows={rows} pulses={pulses} autoBossUuid={autoBossUuid} onAddTarget={addGoalTarget} />
        </aside>
        <main>
          <StatTiles onlineCount={rows.length} health={health} ok={healthOk} />
          <Feed onConnectedChange={handleConnected} onEvent={handleEvent} agents={agents} />
          <PresenceTimeline />
        </main>
      </div>
      <Footer health={health} ok={healthOk} />
      <GoalForm
        open={modalOpen}
        onClose={closeGoal}
        agents={agents}
        remoteViewer={remoteViewer}
        selected={goalTargets}
        onChangeSelected={setGoalTargets}
      />
    </div>
  )
}
