// 감독 로스터: 총감독(정적) 카드 + 실 감독 카드 목록. 목업 "감독 로스터" 섹션 이식.
import type { Agent } from '../api'
import { relativeTime } from '../api'

type Props = {
  agents: Agent[]
  // uuid -> 방금 heartbeat 가 갱신돼 pulse 애니를 잠깐 보여줄지 여부.
  pulses: Record<string, boolean>
}

// 태그 표시 순서. 이 순서에 없는 키는 뒤에 알파벳순으로 덧붙인다.
const TAG_ORDER = ['machine', 'runner', 'role', 'project']

function orderedTags(tags: Record<string, string>): Array<[string, string]> {
  const known = TAG_ORDER.filter((k) => k in tags).map((k) => [k, tags[k]] as [string, string])
  const rest = Object.keys(tags)
    .filter((k) => !TAG_ORDER.includes(k))
    .sort()
    .map((k) => [k, tags[k]] as [string, string])
  return [...known, ...rest]
}

const N_DOTS = 14

function MachineIcon({ machine }: { machine: string | undefined }) {
  if (machine === 'mac') {
    return (
      <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor" aria-label="mac">
        <path d="M11.05 8.36c-.02-1.5 1.22-2.22 1.28-2.26-.7-1.02-1.79-1.16-2.17-1.18-.92-.09-1.8.54-2.27.54-.47 0-1.19-.53-1.96-.51-1.01.01-1.94.59-2.46 1.49-1.05 1.82-.27 4.52.75 6 .5.73 1.09 1.54 1.87 1.51.75-.03 1.03-.48 1.94-.48.9 0 1.16.48 1.95.47.81-.01 1.32-.74 1.81-1.47.57-.84.81-1.66.82-1.7-.02-.01-1.57-.6-1.59-2.38zM9.6 3.87c.41-.5.69-1.2.61-1.9-.59.02-1.31.4-1.74.9-.38.44-.71 1.15-.62 1.83.66.05 1.34-.33 1.75-.83z" />
      </svg>
    )
  }
  if (machine === 'win') {
    return (
      <svg width="11" height="11" viewBox="0 0 16 16" fill="currentColor" aria-label="win">
        <path d="M1.5 3.1 7.1 2.4v5.2H1.5V3.1zM7.7 2.3 14.5 1.4v6.2H7.7V2.3zM1.5 8.2h5.6v5.2L1.5 12.7V8.2zM7.7 8.2h6.8v6.4l-6.8-.9V8.2z" />
      </svg>
    )
  }
  return null
}

function HbClockIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 12 12" fill="none" style={{ flex: 'none' }}>
      <circle cx="6" cy="6" r="4.6" stroke="currentColor" strokeWidth="1.1" />
      <path
        d="M6 3.4V6l1.8 1.1"
        stroke="currentColor"
        strokeWidth="1.1"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

function TagPill({ k, v }: { k: string; v: string }) {
  return (
    <span className="shield">
      <span className="shield-k">{k}</span>
      <span className={'shield-v v-' + k}>{v}</span>
    </span>
  )
}

// 총감독(디스패처) 카드. 이 세션 = 대시보드 뷰어이므로 실 백엔드 데이터 없이 정적으로 그린다.
function DispatcherCard() {
  return (
    <div className="dispatcher-card">
      <div className="card-row">
        <span className="dot-ping-wrap">
          <span className="dot-ping-core" />
          <span className="dot-ping-ring" />
        </span>
        <span className="dispatcher-name">총감독</span>
        <span className="pill-active">지휘 중</span>
        <span className="dash-spacer" />
        <span className="dispatcher-meta">나 · 이 세션</span>
      </div>
      <div className="tag-row">
        <TagPill k="role" v="dispatcher" />
      </div>
    </div>
  )
}

export default function Roster({ agents, pulses }: Props) {
  // online 먼저, offline 뒤(서버는 uuid 오름차순만 보장하므로 클라에서 재정렬).
  const sorted = [...agents].sort((a, b) => Number(b.online) - Number(a.online))

  return (
    <section className="roster-section">
      <div className="section-head">
        <h2 className="section-title">감독 로스터</h2>
        <span className="section-count">
          {agents.filter((a) => a.online).length}/{agents.length} online
        </span>
      </div>

      <DispatcherCard />

      {sorted.length === 0 ? (
        <div className="roster-card">
          <span style={{ color: 'var(--text-3)', fontSize: 13 }}>등록된 감독 없음.</span>
        </div>
      ) : (
        sorted.map((s) => {
          const pulse = !!pulses[s.uuid]
          return (
            <div className="roster-card" key={s.uuid}>
              <div className={'roster-card-body' + (s.online ? '' : ' offline')}>
                <div className="card-row">
                  <span className="status-dot-wrap">
                    <span className={'status-dot' + (s.online ? ' online' : '')} />
                    {pulse ? <span className="status-ping" /> : null}
                  </span>
                  <span className="machine-icon">
                    <MachineIcon machine={s.tags.machine} />
                  </span>
                  <span className="roster-uuid">{s.display_name ?? s.uuid}</span>
                  {s.online ? (
                    <span className="hb-dots">
                      {Array.from({ length: N_DOTS }, (_, i) => (
                        <span
                          key={i}
                          className="hb-dot"
                          style={{ animationDelay: (i * 0.09).toFixed(2) + 's' }}
                        />
                      ))}
                      {pulse ? <span className="hb-sweep" /> : null}
                    </span>
                  ) : null}
                  <span className="dash-spacer" />
                  <span className="hb-label-group">
                    <HbClockIcon />
                    <span className="hb-label">{relativeTime(s.last_heartbeat)}</span>
                  </span>
                </div>
                <div className="tag-row">
                  {orderedTags(s.tags).map(([k, v]) => (
                    <TagPill key={k} k={k} v={v} />
                  ))}
                </div>
              </div>
            </div>
          )
        })
      )}
    </section>
  )
}
