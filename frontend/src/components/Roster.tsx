// 관리자 로스터: 피드와 동일한 패널+행 레이아웃. 각 관리자 = 상태닷·머신아이콘·이름·heartbeat·태그.
// 총괄은 별도 카드가 아니라 대등한 행에 "현재 총괄" 뱃지로 표식(클릭해 지정, 앉는 머신 따라 바뀜).
import { useState } from 'react'
import { relativeTime } from '../api'
import type { SessionRow } from '../activity'

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

// 미무장(heartbeat 없는) 세션의 활동 경과 라벨. ageSecs(jsonl 활동 이후 초)에서 대략 표기.
function agoLabel(secs: number): string {
  if (secs < 60) return '방금'
  const m = Math.floor(secs / 60)
  if (m < 60) return `${m}분 전`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}시간 전`
  return `${Math.floor(h / 24)}일 전`
}

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

// 값만 표시하는 뱃지(라벨/타이틀 없이 - 값이 자명: win/claude/supervised/프로젝트명).
// 무엇의 값인지는 hover title로만 남긴다.
function TagPill({ k, v }: { k: string; v: string }) {
  return (
    <span className="shield-v shield-solo" style={{ background: valueColor(v) }} title={k}>
      {v}
    </span>
  )
}

type Props = {
  // 활성 세션(활동 age < 60분). 이미 age 오름차순(최근 먼저)으로 정렬돼 들어온다(설계 v2-41).
  rows: SessionRow[]
  // uuid -> 방금 heartbeat 가 갱신돼 pulse 애니를 잠깐 보여줄지 여부.
  pulses: Record<string, boolean>
  // 자동 총감독 후보(활성 중 사람 입력 최신). 수동 ★ override가 없으면 이걸 총감독으로 표식.
  autoBossUuid: string
}

export default function Roster({ rows, pulses, autoBossUuid }: Props) {
  // 수동 총감독 override. 비어 있으면 autoBossUuid(자동최신)를 쓴다. 클릭 토글 = override 설정/해제.
  const [manualBoss, setManualBoss] = useState<string>(() => {
    try {
      return localStorage.getItem(BOSS_KEY) ?? ''
    } catch {
      return ''
    }
  })
  const effectiveBoss = manualBoss || autoBossUuid
  const toggleBoss = (uuid: string) => {
    const next = manualBoss === uuid ? '' : uuid
    setManualBoss(next)
    try {
      localStorage.setItem(BOSS_KEY, next)
    } catch {
      // 저장 불가 환경은 무시.
    }
  }

  // 총감독은 항상 최상단(활동 age와 무관). 나머지는 App이 준 age 오름차순 유지.
  const sorted = [...rows].sort(
    (a, b) => Number(b.uuid === effectiveBoss) - Number(a.uuid === effectiveBoss),
  )

  return (
    <section className="roster-section">
      <div className="panel-header">
        <h2 className="section-title">관리자 로스터</h2>
        <span className="section-count">{sorted.length} 활성</span>
      </div>
      <div className="roster-list">
        {sorted.length === 0 ? (
          <div className="roster-empty">활성 세션 없음.</div>
        ) : (
          sorted.map((s) => {
            const pulse = !!pulses[s.uuid]
            const isBoss = effectiveBoss === s.uuid
            const name = s.label
            const activityLabel = s.lastHeartbeat ? relativeTime(s.lastHeartbeat) : agoLabel(s.ageSecs)
            return (
              <div className={'roster-row' + (s.armed && !s.online ? ' offline' : '')} key={s.uuid}>
                <div className="card-row">
                  <span className="status-dot-wrap">
                    <span className={'status-dot' + (s.online ? ' online' : '')} />
                    {pulse ? <span className="status-ping" /> : null}
                  </span>
                  <MachineGlyph machine={s.machine ?? undefined} />
                  <span className="roster-uuid">{name || s.uuid}</span>
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
                  {!s.armed ? <span className="pill-unarmed" title="poll 미등록 = A2A 수신 불가(발견만)">미무장</span> : null}
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
                    <span className="hb-label">{activityLabel}</span>
                  </span>
                </div>
                <div className="tag-row">
                  {orderedTags(s.tags)
                    .filter(([k]) => TAG_ORDER.includes(k))
                    .map(([k, v]) => (
                      <TagPill key={k} k={k} v={v} />
                    ))}
                </div>
                <div className="tag-row session-row">
                  <span className="shield-session" title="session / uuid">
                    {s.tags.session ?? s.uuid}
                  </span>
                </div>
              </div>
            )
          })
        )}
      </div>
    </section>
  )
}
