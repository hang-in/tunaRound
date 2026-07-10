// 관리자 로스터: 머신별 그룹 + 1줄 행(프로젝트·러너·상태만). 이름/뱃지 3중 중복 제거 리디자인.
// role=session(기본값)은 숨기고 예외(supervised 등)만 칩. uuid는 행 클릭 확장 상세로(복사·목표 추가).
import { useState } from 'react'
import { relativeTime } from '../api'
import type { SessionRow } from '../activity'
import { RunnerIcon } from './runnerIcons'

// 태그 값별 색(워커 섹션 TagPill 재사용분). 알려진 값은 고정, 나머지는 해시 팔레트.
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

// 머신 브랜드 글리프(그룹 헤더·워커 섹션 재사용).
export function MachineGlyph({ machine }: { machine: string | undefined }) {
  if (machine === 'mac') {
    return (
      <svg className="machine-glyph" width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-label="mac">
        <path d="M11.05 8.36c-.02-1.5 1.22-2.22 1.28-2.26-.7-1.02-1.79-1.16-2.17-1.18-.92-.09-1.8.54-2.27.54-.47 0-1.19-.53-1.96-.51-1.01.01-1.94.59-2.46 1.49-1.05 1.82-.27 4.52.75 6 .5.73 1.09 1.54 1.87 1.51.75-.03 1.03-.48 1.94-.48.9 0 1.16.48 1.95.47.81-.01 1.32-.74 1.81-1.47.57-.84.81-1.66.82-1.7-.02-.01-1.57-.6-1.59-2.38zM9.6 3.87c.41-.5.69-1.2.61-1.9-.59.02-1.31.4-1.74.9-.38.44-.71 1.15-.62 1.83.66.05 1.34-.33 1.75-.83z" />
      </svg>
    )
  }
  if (machine === 'win') {
    return (
      <svg className="machine-glyph win" width="12" height="12" viewBox="0 0 16 16" fill="currentColor" aria-label="win">
        <path d="M0 2.25 6.5 1.35v6.4H0V2.25zM7.4 1.22 16 0v7.75H7.4V1.22zM0 8.55h6.5v6.4L0 14.05V8.55zM7.4 8.55H16V16l-8.6-1.2V8.55z" />
      </svg>
    )
  }
  return <span className="machine-glyph-none" aria-hidden="true" />
}

// 값만 표시하는 뱃지(워커 섹션 전용으로 유지 - 로스터 본문에서는 예외 role 칩만 쓴다).
export function TagPill({ k, v }: { k: string; v: string }) {
  return (
    <span className="shield-v shield-solo" style={{ background: valueColor(v) }} title={k}>
      {v}
    </span>
  )
}

type Props = {
  rows: SessionRow[]
  // uuid -> 방금 heartbeat 가 갱신돼 pulse 링을 잠깐 보여줄지 여부.
  pulses: Record<string, boolean>
  // 총감독(활성 중 사람 입력 최신).
  autoBossUuid: string
  // 상세의 "이 세션에 목표" -> 목표 제출 폼 선택에 추가(App이 배선).
  onAddTarget?: (uuid: string) => void
}

// 그룹 내 같은 (runner, project) 세션이 여럿이면 두 번째부터 ·B ·C 접미(uuid 정렬 순).
function titleSuffixes(rows: SessionRow[]): Map<string, string> {
  const groups = new Map<string, SessionRow[]>()
  rows.forEach((r) => {
    const key = `${r.runner ?? '?'}|${r.project ?? r.label}`
    const bucket = groups.get(key)
    if (bucket) bucket.push(r)
    else groups.set(key, [r])
  })
  const out = new Map<string, string>()
  for (const group of groups.values()) {
    if (group.length === 1) continue
    ;[...group]
      .sort((a, b) => a.uuid.localeCompare(b.uuid))
      .forEach((r, i) => {
        if (i > 0) out.set(r.uuid, ` ·${String.fromCharCode(65 + i)}`) // ·B, ·C ...
      })
  }
  return out
}

