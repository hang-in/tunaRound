// 목표 제출 모달(목업 .modal): 대상 칩 + "+ 대상 추가" 드롭다운(머신별) + 프리셋 + textarea + 제출.
// 헤더 버튼이 오픈. 원격(remoteViewer)이면 폼 대신 관전 경고. Esc/스크림 클릭 닫기 + 오픈 시 textarea 포커스.
// 선택 상태는 App이 소유(로스터 상세의 "이 세션에 목표"와 공유). loopback 제출/원격 403 태세 보존.
import { useEffect, useRef, useState } from 'react'
import { X } from 'lucide-react'
import type { Agent } from '../api'
import { relativeTime, sendGoal } from '../api'
import { RunnerIcon } from './runnerIcons'

type Props = {
  open: boolean
  onClose: () => void
  agents: Agent[]
  remoteViewer: boolean
  // 선택 상태(App 소유): uuid -> 선택 여부.
  selected: Record<string, boolean>
  onChangeSelected: (next: Record<string, boolean>) => void
}

// 칩·드롭다운의 표시 이름 = project(로스터 타이틀과 동일 규약). 같은 project·runner 충돌은 uuid로 식별.
function agentTitle(a: Agent): string {
  return a.tags?.project ?? a.display_name ?? a.uuid.slice(0, 8)
}

export default function GoalForm({ open, onClose, agents, remoteViewer, selected, onChangeSelected }: Props) {
  const [goal, setGoal] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [status, setStatus] = useState('')
  const [pickerOpen, setPickerOpen] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement | null>(null)
  const modalRef = useRef<HTMLDivElement | null>(null)
  const triggerRef = useRef<HTMLElement | null>(null)

  // 포커스 트랩: 오픈 시 트리거를 기억+textarea 포커스, Esc 닫기, Tab을 모달 안에 가둔다(첫↔끝 순환),
  // 닫힐 때(cleanup) 트리거로 포커스 복원.
  useEffect(() => {
    if (!open) return
    triggerRef.current = document.activeElement as HTMLElement | null
    const focusables = (): HTMLElement[] => {
      if (!modalRef.current) return []
      const sel = 'button, [href], input, textarea, select, [tabindex]:not([tabindex="-1"])'
      return Array.from(modalRef.current.querySelectorAll<HTMLElement>(sel)).filter(
        (el) => !el.hasAttribute('disabled') && el.offsetParent !== null,
      )
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose()
        return
      }
      if (e.key === 'Tab') {
        const list = focusables()
        if (list.length === 0) return
        const first = list[0]
        const last = list[list.length - 1]
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault()
          last.focus()
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault()
          first.focus()
        }
      }
    }
    document.addEventListener('keydown', onKey)
    const t = window.setTimeout(() => textareaRef.current?.focus(), 0)
    return () => {
      document.removeEventListener('keydown', onKey)
      window.clearTimeout(t)
      triggerRef.current?.focus?.() // 닫힐 때 원래 트리거(목표 제출 버튼)로 포커스 복원.
    }
  }, [open, onClose])

  // 목표 대상 = claude 세션 + codex 세션(v2-46). 제외: 워커/infra/relay 없는 머신의 codex.
  const relayMachines = new Set(
    agents
      .filter((a) => a.online && a.tags?.role === 'infra' && a.tags?.purpose === 'codex-inject')
      .map((a) => a.tags?.machine)
      .filter((m): m is string => !!m),
  )
  const online = agents.filter((a) => {
    if (!a.online || a.tags?.role === 'worker' || a.tags?.role === 'infra') return false
    if (a.tags?.runner === 'codex') return !!a.tags?.machine && relayMachines.has(a.tags.machine)
    return true
  })

  if (!open) return null

  // 스크림 클릭(모달 바깥)만 닫는다.
  const onScrimClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose()
  }

  const header = (
    <div className="mhd">
      <h3>목표 제출</h3>
      <button type="button" className="x" onClick={onClose} aria-label="닫기">
        <X size={16} />
      </button>
    </div>
  )

  if (remoteViewer) {
    return (
      <div className="modal-scrim" onClick={onScrimClick} role="dialog" aria-modal="true" aria-label="목표 제출">
        <div className="modal" ref={modalRef}>
          {header}
          <div className="mbd">
            <div className="goal-warning">
              원격 관전 모드입니다 - 목표 제출은 로컬(총괄) 세션에서만 가능합니다.
            </div>
          </div>
        </div>
      </div>
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
    <div className="modal-scrim" onClick={onScrimClick} role="dialog" aria-modal="true" aria-label="목표 제출">
      <div className="modal">
        {header}
        <div className="mbd">
          <div>
            <div className="label lbl">대상</div>
            <div className="chips">
              {picked.length === 0 ? (
                <span className="chip empty">대상 없음 - 프리셋이나 + 대상 추가로 선택하세요</span>
              ) : null}
              {picked.map((a) => (
                <span className="chip" key={a.uuid}>
                  <RunnerIcon runner={a.tags?.runner ?? null} size={12} />
                  {agentTitle(a)}
                  <span className="sub">{a.tags?.machine ?? '?'}</span>
                  <button type="button" className="x" aria-label="대상 제거" onClick={() => toggleOne(a.uuid)}>
                    <X size={12} />
                  </button>
                </span>
              ))}
              <button type="button" className="chip add" onClick={() => setPickerOpen((o) => !o)} aria-expanded={pickerOpen}>
                + 대상 추가
              </button>
            </div>
          </div>

          <div className="gf-presets">
            <span className="label">빠른 선택</span>
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
                        <button
                          type="button"
                          key={a.uuid}
                          className={`gf-picker-item${isSel ? ' checked' : ''}`}
                          role="option"
                          aria-selected={isSel}
                          onClick={() => toggleOne(a.uuid)}
                        >
                          <RunnerIcon runner={a.tags?.runner ?? null} size={12} />
                          {agentTitle(a)}
                          {a.tags?.runner === 'codex' ? <span className="runner">· codex</span> : null}
                          <span className="sub">{isSel ? '선택됨 ✓' : relativeTime(a.last_heartbeat)}</span>
                        </button>
                      )
                    })}
                </div>
              ))}
              <div className="gf-picker-note">오프라인 세션은 제외됩니다. 로스터 행의 &quot;이 세션에 목표&quot;로도 추가할 수 있습니다.</div>
            </div>
          ) : null}

          <textarea
            ref={textareaRef}
            rows={3}
            placeholder="예: tunaround 저장소의 flaky 테스트를 찾아 원인 분석 후 수정 PR을 올려줘"
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
          />

          <div className="mft">
            <span className="label">
              {picked.length > 0 ? `선택 ${picked.length}명 · 로컬 발행` : '대상을 선택하세요'}
            </span>
            <span className="spacer" />
            <button type="button" className="btn ghost" onClick={onClose}>
              취소
            </button>
            <button type="button" className="btn" disabled={!canSubmit || submitting} onClick={onSubmit}>
              {submitting ? '제출 중…' : picked.length > 0 ? `${picked.length}명에게 제출` : '제출'}
            </button>
          </div>
          {status ? <span className="status">{status}</span> : null}
        </div>
      </div>
    </div>
  )
}
