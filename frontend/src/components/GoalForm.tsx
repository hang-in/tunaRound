// 목표 제출: 선택된 대상 칩 + "+ 대상 추가" 드롭다운(머신별 그룹) + 프리셋(전체/win만/mac만/codex만).
// 체크박스 나열 제거 리디자인. 선택 상태는 App이 소유(로스터 상세의 "이 세션에 목표"와 공유).
import { useState } from 'react'
import type { Agent } from '../api'
import { relativeTime, sendGoal } from '../api'
import { RunnerIcon } from './runnerIcons'

type Props = {
  agents: Agent[]
  remoteViewer: boolean
  // 선택 상태(App 소유): uuid -> 선택 여부.
  selected: Record<string, boolean>
  onChangeSelected: (next: Record<string, boolean>) => void
}

function WarnIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
      <path d="M7 1.5 13 12H1L7 1.5Z" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
      <path d="M7 5.5v3M7 10.2v0.1" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
    </svg>
  )
}

// 칩·드롭다운의 표시 이름 = project(로스터 타이틀과 동일 규약). 같은 project·runner 충돌은 uuid로 식별.
// infra(codex 주입 경로)는 project가 없으므로 용도가 드러나는 고정 라벨.
function agentTitle(a: Agent): string {
  if (a.tags?.role === 'infra') {
    return `${a.tags?.machine ?? '?'}-codex 주입`
  }
  return a.tags?.project ?? a.display_name ?? a.uuid.slice(0, 8)
}

