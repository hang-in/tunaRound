// /dashboard/events SSE 를 구독해 task별로 묶은 카드로 표시하는 피드(상위 50 task 유지).
// 한 task의 접수→진행중→완료(실패)를 같은 카드에서 갱신하고, 클릭하면 그 task의 이벤트 이력을 펼친다.
import { useEffect, useMemo, useState, type CSSProperties } from 'react'
import type { Agent, Part, Task, TaskEventMsg } from '../api'
import { relativeTime } from '../api'

const MAX_TASKS = 50

// task id 앞부분만 축약해 보여준다(목업 "t-"+4자리).
function shortId(id: string): string {
  return 't-' + id.slice(0, 4)
}

const STATE_LABEL: Record<string, string> = {
  submitted: '접수',
  working: '진행중',
  completed: '완료',
  failed: '실패',
  canceled: '취소',
}

const STATE_COLOR: Record<string, string> = {
  submitted: 'var(--info)',
  working: 'var(--warn)',
  completed: 'var(--ok)',
  failed: 'var(--err)',
  canceled: 'var(--text-3)',
}

// task id별로 최신 상태 + 받은 이벤트 이력을 누적한다.
type TaskCard = { id: string; latest: TaskEventMsg; history: TaskEventMsg[] }

function badgeStyle(state: string): CSSProperties {
  const color = STATE_COLOR[state] ?? STATE_COLOR.submitted
  return {
    color,
    background: 'color-mix(in srgb, ' + color + ' 12%, transparent)',
    border: '1px solid color-mix(in srgb, ' + color + ' 26%, transparent)',
  }
}

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

// 중복 제거 + 첫 등장 순서 유지(필터 칩 후보값 산출용).
function distinct(values: string[]): string[] {
  return Array.from(new Set(values))
}

function ArrowIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" style={{ flex: 'none' }}>
      <path d="M2 6h7M6.5 3 9.5 6l-3 3" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 12 12"
      fill="none"
      style={{ flex: 'none', transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 0.15s' }}
    >
      <path d="M4.5 3 8 6l-3.5 3" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

// 한 필터 차원의 칩 묶음. 후보가 2개 미만이면(동종 피드) 아예 렌더하지 않는다.
function ChipGroup({
  label,
  values,
  active,
  onPick,
  render,
}: {
  label: string
  values: string[]
  active: string | null
  onPick: (v: string | null) => void
  render?: (v: string) => string
}) {
  // 활성값이 피드에서 사라져도(task 노후로 MAX_TASKS 밖) 해제 경로가 남게 후보에 포함한다.
  const opts = active && !values.includes(active) ? [...values, active] : values
  // 필터가 걸려 있지 않고 후보가 1개뿐이면(동종 피드) 칩 자체를 숨겨 잡음을 줄인다.
  if (opts.length < 2 && active === null) return null
  return (
    <div className="feed-chip-group">
      <span className="feed-chip-label">{label}</span>
      <button
        type="button"
        className={'feed-chip' + (active === null ? ' active' : '')}
        onClick={() => onPick(null)}
      >
        전체
      </button>
      {opts.map((v) => (
        <button
          key={v}
          type="button"
          className={'feed-chip' + (active === v ? ' active' : '')}
          onClick={() => onPick(active === v ? null : v)}
        >
          {render ? render(v) : v}
        </button>
      ))}
    </div>
  )
}

// 펼침 상세의 한 블록(요청/결과/실패 사유). text 가 비면 렌더하지 않는다.
function DetailBlock({ label, text, tone }: { label: string; text: string; tone?: 'ok' | 'err' }) {
  if (!text.trim()) return null
  const toneClass = tone ? ' ' + tone : ''
  return (
    <div className="feed-detail-block">
      <span className={'feed-detail-label' + toneClass}>{label}</span>
      <div className="feed-detail-body">{text}</div>
    </div>
  )
}

type Props = {
  onConnectedChange: (connected: boolean) => void
  onEvent: (msg: TaskEventMsg) => void
  agents: Agent[]
}

export default function Feed({ onConnectedChange, onEvent, agents }: Props) {
  const [cards, setCards] = useState<TaskCard[]>([])
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})
  // 관제 필터(모두 클라이언트 측, 서버 무변경). null=해당 차원 미필터.
  const [stateFilter, setStateFilter] = useState<string | null>(null)
  const [machineFilter, setMachineFilter] = useState<string | null>(null)
  const [runnerFilter, setRunnerFilter] = useState<string | null>(null)
  const [query, setQuery] = useState('')

  // 라우팅 id(uuid)를 로스터의 사람이 읽는 이름으로 바꾼다(호버에 원 id는 title로 유지).
  // 로스터에 없으면(오프라인·과거 세션) uuid는 8자로 축약, 친숙명(dashboard 등)은 그대로.
  // 이름 후보는 trim 후 ||로 고른다 - 빈 문자열 display_name/project가 빈 라벨로 렌더되지 않게(봇리뷰).
  const agentNames = useMemo(() => {
    const names = new Map<string, string>()
    for (const agent of agents) {
      const label =
        (agent.display_name ?? '').trim() || (agent.tags?.project ?? '').trim() || agent.uuid.slice(0, 8)
      names.set(agent.uuid, label)
    }
    return names
  }, [agents])
  const nameOf = (id: string) =>
    agentNames.get(id) ?? (/^[0-9a-f][0-9a-f-]{11,}$/i.test(id) ? id.slice(0, 8) : id)

  // 워커(toAgent)의 머신·러너 태그를 로스터에서 조회한다(머신/러너 필터·표시용).
  const workerMeta = useMemo(() => {
    const m = new Map<string, { machine?: string; runner?: string }>()
    for (const agent of agents) {
      m.set(agent.uuid, { machine: agent.tags?.machine, runner: agent.tags?.runner })
    }
    return m
  }, [agents])
  // 러너는 task 에 직접 실려오면 그 값을, 없으면 워커 태그로 보완한다.
  const runnerOf = (t: Task) => t.runner || workerMeta.get(t.toAgent)?.runner || ''
  const machineOf = (t: Task) => workerMeta.get(t.toAgent)?.machine || ''

  // 칩 후보값 - 현재 피드에 실제 존재하는 값만(동종 피드에선 자동으로 칩이 사라져 잡음이 없다).
  const states = useMemo(() => distinct(cards.map((c) => c.latest.task.state)), [cards])
  const machines = useMemo(
    () => distinct(cards.map((c) => machineOf(c.latest.task)).filter(Boolean)),
    [cards, workerMeta],
  )
  const runners = useMemo(
    () => distinct(cards.map((c) => runnerOf(c.latest.task)).filter(Boolean)),
    [cards, workerMeta],
  )

  // 필터 적용(상태·머신·러너 = 정확일치, 텍스트 = id·양끝 이름·요청/결과/실패 사유 부분일치).
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase()
    return cards.filter((c) => {
      const t = c.latest.task
      if (stateFilter && t.state !== stateFilter) return false
      if (machineFilter && machineOf(t) !== machineFilter) return false
      if (runnerFilter && runnerOf(t) !== runnerFilter) return false
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cards, stateFilter, machineFilter, runnerFilter, query, workerMeta, agentNames])

  useEffect(() => {
    // replay=50: 접속(리로드 포함) 시 최근 50 task 스냅샷을 라이브에 앞서 선행 수신한다
    // (브라우저 리로드 = 피드 전멸이던 것 해소, v2-45 P2).
    const source = new EventSource('/dashboard/events?replay=50')
    source.onopen = () => onConnectedChange(true)
    source.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data) as TaskEventMsg
        setCards((prev) => {
          const id = msg.task.id
          const existing = prev.find((c) => c.id === id)
          // EventSource 자동 재접속마다 스냅샷이 다시 오므로, 이미 반영된 것과 같은
          // updatedAt(+state)의 이벤트는 history에 다시 쌓지 않는다('N단계' 부풀림 방지).
          // 카드 최신 상태 병합·맨 위 이동 자체는 유지한다.
          const duplicate =
            existing !== undefined &&
            existing.latest.task.updatedAt === msg.task.updatedAt &&
            existing.latest.task.state === msg.task.state
          const card: TaskCard = existing
            ? { id, latest: msg, history: duplicate ? existing.history : [...existing.history, msg] }
            : { id, latest: msg, history: [msg] }
          // 방금 갱신된 task를 맨 위로, 상위 50 task만 유지.
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

  return (
    <section className="feed-section">
      <div className="feed-header">
        <h2 className="section-title">라이브 task 피드</h2>
        <span className="feed-live">
          <span className="dash-badge-dot blink" />
          LIVE
        </span>
        <span className="dash-spacer" />
        <span className="feed-count">
          {visible.length === cards.length
            ? cards.length + ' tasks'
            : visible.length + ' / ' + cards.length + ' tasks'}
        </span>
      </div>
      {cards.length > 0 ? (
        <div className="feed-filter">
          <input
            type="search"
            className="feed-filter-search"
            placeholder="task 검색(id·이름·내용)"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <ChipGroup
            label="상태"
            values={states}
            active={stateFilter}
            onPick={setStateFilter}
            render={(v) => STATE_LABEL[v] ?? v}
          />
          <ChipGroup label="머신" values={machines} active={machineFilter} onPick={setMachineFilter} />
          <ChipGroup label="러너" values={runners} active={runnerFilter} onPick={setRunnerFilter} />
        </div>
      ) : null}
      <div className="feed-list">
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
              <div className="feed-row" key={card.id}>
                <div className="feed-row-inner">
                  <button
                    type="button"
                    className="feed-card-head"
                    onClick={() => toggle(card.id)}
                    aria-expanded={isOpen}
                  >
                    <span className="feed-badge" style={badgeStyle(t.state)}>
                      {label}
                    </span>
                    <span className="feed-shortid">{shortId(t.id)}</span>
                    <span className="feed-route">
                      <span title={t.fromAgent}>{nameOf(t.fromAgent)}</span>
                      <ArrowIcon />
                      <span title={t.toAgent}>{nameOf(t.toAgent)}</span>
                    </span>
                    <span className="dash-spacer" />
                    {card.history.length > 1 ? (
                      <span className="feed-steps">{card.history.length}단계</span>
                    ) : null}
                    <span className="feed-rel">{relativeTime(t.updatedAt)}</span>
                    <Chevron open={isOpen} />
                  </button>
                  {text && !isOpen ? <div className="feed-text">{text}</div> : null}
                  {isOpen ? (
                    <div className="feed-detail">
                      <DetailBlock label="요청" text={requestText(t)} />
                      {t.state === 'failed' ? (
                        <DetailBlock label="실패 사유" text={failureText(t)} tone="err" />
                      ) : (
                        <DetailBlock label="결과" text={resultText(t)} tone="ok" />
                      )}
                      {card.history.length > 1 ? (
                        <div className="feed-history">
                          {card.history.map((h) => {
                            const ht = h.task
                            const htext = taskText(h)
                            // 카드 내 이력은 SSE 중복 가드가 (updatedAt, state) 유일성을 보장하므로
                            // 그 조합을 안정 키로 쓴다(index 키 안티패턴 회피).
                            return (
                              <div className="feed-hrow" key={`${ht.updatedAt}-${ht.state}`}>
                                <span className="feed-badge small" style={badgeStyle(ht.state)}>
                                  {STATE_LABEL[ht.state] ?? ht.state}
                                </span>
                                <span className="feed-rel">{relativeTime(ht.updatedAt)}</span>
                                {htext ? <div className="feed-htext">{htext}</div> : null}
                              </div>
                            )
                          })}
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                </div>
              </div>
            )
          })
        )}
      </div>
    </section>
  )
}
