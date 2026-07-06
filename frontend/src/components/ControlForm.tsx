// codex 직접 제어 폼(v2-40 S4): codex app-server 세션(ws)에 turn/start를 직접 주입한다.
// 로컬(loopback) 전용 - 원격 관전은 안내만. goal 폼과 동일 신뢰 경계(무인증 loopback write).
import { useState } from 'react'
import { sendControl } from '../api'

type Props = {
  remoteViewer: boolean
}

const DEFAULT_WS = 'ws://127.0.0.1:8790'

export default function ControlForm({ remoteViewer }: Props) {
  const [ws, setWs] = useState(DEFAULT_WS)
  const [text, setText] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [status, setStatus] = useState('')
  const [answer, setAnswer] = useState('')

  if (remoteViewer) {
    return (
      <section className="goal-section">
        <div className="goal-head">
          <h2 className="section-title">codex 직접 제어</h2>
          <span className="goal-hint">app-server 세션에 turn/start를 직접 주입합니다</span>
        </div>
        <div className="goal-warning">원격 관전 모드입니다 — codex 제어는 로컬(총괄) 세션에서만 가능합니다.</div>
      </section>
    )
  }

  const canSubmit = ws.trim().length > 0 && text.trim().length > 0

  const onSubmit = async () => {
    if (!canSubmit) return
    setSubmitting(true)
    setStatus('주입 중… (codex 응답까지 대기)')
    setAnswer('')
    try {
      const outcome = await sendControl(ws.trim(), text.trim())
      if (outcome.kind === 'ok') {
        setStatus('완료 — codex 응답 수신.')
        setAnswer(outcome.answer)
        setText('')
      } else if (outcome.kind === 'forbidden') {
        setStatus('원격 세션에서는 제어할 수 없습니다(403).')
      } else {
        setStatus(outcome.message)
      }
    } catch (err) {
      console.error('[control] 제어 실패.', err)
      setStatus('제어 실패: 네트워크 또는 서버 오류.')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <section className="goal-section">
      <div className="goal-head">
        <h2 className="section-title">codex 직접 제어</h2>
        <span className="goal-hint">app-server 세션(ws)에 turn/start를 직접 주입합니다</span>
      </div>
      <div className="goal-body">
        <input
          className="control-ws-input"
          type="text"
          aria-label="codex app-server ws 주소"
          placeholder="ws://127.0.0.1:8790"
          value={ws}
          onChange={(e) => setWs(e.target.value)}
        />
        <textarea
          className="goal-textarea"
          rows={3}
          aria-label="codex에 주입할 지시 텍스트"
          placeholder="예: 브로커 로스터를 list_agents로 조회해서 online 관리자 수를 알려줘"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <div className="goal-submit-row">
          <span className="goal-summary">loopback codex app-server에 직접 주입(무인증 로컬)</span>
          <span className="dash-spacer" />
          <button
            type="button"
            className={'goal-submit-btn' + (canSubmit && !submitting ? ' enabled' : '')}
            disabled={!canSubmit || submitting}
            onClick={onSubmit}
          >
            {submitting ? '주입 중…' : 'turn/start 주입'}
          </button>
        </div>
        {status ? (
          <span className="goal-status" role="status" aria-live="polite">
            {status}
          </span>
        ) : null}
        {answer ? <pre className="control-answer">{answer}</pre> : null}
      </div>
    </section>
  )
}
