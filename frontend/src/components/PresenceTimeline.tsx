// presence 타임라인(v2-50): 세션 등장(appear)·소멸(disappear)·사람입력(human_input)을 터미널 로그
// 라인 스타일로 최신순 표시(목업 .log/.logline). GET /dashboard/presence-timeline 5초 폴(read-only). 폴 로직 보존.
import { useEffect, useState } from 'react'
import type { PresenceEvent } from '../api'
import { fetchPresenceTimeline, relativeTime } from '../api'

const POLL_MS = 5000
const LIMIT = 100

// 이벤트 종류별 표시(글리프+라벨+CSS 클래스). appear=녹색·disappear=회색·human_input=★금색.
function eventGlyph(type: PresenceEvent['event_type']): { label: string; cls: string } {
  switch (type) {
    case 'appear':
      return { label: '+ 등장', cls: 'appear' }
    case 'disappear':
      return { label: '− 소멸', cls: 'disappear' }
    case 'human_input':
      return { label: '★ 입력', cls: 'human' }
    default:
      return { label: type, cls: '' }
  }
}

// 이벤트 표시 이름(display_name 우선, 없으면 machine-runner, 최후 uuid 축약).
function eventName(e: PresenceEvent): string {
  if (e.display_name) return e.display_name
  const parts = [e.machine, e.runner].filter(Boolean)
  if (parts.length > 0) return parts.join('-')
  return e.agent_uuid.slice(0, 8)
}

// SQL UTC("YYYY-MM-DD HH:MM:SS")를 로컬 시계 HH:MM:SS로. 파싱 실패 시 원문 뒷부분.
function clockOf(sqlUtc: string): string {
  const t = Date.parse(sqlUtc.replace(' ', 'T') + 'Z')
  if (Number.isNaN(t)) return sqlUtc.slice(11, 19)
  const d = new Date(t)
  const pad = (n: number) => String(n).padStart(2, '0')
  return pad(d.getHours()) + ':' + pad(d.getMinutes()) + ':' + pad(d.getSeconds())
}

export default function PresenceTimeline() {
  const [events, setEvents] = useState<PresenceEvent[] | null>(null)
  const [ok, setOk] = useState(true)

  useEffect(() => {
    let cancelled = false
    // in-flight 폴 추적: 매 폴마다 직전 요청 abort → 늦게 온 옛 응답이 최신을 덮는 레이스 방지(gemini medium).
    let inflight: AbortController | null = null
    const load = () => {
      inflight?.abort()
      const controller = new AbortController()
      inflight = controller
      fetchPresenceTimeline(LIMIT, controller.signal)
        .then((list) => {
          if (cancelled || controller.signal.aborted) return
          setEvents(list)
          setOk(true)
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          if (cancelled) return
          setOk(false)
          console.error('[presence-timeline] 조회 실패.', err)
        })
    }
    load()
    const timer = window.setInterval(load, POLL_MS)
    return () => {
      cancelled = true
      inflight?.abort()
      window.clearInterval(timer)
    }
  }, [])

  return (
    <section className="card timeline">
      <div className="head">
        <h2>Presence 타임라인</h2>
        {!ok && events !== null ? (
          <span className="stale" title="폴 갱신 실패 - 마지막 성공 스냅샷을 표시 중">
            갱신 실패 · 마지막 성공 표시 중
          </span>
        ) : (
          <span className="count">등장·소멸·사람 입력</span>
        )}
      </div>
      <div className="log">
        {events === null ? (
          <div className="log-empty">{ok ? '조회 중…' : '조회 실패'}</div>
        ) : events.length === 0 ? (
          <div className="log-empty">아직 기록된 presence 이벤트가 없습니다.</div>
        ) : (
          events.map((e) => {
            const g = eventGlyph(e.event_type)
            return (
              <div className="logline" key={e.id}>
                <span className="ts">{clockOf(e.at)}</span>
                <span className={'ev ' + g.cls}>{g.label}</span>
                <span className="who">{eventName(e)}</span>
                <span className="tail">
                  {e.detail ? e.detail + ' · ' : ''}
                  {relativeTime(e.at)}
                </span>
              </div>
            )
          })
        )}
      </div>
    </section>
  )
}