export default function Roster({ rows, pulses, autoBossUuid, onAddTarget }: Props) {
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})
  const [copied, setCopied] = useState('')

  // 머신 그룹: 총감독 머신 → win → mac → 나머지 알파벳.
  const bossMachine = rows.find((r) => r.uuid === autoBossUuid)?.machine ?? null
  const machines = [...new Set(rows.map((r) => r.machine ?? '기타'))]
  machines.sort((a, b) => {
    const rank = (m: string) => (m === bossMachine ? 0 : m === 'win' ? 1 : m === 'mac' ? 2 : 3)
    return rank(a) - rank(b) || a.localeCompare(b)
  })

  const toggle = (uuid: string) => setExpanded((p) => ({ ...p, [uuid]: !p[uuid] }))

  const copyUuid = async (uuid: string) => {
    try {
      await navigator.clipboard.writeText(uuid)
      setCopied(uuid)
      window.setTimeout(() => setCopied((c) => (c === uuid ? '' : c)), 1500)
    } catch {
      // clipboard 미지원(비보안 컨텍스트 등)이면 조용히 무시 - uuid는 화면에 이미 보인다.
    }
  }

  return (
    <section className="roster-section">
      <div className="panel-header">
        <h2 className="section-title">관리자 로스터</h2>
        <span className="section-count">{rows.length} online</span>
      </div>
      <div className="roster-list">
        {rows.length === 0 ? (
          <div className="roster-empty">열린 세션 없음.</div>
        ) : (
          machines.map((m) => {
            const group = rows.filter((r) => (r.machine ?? '기타') === m)
            const suffix = titleSuffixes(group)
            const sorted = [...group].sort(
              (a, b) =>
                (a.uuid === autoBossUuid ? 0 : 1) - (b.uuid === autoBossUuid ? 0 : 1) ||
                (a.project ?? a.label).localeCompare(b.project ?? b.label),
            )
            return (
              <div key={m}>
                <div className="rst-group">
                  <MachineGlyph machine={m === '기타' ? undefined : m} />
                  <span className="rst-group-name">{m}</span>
                  <span className="rst-group-count">· {group.length}</span>
                  <span className="rst-group-line" />
                </div>
                {sorted.map((s) => {
                  const isBoss = s.uuid === autoBossUuid
                  const pulse = Boolean(pulses[s.uuid])
                  const open = Boolean(expanded[s.uuid])
                  const role = s.tags.role
                  const title = (s.project ?? s.label) + (suffix.get(s.uuid) ?? '')
                  const sessionId = s.tags.session ?? s.uuid
                  return (
                    <div key={s.uuid}>
                      <div
                        className={`rst-row${isBoss ? ' boss' : ''}${pulse ? ' fresh' : ''}`}
                        role="button"
                        tabIndex={0}
                        aria-expanded={open}
                        onClick={() => toggle(s.uuid)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' || e.key === ' ') {
                            e.preventDefault()
                            toggle(s.uuid)
                          }
                        }}
                      >
                        <span className="rst-dot-wrap">
                          <span className="rst-dot" />
                          {pulse ? <span className="rst-ping" /> : null}
                        </span>
                        <RunnerIcon runner={s.runner} />
                        <span className="rst-title">{title}</span>
                        <span className="rst-runner-name">{s.runner ?? '?'}</span>
                        {isBoss ? (
                          <>
                            <span className="boss-star on" title="현재 총감독(사람이 마지막으로 입력한 세션)">
                              ★
                            </span>
                            <span className="rst-chip boss">총괄</span>
                          </>
                        ) : null}
                        {role && role !== 'session' ? (
                          <span className={`rst-chip role-${role}`}>{role}</span>
                        ) : null}
                        <span className="dash-spacer" />
                        <span className="rst-hb">{relativeTime(s.lastHeartbeat)}</span>
                        <span className="rst-caret" aria-hidden="true">
                          ▸
                        </span>
                      </div>
                      {open ? (
                        <div className="rst-detail">
                          <span className="rst-uuid" title="session / uuid">
                            {sessionId}
                          </span>
                          <button
                            type="button"
                            className="rst-detail-btn"
                            onClick={(e) => {
                              e.stopPropagation()
                              copyUuid(s.uuid) // 내부에서 실패를 삼키는 fire-and-forget.
                            }}
                          >
                            {copied === s.uuid ? '복사됨 ✓' : 'uuid 복사'}
                          </button>
                          {onAddTarget ? (
                            <button
                              type="button"
                              className="rst-detail-btn"
                              onClick={(e) => {
                                e.stopPropagation()
                                onAddTarget(s.uuid)
                              }}
                            >
                              이 세션에 목표
                            </button>
                          ) : null}
                        </div>
                      ) : null}
                    </div>
                  )
                })}
              </div>
            )
          })
        )}
      </div>
    </section>
  )
}
