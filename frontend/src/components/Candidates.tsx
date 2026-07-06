// 발견된 세션(후보) 패널: 발견 리포터가 보고한 미무장 세션을 뜬다(v2-40 S2/S3). 5초 폴로 자족 갱신.
// armed(=online roster 소속)는 이미 로스터에 있으므로 여기선 후보(armed=false)만 노출한다.
// claude 세션은 외부 제어 소켓이 없어(발견≠제어) "연결"은 그 세션에 붙여넣을 arm 프롬프트를 팝업으로 안내한다.
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

// 발견된 세션을 관리자로 편입시키기 위해 "그 세션에" 붙여넣을 자연어 arm 프롬프트를 만든다.
// claude는 외부 push가 없어 그 세션이 스스로 poll+Monitor를 켜야 하므로, 사람이 이 프롬프트를 그
// 세션에 붙여넣으면 세션이 자가 무장한다. core는 이 대시보드를 연 주소 기준(같은 머신 후보에 정확).
function buildArmPrompt(c: Candidate): string {
  const core = window.location.origin + '/mcp'
  const name = (c.machine ? c.machine + '-' : '') + c.runner + '-' + c.uuid.slice(0, 8)
  const tags = [
    'machine=' + (c.machine ?? 'unknown'),
    'runner=' + c.runner,
    'role=supervised',
    'project=' + (c.project ?? 'unknown'),
  ].join(',')
  return [
    '이 세션을 tunaRound 브로커에 "관리자"로 등록해줘.',
    '',
    '1) 백그라운드로 다음을 실행:',
    `   tunaround poll --core ${core} --agent ${c.uuid} --display-name ${name} --tags "${tags}" --interval 15`,
    '2) 그 poll 출력을 Monitor 도구로 감시하다가, task가 도착하면 claim_task로 잡아 처리하고 complete_task로 보고해줘.',
    '',
    '(토큰은 TUNA_BROKER_TOKEN 환경변수 사용. 원격 머신이면 --core를 그 머신에서 본 브로커 주소로 바꿔.)',
  ].join('\n')
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
  // "연결" 팝업 대상 후보(null이면 팝업 닫힘).
  const [armTarget, setArmTarget] = useState<Candidate | null>(null)
  const [copied, setCopied] = useState(false)

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

  const openArm = (c: Candidate) => {
    setArmTarget(c)
    setCopied(false)
  }
  const copyPrompt = () => {
    if (!armTarget || !navigator.clipboard) return
    navigator.clipboard
      .writeText(buildArmPrompt(armTarget))
      .then(() => {
        setCopied(true)
        window.setTimeout(() => setCopied(false), 1500)
      })
      .catch(() => {
        // 복사 실패는 무시(사용자가 수동 선택 복사).
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
                  onClick={() => openArm(c)}
                  title="이 세션을 관리자로 편입하는 방법을 안내합니다(그 세션에 붙여넣기)."
                >
                  연결
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

      {armTarget ? (
        <div className="modal-overlay" onClick={() => setArmTarget(null)}>
          <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h3 className="modal-title">관리자로 연결</h3>
              <button type="button" className="modal-close" onClick={() => setArmTarget(null)} aria-label="닫기">
                ✕
              </button>
            </div>
            <p className="modal-desc">
              claude 세션은 밖에서 밀어넣을 수 없어, 아래를 <b>그 세션</b>(uuid {armTarget.uuid.slice(0, 8)}…
              {armTarget.machine ? `, ${armTarget.machine}` : ''})의 Claude Code 프롬프트에 붙여넣으면 세션이 스스로
              무장해 <b>관리자 로스터</b>로 올라옵니다.
            </p>
            <pre className="control-answer modal-prompt">{buildArmPrompt(armTarget)}</pre>
            <div className="modal-actions">
              <button type="button" className="goal-submit-btn enabled" onClick={copyPrompt}>
                {copied ? '복사됨' : '프롬프트 복사'}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </section>
  )
}
