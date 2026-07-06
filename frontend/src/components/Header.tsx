// 상단 헤더: 로고 + 타이틀 + 연결 배지들 + 라이브 시계. 목업 <header> 이식.
import { useEffect, useState } from 'react'

type Props = {
  brokerOk: boolean
  sseOpen: boolean
  remoteViewer: boolean
}

// "오전/오후 h:mm:ss" 형식의 라이브 시계 문자열을 만든다(목업 clock 계산 그대로).
function formatClock(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  const h24 = d.getHours()
  const ampm = h24 < 12 ? '오전' : '오후'
  const h12 = h24 % 12 === 0 ? 12 : h24 % 12
  return ampm + ' ' + h12 + ':' + pad(d.getMinutes()) + ':' + pad(d.getSeconds())
}

export default function Header({ brokerOk, sseOpen, remoteViewer }: Props) {
  const [now, setNow] = useState(() => new Date())

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 1000)
    return () => window.clearInterval(timer)
  }, [])

  return (
    <header className="dash-header">
      <div className="dash-header-inner">
        <span className="dash-logo">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <circle cx="8" cy="8" r="6.4" stroke="currentColor" strokeWidth="1.4" />
            <circle cx="8" cy="8" r="3" stroke="currentColor" strokeWidth="1.4" opacity="0.55" />
            <circle cx="8" cy="8" r="1.2" fill="currentColor" />
          </svg>
        </span>
        <h1 className="dash-title">총감독 대시보드</h1>
        <span className="dash-divider" />
        <span className="dash-badge">
          <span className={'dash-badge-dot' + (brokerOk ? '' : ' off')} />
          {brokerOk ? '브로커 연결됨' : '브로커 연결 끊김'}
        </span>
        <span className="dash-badge">
          <span className={'dash-badge-dot' + (sseOpen ? ' blink' : ' off')} />
          {sseOpen ? '피드 SSE 수신 중' : '피드 SSE 끊김'}
        </span>
        {remoteViewer ? <span className="dash-badge-warn">읽기 전용 관전</span> : null}
        <span className="dash-spacer" />
        <span className="dash-clock">{formatClock(now)}</span>
      </div>
    </header>
  )
}
