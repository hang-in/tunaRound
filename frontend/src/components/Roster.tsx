// 관리자 로스터: 피드와 동일한 패널+행 레이아웃. 각 관리자 = 상태닷·머신아이콘·이름·heartbeat·태그.
// 총괄은 별도 카드가 아니라 대등한 행에 "현재 총괄" 뱃지로 표식(클릭해 지정, 앉는 머신 따라 바뀜).
import { useState } from 'react'
import type { Agent } from '../api'
import { relativeTime } from '../api'

// 태그 표시 순서. 없는 키는 뒤에 알파벳순.
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
const BOSS_KEY = 'tuna_dash_boss'

// 태그 값별 색(같은 키라도 값에 따라 다르게: mac≠win, claude≠codex, supervised≠dispatcher).
// 알려진 값은 고정 색, 나머지는 값 해시로 팔레트에서 안정적으로 배정.
const VALUE_COLOR: Record<string, string> = {
  mac: '#6e7681',
  win: '#0078d4',
  linux: '#f0883e',
  claude: '#c15f3c',
  codex: '#10a37f',
  gemini: '#4285f4',
  supervised: '#2da44e',
  dispatcher: '#8250df',
  worker: '#bf8700',
  tunaround: '#d29922',
}
const PALETTE = ['#2f6fe4', '#8957e5', '#2da44e', '#d29922', '#c15f3c', '#10a37f', '#bf3989', '#57606a']

function valueColor(v: string): string {
  if (VALUE_COLOR[v]) return VALUE_COLOR[v]
  let h = 0
  for (let i = 0; i < v.length; i++) h = (h * 31 + v.charCodeAt(i)) >>> 0
  return PALETTE[h % PALETTE.length]
}

// 머신 브랜드 글리프(박스 없이 인라인, 일관 크기).
function MachineGlyph({ machine }: { machine: string | undefined }) {
  if (machine === 'mac') {
    return (
      <svg className="machine-glyph" width="14" height="14" viewBox="0 0 16 16" fill="currentColor" aria-label="mac">
        <path d="M11.05 8.36c-.02-1.5 1.22-2.22 1.28-2.26-.7-1.02-1.79-1.16-2.17-1.18-.92-.09-1.8.54-2.27.54-.47 0-1.19-.53-1.96-.51-1.01.01-1.94.59-2.46 1.49-1.05 1.82-.27 4.52.75 6 .5.73 1.09 1.54 1.87 1.51.75-.03 1.03-.48 1.94-.48.9 0 1.16.48 1.95.47.81-.01 1.32-.74 1.81-1.47.57-.84.81-1.66.82-1.7-.02-.01-1.57-.6-1.59-2.38zM9.6 3.87c.41-.5.69-1.2.61-1.9-.59.02-1.31.4-1.74.9-.38.44-.71 1.15-.62 1.83.66.05 1.34-.33 1.75-.83z" />
      </svg>
    )
  }
  if (machine === 'win') {
    return (
      <svg className="machine-glyph win" width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-label="win">
        <path d="M0 2.25 6.5 1.35v6.4H0V2.25zM7.4 1.22 16 0v7.75H7.4V1.22zM0 8.55h6.5v6.4L0 14.05V8.55zM7.4 8.55H16V16l-8.6-1.2V8.55z" />
      </svg>
    )
  }
  return <span className="machine-glyph-none" aria-hidden="true" />
}

function HbClockIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 12 12" fill="none" style={{ flex: 'none' }}>
      <circle cx="6" cy="6" r="4.6" stroke="currentColor" strokeWidth="1.1" />
      <path d="M6 3.4V6l1.8 1.1" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

function TagPill({ k, v }: { k: string; v: string }) {
  return (
    <span className="shield">
      <span className="shield-k">{k}</span>
      <span className="shield-v" style={{ background: valueColor(v) }}>
        {v}
      </span>
    </span>
  )
}

type Props = {
  agents: Agent[]
  // uuid -> 방금 heartbeat 가 갱신돼 pulse 애니를 잠깐 보여줄지 여부.
  pulses: Record<string, boolean>
}

export default function Roster({ agents, pulses }: Props) {
  // 현재 총괄(내가 앉은 머신). 클릭해 지정, 브라우저별 localStorage 보관.
  const [boss, setBoss] = useState<string>(() => {
    try {
      return localStorage.getItem(BOSS_KEY) ?? ''
    } catch {
      return ''
    }
  })
  const toggleBoss = (uuid: string) => {
    const next = boss === uuid ? '' : uuid
    setBoss(next)
    try {
      localStorage.setItem(BOSS_KEY, next)
    } catch {
      // 저장 불가 환경은 무시.
    }
  }

  // online 먼저, offline 뒤(서버는 uuid 오름차순만 보장).
  const sorted = [...agents].sort((a, b) => Number(b.online) - Number(a.online))
  const onlineCount = agents.filter((a) => a.online).length

  return (
    <section className="roster-section">
      <div className="panel-header">
        <h2 className="section-title">관리자 로스터</h2>
        <span className="section-count">
          {onlineCount}/{agents.length} online
        </span>
      </div>
      <div className="roster-list">
        {sorted.length === 0 ? (
          <div className="roster-empty">등록된 관리자 없음.</div>
        ) : (
          sorted.map((s) => {
            const pulse = !!pulses[s.uuid]
            const isBoss = boss === s.uuid
            return (
              <div className={'roster-row' + (s.online ? '' : ' offline')} key={s.uuid}>
                <div className="card-row">
                  <span className="status-dot-wrap">
                    <span className={'status-dot' + (s.online ? ' online' : '')} />
                    {pulse ? <span className="status-ping" /> : null}
                  </span>
                  <MachineGlyph machine={s.tags.machine} />
                  <span className="roster-uuid">{s.display_name ?? s.uuid}</span>
                  <button
                    type="button"
                    className={'boss-toggle' + (isBoss ? ' on' : '')}
                    onClick={() => toggleBoss(s.uuid)}
                    title={isBoss ? '현재 총괄(클릭해 해제)' : '클릭해 현재 총괄으로 지정'}
                    aria-label="현재 총괄 지정"
                  >
                    {isBoss ? '★' : '☆'}
                  </button>
                  {isBoss ? <span className="pill-boss">현재 총괄</span> : null}
                  {s.online ? (
                    <span className="hb-dots">
                      {Array.from({ length: N_DOTS }, (_, i) => (
                        <span key={i} className="hb-dot" style={{ animationDelay: (i * 0.09).toFixed(2) + 's' }} />
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
            )
          })
        )}
      </div>
    </section>
  )
}
