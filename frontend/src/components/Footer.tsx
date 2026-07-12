// 고정 하단 푸터: 브로커 헬스(좌) / 머신별 스캐너 도달성(우). 목업 footer.dash 이식(타이틀 제거).
// health 데이터는 App이 5초 폴로 소유해 내려준다(read-only, 관제탑 원칙). 스캐너 닷=브레싱 하트비트.
import type { BrokerHealth } from '../api'
import { relativeTime } from '../api'

type Props = {
  health: BrokerHealth | null
  // 최신 폴이 성공했는지(실패 시 stale 스냅샷을 표시 중임을 알린다).
  ok: boolean
}

// 경과 초를 짧은 사람 표기로(스캐너 heartbeat 나이).
function ageLabel(secs: number): string {
  if (secs < 0) return '?'
  if (secs < 60) return secs + 's'
  const min = Math.floor(secs / 60)
  if (min < 60) return min + 'm'
  return Math.floor(min / 60) + 'h'
}

// 브로커 기동 후 경과 초를 초/분/시간/일 표기로.
function uptimeLabel(secs: number): { v: string; u: string } {
  if (secs < 0) return { v: '?', u: '' }
  if (secs < 60) return { v: String(secs), u: '초' }
  const min = Math.floor(secs / 60)
  if (min < 60) return { v: String(min), u: '분' }
  const hr = Math.floor(min / 60)
  if (hr < 24) return { v: String(hr), u: '시간' }
  return { v: String(Math.floor(hr / 24)), u: '일' }
}

// 바이트를 값/단위로 분리(WAL 사이드카 크기).
function byteLabel(bytes: number): { v: string; u: string } {
  if (bytes < 1024) return { v: String(bytes), u: 'B' }
  const kb = bytes / 1024
  if (kb < 1024) return { v: kb < 10 ? kb.toFixed(1) : String(Math.round(kb)), u: 'KB' }
  const mb = kb / 1024
  return { v: mb < 10 ? mb.toFixed(1) : String(Math.round(mb)), u: 'MB' }
}

export default function Footer({ health, ok }: Props) {
  if (!health) {
    return (
      <footer className="dash">
        <span className="footer-empty">{ok ? '헬스 조회 중…' : '헬스 조회 실패'}</span>
      </footer>
    )
  }

  const up = uptimeLabel(health.uptime_secs)
  const wal = byteLabel(health.wal_bytes)

  return (
    <footer className="dash">
      <div className="health">
        {!ok ? <span className="footer-empty" title="폴 갱신 실패 - 마지막 성공 스냅샷">갱신 실패</span> : null}
        <div className="m">
          <span className="label">열린</span>
          <span className="v">{health.open_tasks}</span>
        </div>
        <div className={'m' + (health.no_consumer > 0 ? ' warn' : '')}>
          <span className="label">미배달</span>
          <span className="v">{health.no_consumer}</span>
        </div>
        <div className={'m' + (health.stuck > 0 ? ' err' : '')}>
          <span className="label">고착</span>
          <span className="v">{health.stuck}</span>
        </div>
        <div className="m">
          <span className="label">가동</span>
          <span className="v">{up.v}</span>
          {up.u ? <span className="u">{up.u}</span> : null}
        </div>
        <div className="m">
          <span className="label">WAL</span>
          <span className="v">{wal.v}</span>
          <span className="u">{wal.u}</span>
        </div>
      </div>
      <div className="scanners">
        {health.scanners.length === 0 ? (
          <span className="footer-empty">스캐너 없음</span>
        ) : (
          health.scanners.map((s) => (
            <span
              className="s"
              key={s.machine}
              title={s.online ? '도달 가능' : '도달 불가(마지막 ' + relativeTime(s.last_heartbeat) + ')'}
            >
              <span className={'dot' + (s.online ? '' : ' off')} />
              {s.machine}
              <span className="mono">{ageLabel(s.age_secs)}</span>
            </span>
          ))
        )}
      </div>
    </footer>
  )
}
