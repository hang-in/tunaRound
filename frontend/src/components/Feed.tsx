// /dashboard/events SSE 를 구독해 task별로 묶은 카드로 표시하는 라이브 피드(상위 200 task 유지).
// 접수→진행중→완료(실패)를 같은 카드에서 갱신하고, 클릭하면 그 task 상세(요청·결과·실패 사유·이력)를 펼친다.
// 필터=검색 + 상태/머신/러너 드롭다운(체크박스 다중선택, 모두 클라이언트 측). 목업 .card.feed 이식.
import { useCallback, useEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from 'react'
import { ChevronDown, Search } from 'lucide-react'
import type { Agent, Part, Task, TaskEventMsg } from '../api'
import { relativeTime } from '../api'

// 리로드(=SSE 재접속) 시 선행 스냅샷 수와 유지 상한(v2-53: 50→200). 서버 clamp(500) 이내.
const MAX_TASKS = 200
const REPLAY = 200

// task id 앞부분만 축약해 보여준다(목업 "t-"+4자리).
function shortId(id: string): string {
  return 't-' + id.slice(0, 4)
}

const STATE_LABEL: Record<string, string> = {
  submitted: '접수',
  working: '진행',
  input_required: '입력 대기',
  completed: '완료',
  failed: '실패',
  canceled: '취소',
}

// 상태를 목업 배지 클래스(.state.working/.completed/.failed/.canceled)로 매핑. 열린 상태는 working 톤.
function stateClass(state: string): string {
  if (state === 'completed') return 'completed'
  if (state === 'failed') return 'failed'
  if (state === 'canceled') return 'canceled'
  return 'working'
}

// task id별로 최신 상태 + 받은 이벤트 이력을 누적한다.
type TaskCard = { id: string; latest: TaskEventMsg; history: TaskEventMsg[] }

// task 스냅샷에서 표시할 텍스트(완료=아티팩트 결과, 그 외=원 메시지 일부).
function taskText(msg: TaskEventMsg): string | undefined {
  return msg.task.artifacts?.[0]?.parts?.[0]?.text
}

// parts 의 text 를 이어붙인다(공백뿐인 조각은 건너뜀 - 빈 상세 블록 방지).
function joinParts(parts?: Part[]): string {
  return (parts ?? [])
    .map((p) => p.text ?? '')
    .filter((s) => s.trim() !== '')
    .join('\n')
}

// 요청 원문(history[0] = 접수 당시 메시지). 슬림된 과거 task 는 비어 있을 수 있다(P6b 보존정책).
function requestText(task: Task): string {
  return joinParts(task.history?.[0]?.parts)
}

// 결과 전문(완료 아티팩트 전체 parts 를 이어붙임).
function resultText(task: Task): string {
  return (task.artifacts ?? [])
    .map((a) => joinParts(a.parts))
    .filter(Boolean)
    .join('\n\n')
}

// 실패 사유(state=failed 일 때만 status_message 를 노출한다 - 그 외 상태의 진행 메시지와 섞지 않음).
function failureText(task: Task): string {
  return task.state === 'failed' ? joinParts(task.statusMessage?.parts) : ''
}

// 중복 제거 + 첫 등장 순서 유지(필터 후보값 산출용).
function distinct(values: string[]): string[] {
  return Array.from(new Set(values))
}

// 필터 한 차원의 드롭다운 버튼 + 체크박스 메뉴(다중선택). selected가 비면 "전체"(무필터).
function FilterDropdown({
  label,
  options,
  selected,
  open,
  onOpen,
  onToggleValue,
  onClear,
  render,
}: {
  label: string
  options: string[]
  selected: Set<string>
  open: boolean
  onOpen: () => void
  onToggleValue: (v: string) => void
  onClear: () => void
  render?: (v: string) => string
}) {
  // 후보가 없으면(동종 피드) 버튼 자체를 숨긴다.
  if (options.length === 0 && selected.size === 0) return null
  const active = selected.size > 0
  // 메뉴(체크박스 목록)는 버튼의 형제로 둔다 - <button> 안에 <input> 을 중첩하면 불법 HTML이라
  // 스크린리더·키보드 조작이 불안정해진다(a11y). 래퍼에 position:relative 를 둬 메뉴 위치는 그대로 유지.
  return (
    <div className="dd-wrap">
      <button
        type="button"
        className={'ddbtn' + (active ? ' active' : '')}
        onClick={onOpen}
        aria-haspopup="true"
        aria-expanded={open}
      >
        {label}
        {active ? <span className="cnt">{selected.size}</span> : null}
        <ChevronDown size={13} />
      </button>
      {open ? (
        <div className="ddmenu" onClick={(e) => e.stopPropagation()}>
          <label>
            <input type="checkbox" checked={selected.size === 0} onChange={onClear} />
            전체
          </label>
          {options.map((v) => (
            <label key={v}>
              <input type="checkbox" checked={selected.has(v)} onChange={() => onToggleValue(v)} />
              {render ? render(v) : v}
            </label>
          ))}
        </div>
      ) : null}
    </div>
  )
}

// 펼침 상세의 한 블록(요청/결과/실패 사유). text 가 비면 렌더하지 않는다.
function DetailBlock({ label, text, tone }: { label: string; text: string; tone?: 'ok' | 'err' }) {
  if (!text.trim()) return null
  const toneClass = tone ? ' ' + tone : ''
  return (
    <div className="tdetail-block">
      <span className={'tdetail-label' + toneClass}>{label}</span>
      <div className="tdetail-body">{text}</div>
    </div>
  )
}

type Props = {
  onConnectedChange: (connected: boolean) => void
  onEvent: (msg: TaskEventMsg) => void
  agents: Agent[]
}

type Dim = 'state' | 'machine' | 'runner'

export default function Feed({ onConnectedChange, onEvent, agents }: Props) {
  const [cards, setCards] = useState<TaskCard[]>([])
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})
  // 관제 필터(모두 클라이언트 측, 서버 무변경). 빈 Set = 해당 차원 무필터(전체).
  const [stateFilter, setStateFilter] = useState<Set<string>>(new Set())
  const [machineFilter, setMachineFilter] = useState<Set<string>>(new Set())
  const [runnerFilter, setRunnerFilter] = useState<Set<string>>(new Set())
  const [query, setQuery] = useState('')
  // 한 번에 하나의 드롭다운만 연다. 바깥 클릭으로 닫는다.
  const [openDim, setOpenDim] = useState<Dim | null>(null)
  const filterRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (openDim === null) return
    const onDown = (e: MouseEvent) => {
      if (filterRef.current && !filterRef.current.contains(e.target as Node)) setOpenDim(null)
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [openDim])

  // 라우팅 id(uuid)를 로스터의 사람이 읽는 이름으로 바꾼다(호버에 원 id는 title로 유지).
  const agentNames = useMemo(() => {
    const names = new Map<string, string>()
    for (const agent of agents) {
      const label =
        (agent.display_name ?? '').trim() || (agent.tags?.project ?? '').trim() || agent.uuid.slice(0, 8)
      names.set(agent.uuid, label)
    }
    return names
  }, [agents])
  // 라우팅 id → 표시 이름(useCallback으로 안정화해 아래 memo 들의 deps 계약을 정확히 유지).
  const nameOf = useCallback(
    (id: string) => agentNames.get(id) ?? (/^[0-9a-f][0-9a-f-]{11,}$/i.test(id) ? id.slice(0, 8) : id),
    [agentNames],
  )

  // 워커(toAgent)의 머신·러너 태그를 로스터에서 조회한다(머신/러너 필터·표시용).
  const workerMeta = useMemo(() => {
    const m = new Map<string, { machine?: string; runner?: string }>()
    for (const agent of agents) {
      m.set(agent.uuid, { machine: agent.tags?.machine, runner: agent.tags?.runner })
    }
    return m
  }, [agents])
  const runnerOf = useCallback(
    (t: Task) => t.runner || workerMeta.get(t.toAgent)?.runner || '',
    [workerMeta],
  )
  const machineOf = useCallback((t: Task) => workerMeta.get(t.toAgent)?.machine || '', [workerMeta])

  // 필터 후보값 - 현재 피드에 실제 존재하는 값만.
  const states = useMemo(() => distinct(cards.map((c) => c.latest.task.state)), [cards])
  const machines = useMemo(
    () => distinct(cards.map((c) => machineOf(c.latest.task)).filter(Boolean)),
    [cards, machineOf],
  )
  const runners = useMemo(
    () => distinct(cards.map((c) => runnerOf(c.latest.task)).filter(Boolean)),
    [cards, runnerOf],
  )

  // 필터 적용(상태·머신·러너 = Set 포함일치, 텍스트 = id·양끝 이름·요청/결과/실패 사유 부분일치).
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase()
    return cards.filter((c) => {
      const t = c.latest.task
      if (stateFilter.size > 0 && !stateFilter.has(t.state)) return false
      if (machineFilter.size > 0 && !machineFilter.has(machineOf(t))) return false
      if (runnerFilter.size > 0 && !runnerFilter.has(runnerOf(t))) return false
      if (q) {
        const hay = [
          shortId(t.id),
          nameOf(t.fromAgent),
          nameOf(t.toAgent),
          requestText(t),
          resultText(t),
          failureText(t),
        ]
          .join(' ')
          .toLowerCase()
        if (!hay.includes(q)) return false
      }
      return true
    })
  }, [cards, stateFilter, machineFilter, runnerFilter, query, machineOf, runnerOf, nameOf])

  useEffect(() => {
    // replay=200: 접속(리로드 포함) 시 최근 200 task 스냅샷을 라이브에 앞서 선행 수신한다(v2-53).
    const source = new EventSource('/dashboard/events?replay=' + REPLAY)
    source.onopen = () => onConnectedChange(true)
    source.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data) as TaskEventMsg
        setCards((prev) => {
          const id = msg.task.id
          const existing = prev.find((c) => c.id === id)
          // EventSource 자동 재접속마다 스냅샷이 다시 오므로, 이미 반영된 것과 같은
          // updatedAt(+state)의 이벤트는 history에 다시 쌓지 않는다('N단계' 부풀림 방지).
          const duplicate =
            existing !== undefined &&
            existing.latest.task.updatedAt === msg.task.updatedAt &&
            existing.latest.task.state === msg.task.state
          const card: TaskCard = existing
            ? { id, latest: msg, history: duplicate ? existing.history : [...existing.history, msg] }
            : { id, latest: msg, history: [msg] }
          // 방금 갱신된 task를 맨 위로, 상위 MAX_TASKS만 유지.
          return [card, ...prev.filter((c) => c.id !== id)].slice(0, MAX_TASKS)
        })
        onEvent(msg)
      } catch (err) {
        console.error('[feed] 이벤트 파싱 실패.', err)
      }
    }
    source.onerror = (err) => {
      onConnectedChange(false)
      console.error('[feed] SSE 오류.', err)
    }
    return () => {
      source.close()
      onConnectedChange(false)
    }
  }, [onConnectedChange, onEvent])

  const toggle = (id: string) => setExpanded((e) => ({ ...e, [id]: !e[id] }))

  // Set 토글 헬퍼(불변 갱신).
  const toggleIn = (setter: Dispatch<SetStateAction<Set<string>>>, v: string) =>
    setter((prev) => {
      const next = new Set(prev)
      if (next.has(v)) next.delete(v)
      else next.add(v)
      return next
    })

  const countLabel =
    visible.length === cards.length ? `${cards.length}건` : `${visible.length} / ${cards.length}건`

  return (
    <section className="card feed">
      <div className="head">
        <h2>라이브 task 피드</h2>
        <span className="count">{cards.length === 0 ? '대기 중' : countLabel}</span>
      </div>
      {cards.length > 0 ? (
        <div className="filter" ref={filterRef}>
          <div className="search">
            <Search size={14} />
            <input
              type="search"
              placeholder="task 검색(id·이름·내용)"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <FilterDropdown
            label="상태"
            options={states}
            selected={stateFilter}
            open={openDim === 'state'}
            onOpen={() => setOpenDim((d) => (d === 'state' ? null : 'state'))}
            onToggleValue={(v) => toggleIn(setStateFilter, v)}
            onClear={() => setStateFilter(new Set())}
            render={(v) => STATE_LABEL[v] ?? v}
          />
          <FilterDropdown
            label="머신"
            options={machines}
            selected={machineFilter}
            open={openDim === 'machine'}
            onOpen={() => setOpenDim((d) => (d === 'machine' ? null : 'machine'))}
            onToggleValue={(v) => toggleIn(setMachineFilter, v)}
            onClear={() => setMachineFilter(new Set())}
          />
          <FilterDropdown
            label="러너"
            options={runners}
            selected={runnerFilter}
            open={openDim === 'runner'}
            onOpen={() => setOpenDim((d) => (d === 'runner' ? null : 'runner'))}
            onToggleValue={(v) => toggleIn(setRunnerFilter, v)}
            onClear={() => setRunnerFilter(new Set())}
          />
        </div>
      ) : null}
      <div className="feed-body">
        {cards.length === 0 ? (
          <div className="feed-empty">task 이벤트 대기 중.</div>
        ) : visible.length === 0 ? (
          <div className="feed-empty">필터에 맞는 task 가 없습니다.</div>
        ) : (
          visible.map((card) => {
            const t = card.latest.task
            const label = STATE_LABEL[t.state] ?? t.state
            const isOpen = !!expanded[card.id]
            const text = taskText(card.latest)
            return (
              <div className="tcard" key={card.id}>
                <button type="button" className="tl" onClick={() => toggle(card.id)} aria-expanded={isOpen}>
                  <span className="tid">{shortId(t.id)}</span>
                  <span className="route">
                    <span title={t.fromAgent}>{nameOf(t.fromAgent)}</span>
                    <span className="arrow">→</span>
                    <span title={t.toAgent}>{nameOf(t.toAgent)}</span>
                  </span>
                  {card.history.length > 1 ? <span className="steps">{card.history.length}단계</span> : null}
                  <span className="dash-spacer" />
                  <span className="rel">{relativeTime(t.updatedAt)}</span>
                  <span className={'state ' + stateClass(t.state)}>{label}</span>
                  <span className={'caret' + (isOpen ? ' open' : '')}>
                    <ChevronDown size={13} />
                  </span>
                </button>
                {text && !isOpen ? <div className="desc">{text}</div> : null}
                {isOpen ? (
                  <div className="tcard-detail">
                    <DetailBlock label="요청" text={requestText(t)} />
                    {t.state === 'failed' ? (
                      <DetailBlock label="실패 사유" text={failureText(t)} tone="err" />
                    ) : (
                      <DetailBlock label="결과" text={resultText(t)} tone="ok" />
                    )}
                    {card.history.length > 1 ? (
                      <div className="tdetail-history">
                        {card.history.map((h) => {
                          const ht = h.task
                          const htext = taskText(h)
                          return (
                            <div className="throw" key={`${ht.updatedAt}-${ht.state}`}>
                              <span className={'state small ' + stateClass(ht.state)}>
                                {STATE_LABEL[ht.state] ?? ht.state}
                              </span>
                              <span className="rel">{relativeTime(ht.updatedAt)}</span>
                              {htext ? <div className="htext">{htext}</div> : null}
                            </div>
                          )
                        })}
                      </div>
                    ) : null}
                  </div>
                ) : null}
              </div>
            )
          })
        )}
      </div>
    </section>
  )
}
