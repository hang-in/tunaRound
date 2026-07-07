// 총괄 대시보드 루트. roster 폴링 + heartbeat 변화 감지(pulse) + feed 이벤트 취합(통계)을 소유하고
// 헤더/통계/로스터/피드/goal 폼을 배치한다. 목업(총괄 대시보드.dc.html) 레이아웃 이식.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { Agent, Candidate, TaskEventMsg } from './api'
import { fetchRoster, fetchCandidates } from './api'
import { mergeSessions } from './activity'
import Header from './components/Header'
import StatTiles from './components/StatTiles'
import Roster from './components/Roster'
import Feed from './components/Feed'
import Candidates from './components/Candidates'
import GoalForm from './components/GoalForm'
import ControlForm from './components/ControlForm'

// 로컬(총괄) 세션인지 원격 관전인지 클라이언트에서 판정한다(loopback 여부).
const LOOPBACK_HOSTS = ['127.0.0.1', 'localhost', '[::1]', '::1']
const remoteViewer = !LOOPBACK_HOSTS.includes(location.hostname)

const PULSE_MS = 750

export default function App() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [candidates, setCandidates] = useState<Candidate[]>([])
  const [brokerOk, setBrokerOk] = useState(false)
  const [sseOpen, setSseOpen] = useState(false)
  // uuid -> 방금 heartbeat 가 갱신돼 짧게 pulse 를 보여줄지 여부.
  const [pulses, setPulses] = useState<Record<string, boolean>>({})
  // taskId -> 가장 최근 수신한 이벤트(통계 타일 계산용).
  const [taskLatest, setTaskLatest] = useState<Record<string, TaskEventMsg>>({})

  const prevHbRef = useRef<Record<string, string>>({})
  const pulseTimersRef = useRef<number[]>([])

  // roster 를 5초 주기로 폴링해 로스터 패널/goal 폼/통계 타일이 공유한다.
  useEffect(() => {
    let cancelled = false
    const controller = new AbortController()

    const load = () => {
      // 발견 후보(discover jsonl+age)도 같이 폴 - 로스터와 세션 uuid로 병합해 활동 배치(설계 v2-41).
      fetchCandidates(controller.signal)
        .then((list) => {
          if (!cancelled) setCandidates(list)
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          // 후보 조회 실패는 치명 아님(브로커/로스터는 별도 판정).
          console.error('[candidates] 조회 실패.', err)
        })

      fetchRoster(controller.signal)
        .then((list) => {
          if (cancelled) return
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
              }, PULSE_MS)
              pulseTimersRef.current.push(timer)
            })
          }
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          setBrokerOk(false)
          console.error('[roster] 조회 실패.', err)
        })
    }

    load()
    const timer = window.setInterval(load, 5000)
    return () => {
      cancelled = true
      controller.abort()
      window.clearInterval(timer)
      pulseTimersRef.current.forEach(window.clearTimeout)
      pulseTimersRef.current = []
    }
  }, [])

  // Feed 의 useEffect 재실행(SSE 재구독)을 막기 위해 안정적인 콜백으로 감싼다.
  const handleConnected = useCallback((v: boolean) => setSseOpen(v), [])
  const handleEvent = useCallback((msg: TaskEventMsg) => {
    setTaskLatest((prev) => ({ ...prev, [msg.task.id]: msg }))
  }, [])

  // taskId 별 최신 상태로 진행중/완료/실패 카운트를 계산한다(목업 renderVals 의 latest-per-task 로직).
  const { workingCount, completedCount, failedCount } = useMemo(() => {
    let working = 0
    let completed = 0
    let failed = 0
    Object.values(taskLatest).forEach((msg) => {
      const s = msg.task.state
      if (s === 'submitted' || s === 'working') working += 1
      else if (s === 'completed') completed += 1
      else if (s === 'failed') failed += 1
    })
    return { workingCount: working, completedCount: completed, failedCount: failed }
  }, [taskLatest])

  // 로스터+후보를 세션 uuid로 병합해 활동(age)으로 활성/유휴 분리(설계 v2-41).
  const { active, idle, autoBossUuid } = useMemo(
    () => mergeSessions(agents, candidates),
    [agents, candidates],
  )

  const onlineCount = active.filter((r) => r.online).length

  return (
    <div className="dash-root">
      <Header brokerOk={brokerOk} sseOpen={sseOpen} remoteViewer={remoteViewer} />
      <main className="dash-main">
        <StatTiles
          onlineCount={onlineCount}
          totalSups={active.length}
          workingCount={workingCount}
          completedCount={completedCount}
          failedCount={failedCount}
        />
        <Roster rows={active} pulses={pulses} autoBossUuid={autoBossUuid} />
        <Feed onConnectedChange={handleConnected} onEvent={handleEvent} />
        <Candidates rows={idle} />
        <GoalForm agents={agents} remoteViewer={remoteViewer} />
        <ControlForm remoteViewer={remoteViewer} />
      </main>
    </div>
  )
}
