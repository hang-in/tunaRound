// 관리자 로스터(사이드바): 머신 그룹 + 세션 행(mono id, ★ boss 강조, runner pill, 상대시간).
// 목업 aside.sidebar 이식. 행 클릭=확장 상세(uuid 복사·이 세션에 목표). buildRoster·★ autoBoss 도출 보존.
import { useState } from 'react'
import { relativeTime } from '../api'
import type { SessionRow } from '../activity'
import { RunnerIcon } from './runnerIcons'

type Props = {
  rows: SessionRow[]
  // uuid -> 방금 heartbeat 가 갱신돼 잠깐 강조할지 여부.
  pulses: Record<string, boolean>
  // 총감독(활성 중 사람 입력 최신). 없으면 ''.
  autoBossUuid: string
  // 상세의 "이 세션에 목표" -> 목표 모달 대상에 추가(App이 배선).
  onAddTarget?: (uuid: string) => void
}

// 머신 letter-glyph(목업 .glyph). win→W, mac→M, 기타→첫 글자 대문자.
function machineLetter(m: string): string {
  if (m === 'win') return 'W'
  if (m === 'mac') return 'M'
  return (m[0] ?? '·').toUpperCase()
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

  // 머신 그룹 순서: 총감독 머신 → win → mac → 나머지 알파벳.
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
    <div className="side-block">
      <div className="sh">
        <h2>관리자 로스터</h2>
        <span className="count">{rows.length} online</span>
      </div>
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
              <div className="mgroup">
                <div className="mh">
                  <span className="glyph">{machineLetter(m)}</span>
                  <span className="label">{m}</span>
                </div>
              </div>
              {sorted.map((s) => {
                const isBoss = s.uuid === autoBossUuid
                const fresh = Boolean(pulses[s.uuid])
                const open = Boolean(expanded[s.uuid])
                const title = (s.project ?? s.label) + (suffix.get(s.uuid) ?? '')
                const sessionId = s.tags.session ?? s.uuid
                // .rr 우측: boss는 heartbeat rel, 사람 입력 이력 있으면 ★+humanInputAt rel, 아니면 heartbeat rel.
                const tail =
                  isBoss || !s.humanInputAt
                    ? relativeTime(s.lastHeartbeat)
                    : '★ ' + relativeTime(s.humanInputAt)
                return (
                  <div key={s.uuid}>
                    <button
                      type="button"
                      className={`srow${isBoss ? ' boss' : ''}${fresh ? ' fresh' : ''}${s.busy ? ' working' : ''}`}
                      aria-expanded={open}
                      onClick={() => toggle(s.uuid)}
                    >
                      <RunnerIcon runner={s.runner} size={14} />
                      <span className="rid" title={s.uuid}>
                        {title}
                      </span>
                      {isBoss ? (
                        <span className="star" title="현재 총감독(사람이 마지막으로 입력한 세션)">
                          ★
                        </span>
                      ) : null}
                      <span className="rr">
                        {s.busy ? (
                          <span
                            className="spinner"
                            title="지금 task 처리 중"
                            aria-label="working"
                          />
                        ) : null}
                        <span className="pill on">{s.runner ?? '?'}</span>
                        {tail}
                      </span>
                    </button>
                    {open ? (
                      <div className="srow-detail">
                        <span className="srow-uuid" title="session / uuid">
                          {sessionId}
                        </span>
                        <button
                          type="button"
                          className="srow-detail-btn"
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
                            className="srow-detail-btn"
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
      <div className="side-empty">rail 하단은 비워둠 · 필요한 패널을 여기 얹습니다</div>
    </div>
  )
}
