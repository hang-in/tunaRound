// /dashboard/events SSE 를 구독해 task 이벤트를 실시간으로 최신순 표시하는 피드(상위 50개 유지).
// 목업 "라이브 task 피드" 섹션 이식 + taskId 별 최신 상태를 상위(StatTiles)로 올린다.
import { useEffect, useRef, useState } from 'react'
import type { TaskEventMsg } from '../api'
import { relativeTime } from '../api'

// 화면 목록용 항목. seq 로 안정적인 React key 를 부여한다(같은 task 가 여러 번 와도 구분).
type FeedRow = { seq: number; msg: TaskEventMsg }

const MAX_ROWS = 50

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

function ArrowIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" style={{ flex: 'none' }}>
      <path
        d="M2 6h7M6.5 3 9.5 6l-3 3"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

type Props = {
  // 피드 SSE 연결 상태를 상위(헤더 연결 뱃지)로 올린다.
  onConnectedChange: (connected: boolean) => void
  // task 이벤트 하나를 상위(App)로 올려 통계 타일이 taskId 별 최신 상태를 계산할 수 있게 한다.
  onEvent: (msg: TaskEventMsg) => void
}

export default function Feed({ onConnectedChange, onEvent }: Props) {
  const [rows, setRows] = useState<FeedRow[]>([])
  // seq는 useEffect 재실행(StrictMode 이중호출 포함)에도 유지돼야 key 중복이 안 난다.
  const seqRef = useRef(0)

  useEffect(() => {
    const source = new EventSource('/dashboard/events')

    source.onopen = () => onConnectedChange(true)

    source.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data) as TaskEventMsg
        seqRef.current += 1
        const row: FeedRow = { seq: seqRef.current, msg }
        // 최신을 위로 prepend 하고 상위 50개만 유지한다.
        setRows((prev) => [row, ...prev].slice(0, MAX_ROWS))
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

  return (
    <section className="feed-section">
      <div className="feed-header">
        <h2 className="section-title">라이브 task 피드</h2>
        <span className="feed-live">
          <span className="dash-badge-dot blink" />
          LIVE
        </span>
        <span className="dash-spacer" />
        <span className="feed-count">{rows.length} events</span>
      </div>
      <div className="feed-list">
        {rows.length === 0 ? (
          <div className="feed-empty">task 이벤트 대기 중.</div>
        ) : (
          rows.map(({ seq, msg }) => {
            const t = msg.task
            const text = t.artifacts[0]?.parts[0]?.text
            const color = STATE_COLOR[t.state] ?? STATE_COLOR.submitted
            const label = STATE_LABEL[t.state] ?? t.state
            return (
              <div className="feed-row" key={seq}>
                <div className="feed-row-inner">
                  <div className="feed-row-head">
                    <span
                      className="feed-badge"
                      style={{
                        color,
                        background: 'color-mix(in srgb, ' + color + ' 12%, transparent)',
                        border: '1px solid color-mix(in srgb, ' + color + ' 26%, transparent)',
                      }}
                    >
                      {label}
                    </span>
                    <span className="feed-shortid">{shortId(t.id)}</span>
                    <span className="feed-route">
                      <span>{t.fromAgent}</span>
                      <ArrowIcon />
                      <span>{t.toAgent}</span>
                    </span>
                    <span className="dash-spacer" />
                    <span className="feed-rel">{relativeTime(t.updatedAt)}</span>
                  </div>
                  {text ? <div className="feed-text">{text}</div> : null}
                </div>
              </div>
            )
          })
        )}
      </div>
    </section>
  )
}
