// presence 타임라인 패널(v2-50): 세션 등장(appear)·소멸(disappear)·사람입력(human_input) 이력을
// 최신순으로 보여준다(read-only, 관제탑 원칙). GET /dashboard/presence-timeline 를 5초 주기로 폴한다.
import { useEffect, useState } from 'react'
import type { PresenceEvent } from '../api'
import { fetchPresenceTimeline, relativeTime } from '../api'

const POLL_MS = 5000
const LIMIT = 100

// 이벤트 종류별 표시(글리프 + 라벨 + CSS 클래스). appear=녹색·disappear=회색·human_input=★.
function eventGlyph(type: PresenceEvent['event_type']): { icon: string; label: string; cls: string } {
  switch (type) {
    case 'appear':
      return { icon: '+', label: '등장', cls: 'appear' }
    case 'disappear':
      return { icon: '−', label: '소멸', cls: 'disappear' }
    case 'human_input':
      return { icon: '★', label: '사람입력', cls: 'human' }
    default:
      return { icon: '·', label: type, cls: '' }
  }
}

// 이벤트의 표시 이름(display_name 우선, 없으면 machine-runner 조합, 최후엔 uuid 축약).
function eventName(e: PresenceEvent): string {
  if (e.display_name) return e.display_name
  const parts = [e.machine, e.runner].filter(Boolean)
  if (parts.length > 0) return parts.join('-')
  return e.agent_uuid.slice(0, 8)
}

export default function PresenceTimeline() {
  const [events, setEvents] = useState<PresenceEvent[] | null>(null)
  const [ok, setOk] = useState(true)

  useEffect(() => {
    let cancelled = false
    const controller = new AbortController()
    const load = () => {
      fetchPresenceTimeline(LIMIT, controller.signal)
        .then((list) => {
          if (cancelled) return
          setEvents(list)
          setOk(true)
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          setOk(false)
          console.error('[presence-timeline] 조회 실패.', err)
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

  return (
    <section className="timeline-section">
      <div className="timeline-header">
        <h2 className="section-title">presence 타임라인</h2>
        <span className="timeline-hint">세션 등장·소멸 + 사람입력 이력(최신순)</span>
        {/* 첫 로드 성공 후의 폴 실패도 가시화한다(백엔드 fail-visible 원칙과 일관): stale 스냅샷을
            오류표식 없이 렌더하면 관전자가 최신으로 오판한다. events가 있어도 최신 폴이 실패면 경고. */}
        {!ok && events !== null ? (
          <span className="timeline-stale" title="폴 갱신 실패 - 마지막 성공 스냅샷을 표시 중">
            갱신 실패 · 마지막 성공 표시 중
          </span>
        ) : null}
      </div>
      <div className="timeline-list">
        {events === null ? (
          <div className="timeline-empty">{ok ? '조회 중…' : '조회 실패'}</div>
        ) : events.length === 0 ? (
          <div className="timeline-empty">아직 기록된 presence 이벤트가 없습니다.</div>
        ) : (
          events.map((e) => {
            const g = eventGlyph(e.event_type)
            return (
              <div className="timeline-row" key={e.id}>
                <span className={'timeline-glyph ' + g.cls} title={g.label}>
                  {g.icon}
                </span>
                <span className="timeline-name">{eventName(e)}</span>
                <span className="timeline-kind">{g.label}</span>
                {e.detail ? <span className="timeline-detail">{e.detail}</span> : null}
                <span className="timeline-rel">{relativeTime(e.at)}</span>
              </div>
            )
          })
        )}
      </div>
    </section>
  )
}
