// mesh 건강 요약 패널: 열린 task 수 + 미배달(no-consumer)/고착(stuck) 경보 + 머신별 스캐너 도달성.
// GET /dashboard/health 를 5초 주기로 폴한다(read-only, 관제탑 원칙). 서버가 tasks() 와 같은 임계로 집계.
import { useEffect, useState } from 'react'
import type { BrokerHealth } from '../api'
import { fetchHealth, relativeTime } from '../api'

const POLL_MS = 5000

// 경과 초를 짧은 사람 표기로(스캐너 heartbeat 나이).
function ageLabel(secs: number): string {
  if (secs < 0) return '?'
  if (secs < 60) return secs + '초'
  const min = Math.floor(secs / 60)
  if (min < 60) return min + '분'
  return Math.floor(min / 60) + '시간'
}

// 브로커 기동 후 경과 초를 초/분/시간/일 표기로(uptime, 스캐너 나이보다 길어질 수 있어 일 단위 추가).
function uptimeLabel(secs: number): string {
  if (secs < 0) return '?'
  if (secs < 60) return secs + '초'
  const min = Math.floor(secs / 60)
  if (min < 60) return min + '분'
  const hr = Math.floor(min / 60)
  if (hr < 24) return hr + '시간'
  return Math.floor(hr / 24) + '일'
}

// 바이트를 B/KB/MB 짧은 표기로(WAL 사이드카 크기).
function byteLabel(bytes: number): string {
  if (bytes < 1024) return bytes + ' B'
  const kb = bytes / 1024
  if (kb < 1024) return (kb < 10 ? kb.toFixed(1) : Math.round(kb).toString()) + ' KB'
  const mb = kb / 1024
  return (mb < 10 ? mb.toFixed(1) : Math.round(mb).toString()) + ' MB'
}

export default function HealthPanel() {
  const [health, setHealth] = useState<BrokerHealth | null>(null)
  const [ok, setOk] = useState(true)

  useEffect(() => {
    let cancelled = false
    const controller = new AbortController()
    const load = () => {
      fetchHealth(controller.signal)
        .then((h) => {
          if (cancelled) return
          setHealth(h)
          setOk(true)
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          setOk(false)
          console.error('[health] 조회 실패.', err)
        })
    }
    load()
    const timer = window.setInterval(load, POLL_MS)
    return () => {
      cancelled = true
      controller.abort()
      window.clearInterval(timer)
    }
  }, [])

  if (!health) {
    return (
      <section className="health-panel">
        <span className="health-title">브로커 헬스</span>
        <span className="health-empty">{ok ? '조회 중…' : '조회 실패'}</span>
      </section>
    )
  }

  return (
    <section className="health-panel">
      <span className="health-title">브로커 헬스</span>
      <div className="health-metrics">
        <span className="health-metric">
          <span className="health-metric-label">열린 task</span>
          <span className="health-metric-value">{health.open_tasks}</span>
        </span>
        <span className={'health-metric' + (health.no_consumer > 0 ? ' warn' : '')}>
          <span className="health-metric-label">미배달</span>
          <span className="health-metric-value">{health.no_consumer}</span>
        </span>
        <span className={'health-metric' + (health.stuck > 0 ? ' err' : '')}>
          <span className="health-metric-label">고착</span>
          <span className="health-metric-value">{health.stuck}</span>
        </span>
        <span className="health-metric">
          <span className="health-metric-label">가동</span>
          <span className="health-metric-value">{uptimeLabel(health.uptime_secs)}</span>
        </span>
        <span className="health-metric">
          <span className="health-metric-label">WAL</span>
          <span className="health-metric-value">{byteLabel(health.wal_bytes)}</span>
        </span>
      </div>
      <span className="health-divider" />
      <div className="health-scanners">
        <span className="health-metric-label">스캐너</span>
        {health.scanners.length === 0 ? (
          <span className="health-empty">없음</span>
        ) : (
          health.scanners.map((s) => (
            <span
              className="health-scanner"
              key={s.machine}
              title={s.online ? '도달 가능' : '도달 불가(마지막 ' + relativeTime(s.last_heartbeat) + ')'}
            >
              <span className={'dash-badge-dot' + (s.online ? '' : ' off')} />
              {s.machine}
              <span className="health-scanner-age">{ageLabel(s.age_secs)}</span>
            </span>
          ))
        )}
      </div>
    </section>
  )
}