export default function GoalForm({ agents, remoteViewer, selected, onChangeSelected }: Props) {
  const [goal, setGoal] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [status, setStatus] = useState('')
  const [pickerOpen, setPickerOpen] = useState(false)

  // 목표 대상 = claude 세션 + codex 주입 infra. 제외 3종(전부 no-consumer 방지):
  //   워커(work 데몬이 별도 소비) / presence 스캐너(task 처리 자리 아님) /
  //   codex 세션·role 미지정 codex(자기 poll이 없어 수신 불가 - codex 경로는 codex-inject watcher만).
  const online = agents.filter((a) => {
    if (!a.online || a.tags?.role === 'worker') return false
    if (a.tags?.role === 'infra') return a.tags?.purpose === 'codex-inject'
    // role 미지정도 세션 취급이므로 runner 기준으로 codex를 막는다(봇리뷰: undefined role 누락).
    return a.tags?.runner !== 'codex'
  })

  if (remoteViewer) {
    return (
      <section className="goal-section">
        <div className="goal-head">
          <h2 className="section-title">목표 제출</h2>
          <span className="goal-hint">선택한 대상 각각에게 목표가 전달됩니다</span>
        </div>
        <div className="goal-warning">
          <WarnIcon />
          원격 관전 모드입니다 — 목표 제출은 로컬(총괄) 세션에서만 가능합니다.
        </div>
      </section>
    )
  }

  const picked = online.filter((a) => selected[a.uuid])
  const canSubmit = goal.trim().length > 0 && picked.length > 0

  // 프리셋: online 중 조건에 맞는 대상으로 선택을 통째로 교체한다.
  const applyPreset = (pred: (a: Agent) => boolean) => {
    const next: Record<string, boolean> = {}
    online.forEach((a) => {
      if (pred(a)) next[a.uuid] = true
    })
    onChangeSelected(next)
  }

  const toggleOne = (uuid: string) => {
    onChangeSelected({ ...selected, [uuid]: !selected[uuid] })
  }

  const onSubmit = async () => {
    const text = goal.trim()
    const targets = picked.map((a) => a.uuid)
    if (!text || targets.length === 0) return
    setSubmitting(true)
    setStatus('')
    try {
      const outcome = await sendGoal(text, targets)
      if (outcome.kind === 'ok') {
        setGoal('')
        setStatus(`${outcome.created.length}개 task 생성됨.`)
      } else if (outcome.kind === 'forbidden') {
        setStatus('원격 세션에서는 목표를 제출할 수 없습니다(403).')
      } else {
        setStatus(outcome.message)
      }
    } catch (err) {
      console.error('[goal] 제출 실패.', err)
      setStatus('제출 실패: 네트워크 또는 서버 오류.')
    } finally {
      setSubmitting(false)
    }
  }

  // 드롭다운도 로스터와 같은 머신 그룹으로.
  const machines = [...new Set(online.map((a) => a.tags?.machine ?? '기타'))].sort(
    (a, b) => (a === 'win' ? 0 : a === 'mac' ? 1 : 2) - (b === 'win' ? 0 : b === 'mac' ? 1 : 2) || a.localeCompare(b),
  )

  return (
    <section className="goal-section">
      <div className="goal-head">
        <h2 className="section-title">목표 제출</h2>
        <span className="goal-hint">선택한 대상 각각에게 독립 task로 전달됩니다</span>
        <div className="gf-presets">
          <span className="gf-preset-label">빠른 선택</span>
          <button type="button" className="gf-preset" onClick={() => applyPreset(() => true)}>
            전체
          </button>
          <button type="button" className="gf-preset" onClick={() => applyPreset((a) => a.tags?.machine === 'win')}>
            win만
          </button>
          <button type="button" className="gf-preset" onClick={() => applyPreset((a) => a.tags?.machine === 'mac')}>
            mac만
          </button>
          <button type="button" className="gf-preset" onClick={() => applyPreset((a) => a.tags?.runner === 'codex')}>
            codex만
          </button>
          {picked.length > 0 ? (
            <button type="button" className="gf-preset clear" onClick={() => onChangeSelected({})}>
              비우기
            </button>
          ) : null}
        </div>
      </div>
      <div className="goal-body">
        <div className="gf-targets">
          {picked.length === 0 ? <span className="gf-empty">대상 없음 — 프리셋이나 + 대상 추가로 선택하세요</span> : null}
          {picked.map((a) => (
            <span className="gf-chip" key={a.uuid}>
              <RunnerIcon runner={a.tags?.runner ?? null} size={12} />
              {agentTitle(a)}
              <span className="gf-chip-sub">{a.tags?.machine ?? '?'}</span>
              <button type="button" className="gf-chip-x" aria-label="대상 제거" onClick={() => toggleOne(a.uuid)}>
                ×
              </button>
            </span>
          ))}
          <button type="button" className="gf-add" onClick={() => setPickerOpen((o) => !o)} aria-expanded={pickerOpen}>
            + 대상 추가
          </button>
        </div>

        {pickerOpen ? (
          <div className="gf-picker" role="listbox" aria-label="대상 선택">
            {machines.map((m) => (
              <div key={m}>
                <div className="gf-picker-group">{m}</div>
                {online
                  .filter((a) => (a.tags?.machine ?? '기타') === m)
                  .map((a) => {
                    const isSel = Boolean(selected[a.uuid])
                    return (
                      <div
                        key={a.uuid}
                        className={`gf-picker-item${isSel ? ' checked' : ''}`}
                        role="option"
                        aria-selected={isSel}
                        tabIndex={0}
                        onClick={() => toggleOne(a.uuid)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' || e.key === ' ') {
                            e.preventDefault()
                            toggleOne(a.uuid)
                          }
                        }}
                      >
                        <RunnerIcon runner={a.tags?.runner ?? null} size={12} />
                        {agentTitle(a)}
                        {a.tags?.runner === 'codex' ? <span className="gf-item-runner">· codex</span> : null}
                        <span className="gf-item-sub">{isSel ? '선택됨 ✓' : relativeTime(a.last_heartbeat)}</span>
                      </div>
                    )
                  })}
              </div>
            ))}
            <div className="gf-picker-note">오프라인 세션은 제외됩니다. 로스터 행의 &quot;이 세션에 목표&quot;로도 추가할 수 있습니다.</div>
          </div>
        ) : null}

        <textarea
          className="goal-textarea"
          rows={3}
          placeholder="예: tunaround 저장소의 flaky 테스트를 찾아 원인 분석 후 수정 PR을 올려줘"
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
        />
        <div className="goal-submit-row">
          <span className="goal-summary">
            {picked.length > 0 ? `${picked.length}개 대상 선택됨 — 각각 독립 task로 생성됩니다` : '대상을 선택하세요'}
          </span>
          <span className="dash-spacer" />
          <button
            type="button"
            className={`goal-submit-btn${canSubmit && !submitting ? ' enabled' : ''}`}
            disabled={!canSubmit || submitting}
            onClick={onSubmit}
          >
            {picked.length > 0 ? `${picked.length}개 대상에게 목표 전달` : '목표 전달'}
          </button>
        </div>
        {status ? <span className="goal-status">{status}</span> : null}
      </div>
    </section>
  )
}
