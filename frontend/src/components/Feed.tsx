// /dashboard/events SSE 를 구독해 task별로 묶은 카드로 표시하는 피드(상위 50 task 유지).
// 한 task의 접수→진행중→완료(실패)를 같은 카드에서 갱신하고, 클릭하면 그 task의 이벤트 이력을 펼친다.
import { useEffect, useMemo, useState, type CSSProperties } from 'react'
import type { Agent, TaskEventMsg } from '../api'
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

type Props = {
  onConnectedChange: (connected: boolean) => void
  onEvent: (msg: TaskEventMsg) => void
  agents: Agent[]
}

export default function Feed({ onConnectedChange, onEvent, agents }: Props) {
  const [cards, setCards] = useState<TaskCard[]>([])
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})

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

  useEffect(() => {
    const source = new EventSource('/dashboard/events')
    source.onopen = () => onConnectedChange(true)
    source.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data) as TaskEventMsg
        setCards((prev) => {
          const id = msg.task.id
          const existing = prev.find((c) => c.id === id)
          const card: TaskCard = existing
            ? { id, latest: msg, history: [...existing.history, msg] }
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
        <span className="feed-count">{cards.length} tasks</span>
      </div>
      <div className="feed-list">
        {cards.length === 0 ? (
          <div className="feed-empty">task 이벤트 대기 중.</div>
        ) : (
          cards.map((card) => {
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
                    <div className="feed-history">
                      {card.history.map((h, i) => {
                        const ht = h.task
                        const htext = taskText(h)
                        return (
                          <div className="feed-hrow" key={i}>
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
              </div>
            )
          })
        )}
      </div>
    </section>
  )
}
