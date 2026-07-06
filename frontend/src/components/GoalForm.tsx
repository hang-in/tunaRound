// 목표 텍스트 + online 관리자 멀티선택으로 POST /dashboard/goal 을 호출하는 폼.
// 목업 "목표 제출" 섹션 이식(토큰 입력칸 없음, loopback 무인증).
import { useEffect, useState } from 'react'
import type { Agent } from '../api'
import { sendGoal } from '../api'

type Props = {
  agents: Agent[]
  remoteViewer: boolean
}

function WarnIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
      <path d="M7 1.5 13 12H1L7 1.5Z" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
      <path d="M7 5.5v3M7 10.2v0.1" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
    </svg>
  )
}

export default function GoalForm({ agents, remoteViewer }: Props) {
  const [selected, setSelected] = useState<Record<string, boolean>>({})
  const [goal, setGoal] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [status, setStatus] = useState('')

  const online = agents.filter((a) => a.online)

  // 새로 나타난 online 관리자은 기본 선택 상태로 추가한다(기존 선택은 보존).
  useEffect(() => {
    setSelected((prev) => {
      let changed = false
      const next = { ...prev }
      online.forEach((a) => {
        if (!(a.uuid in next)) {
          next[a.uuid] = true
          changed = true
        }
      })
      return changed ? next : prev
    })
    // eslint 계열 룰이 없어도 online 목록 변화에만 반응하면 충분하다.
    // (agents 전체가 아니라 uuid 조합이 바뀔 때만 실행되도록 join 을 의존성으로 쓴다.)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [online.map((a) => a.uuid).join(',')])

  if (remoteViewer) {
    return (
      <section className="goal-section">
        <div className="goal-head">
          <h2 className="section-title">목표 제출</h2>
          <span className="goal-hint">선택한 관리자 각각에게 목표가 전달됩니다</span>
        </div>
        <div className="goal-warning">
          <WarnIcon />
          원격 관전 모드입니다 — 목표 제출은 로컬(총괄) 세션에서만 가능합니다.
        </div>
      </section>
    )
  }

  const selCount = online.filter((a) => selected[a.uuid]).length
  const allSelected = selCount === online.length && online.length > 0
  const canSubmit = goal.trim().length > 0 && selCount > 0

  const toggleAll = () => {
    if (allSelected) {
      const next: Record<string, boolean> = {}
      online.forEach((a) => {
        next[a.uuid] = false
      })
      setSelected((prev) => ({ ...prev, ...next }))
    } else {
      const next: Record<string, boolean> = {}
      online.forEach((a) => {
        next[a.uuid] = true
      })
      setSelected((prev) => ({ ...prev, ...next }))
    }
  }

  const toggleOne = (uuid: string) => {
    setSelected((prev) => ({ ...prev, [uuid]: !prev[uuid] }))
  }

  const onSubmit = async () => {
    const text = goal.trim()
    const targets = online.filter((a) => selected[a.uuid]).map((a) => a.uuid)
    if (!text || targets.length === 0) return
    setSubmitting(true)
    setStatus('')
    try {
      const outcome = await sendGoal(text, targets)
      if (outcome.kind === 'ok') {
        setGoal('')
        setStatus(outcome.created.length + '개 task 생성됨.')
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

  return (
    <section className="goal-section">
      <div className="goal-head">
        <h2 className="section-title">목표 제출</h2>
        <span className="goal-hint">선택한 관리자 각각에게 목표가 전달됩니다</span>
      </div>
      <div className="goal-body">
        <div className="goal-targets">
          <label className="goal-chip-all">
            <input type="checkbox" checked={allSelected} onChange={toggleAll} />
            <span>전체 선택</span>
          </label>
          <span className="dash-divider" />
          {agents.map((a) => {
            // 대상 체크는 online 만 유효하나, offline 도 목록엔 노출하고 비활성화한다(목업 그대로).
            const isSel = a.online && !!selected[a.uuid]
            return (
              <label
                className={'goal-chip' + (isSel ? ' selected' : '') + (a.online ? '' : ' offline')}
                key={a.uuid}
              >
                <input
                  type="checkbox"
                  checked={isSel}
                  disabled={!a.online}
                  onChange={() => toggleOne(a.uuid)}
                />
                <span className="goal-chip-uuid">{a.display_name ?? a.uuid}</span>
                {!a.online ? <span className="goal-chip-offline-label">오프라인</span> : null}
              </label>
            )
          })}
        </div>
        <textarea
          className="goal-textarea"
          rows={3}
          placeholder="예: tunaround 저장소의 flaky 테스트를 찾아 원인 분석 후 수정 PR을 올려줘"
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
        />
        <div className="goal-submit-row">
          <span className="goal-summary">
            {selCount > 0
              ? selCount + '명의 관리자 선택됨 — 각각 독립 task로 생성됩니다'
              : '대상 관리자을 선택하세요'}
          </span>
          <span className="dash-spacer" />
          <button
            type="button"
            className={'goal-submit-btn' + (canSubmit && !submitting ? ' enabled' : '')}
            disabled={!canSubmit || submitting}
            onClick={onSubmit}
          >
            {selCount > 0 ? selCount + '명의 관리자에게 목표 전달' : '목표 전달'}
          </button>
        </div>
        {status ? <span className="goal-status">{status}</span> : null}
      </div>
    </section>
  )
}
