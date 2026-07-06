// 발견된 세션(후보) 패널: 발견 리포터가 보고한 미무장 세션을 뜬다(v2-40 S2/S3). 5초 폴로 자족 갱신.
// armed(=online roster 소속)는 이미 무장돼 로스터에 있으므로 여기선 후보(armed=false)만 노출한다.
// claude 세션은 외부 제어 소켓이 없어(발견≠제어) "연결"은 세션 id 복사 + 수동 무장 안내에 그친다.
import { useEffect, useState } from 'react'
import type { Candidate } from '../api'
import { fetchCandidates } from '../api'

// 러너·머신 값별 색(Roster의 shield 색과 통일).
const RUNNER_COLOR: Record<string, string> = {
  claude: '#c15f3c',
  codex: '#10a37f',
  gemini: '#4285f4',
}
function runnerColor(runner: string): string {
  return RUNNER_COLOR[runner] ?? '#57606a'
}
const MACHINE_COLOR: Record<string, string> = {
  mac: '#6e7681',
  win: '#0078d4',
  linux: '#f0883e',
  unix: '#8791a3',
}
function machineColor(machine: string): string {
  return MACHINE_COLOR[machine] ?? '#57606a'
}

// 활동 경과 초를 사람이 읽는 상대시간으로.
function formatAge(sec: number): string {
  if (sec < 5) return '방금'
  if (sec < 60) return sec + '초 전'
  const min = Math.floor(sec / 60)
  if (min < 60) return min + '분 전'
  const hr = Math.floor(min / 60)
  if (hr < 24) return hr + '시간 전'
  return Math.floor(hr / 24) + '일 전'
}

function Pill({ k, v, color }: { k: string; v: string; color?: string }) {
  return (
    <span className="shield">
      <span className="shield-k">{k}</span>
      <span className="shield-v" style={color ? { background: color } : undefined}>
        {v}
      </span>
    </span>
  )
}

export default function Candidates() {
  const [candidates, setCandidates] = useState<Candidate[]>([])
  const [copied, setCopied] = useState<string>('')

  // /dashboard/candidates 를 5초 주기로 폴링한다(roster 폴 주기와 통일).
  useEffect(() => {
    let cancelled = false
    const controller = new AbortController()
    const load = () => {
      fetchCandidates(controller.signal)
        .then((list) => {
          if (!cancelled) setCandidates(list)
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          console.error('[candidates] 조회 실패.', err)
        })
    }
    load()
    const timer = window.setInterval(load, 5000)
    return () => {
      cancelled = true
      controller.abort()
      window.clearInterval(timer)
    }
  }, [])

  // armed(이미 로스터에 있음)는 제외하고 미무장 후보만 노출한다.
  const unarmed = candidates.filter((c) => !c.armed).sort((a, b) => a.age_secs - b.age_secs)

  const onConnect = (uuid: string) => {
    // 클립보드 API가 없으면 no-op(실제 복사 안 됐는데 "복사됨"이 뜨지 않게).
    if (!navigator.clipboard) return
    navigator.clipboard
      .writeText(uuid)
      .then(() => {
        setCopied(uuid)
        window.setTimeout(() => setCopied((c) => (c === uuid ? '' : c)), 1500)
      })
      .catch(() => {
        // 복사 실패는 무시(수동 복사).
      })
  }

  return (
    <section className="candidates-section">
      <div className="panel-header">
        <h2 className="section-title">발견된 세션</h2>
        <span className="section-count">{unarmed.length} 후보</span>
      </div>
      <div className="roster-list">
        {unarmed.length === 0 ? (
          <div className="roster-empty">발견된 미무장 세션 없음.</div>
        ) : (
          unarmed.map((c) => (
            <div className="roster-row" key={c.uuid}>
              <div className="card-row">
                <span className="status-dot-wrap">
                  <span className="status-dot candidate" />
                </span>
                <span className="roster-uuid">{c.uuid}</span>
                <span className="dash-spacer" />
                <span className="hb-label">활동 {formatAge(c.age_secs)}</span>
                <button
                  type="button"
                  className="candidate-arm"
                  onClick={() => onConnect(c.uuid)}
                  title="claude 세션은 외부 제어 소켓이 없어 무장은 그 세션에서 직접(TUNA_AUTOARM=1) 켭니다. 세션 id를 복사합니다."
                >
                  {copied === c.uuid ? '복사됨' : '연결'}
                </button>
              </div>
              <div className="tag-row">
                {c.machine ? <Pill k="machine" v={c.machine} color={machineColor(c.machine)} /> : null}
                <Pill k="runner" v={c.runner} color={runnerColor(c.runner)} />
                {c.project ? <Pill k="project" v={c.project} /> : null}
                <Pill k="source" v={c.source} />
              </div>
            </div>
          ))
        )}
      </div>
    </section>
  )
}
