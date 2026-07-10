// 워커 섹션: role=worker 헤드리스 중 작업 중(진행 task 보유)인 것만 별 섹션 표시(설계 v2-43 §5-4).
// 관리자 로스터와 같은 패널+행 레이아웃. 작업 중인 워커가 없으면 섹션 자체를 렌더하지 않는다.
import { relativeTime } from '../api'
import type { Task } from '../api'
import type { SessionRow } from '../activity'
import { MachineGlyph, TagPill } from './Roster'
import { TAG_ORDER, orderedTags } from './tags'

type Props = {
  // online role=worker 세션 전부(buildRoster가 분리).
  workers: SessionRow[]
  // uuid -> 진행 중(submitted/working) task. App이 SSE 피드 최신 상태에서 도출.
  activeByAgent: Record<string, Task>
}

export default function WorkerSection({ workers, activeByAgent }: Props) {
  const working = workers.filter((w) => activeByAgent[w.uuid])
  if (working.length === 0) return null

  return (
    <section className="roster-section">
      <div className="panel-header">
        <h2 className="section-title">워커 (작업 중)</h2>
        <span className="section-count">{working.length} working</span>
      </div>
      <div className="roster-list">
        {working.map((w) => {
          const task = activeByAgent[w.uuid]
          return (
            <div className="roster-row" key={w.uuid}>
              <div className="card-row">
                <span className="status-dot-wrap">
                  <span className="status-dot online" />
                </span>
                <MachineGlyph machine={w.machine ?? undefined} />
                <span className="roster-uuid">{w.label || w.uuid}</span>
                <span className="pill-working" title={`task ${task.id} (${task.state})`}>
                  작업 중
                </span>
                <span className="dash-spacer" />
                <span className="hb-label-group">
                  <span className="hb-label">
                    task {task.id.slice(0, 8)} · {relativeTime(w.lastHeartbeat)}
                  </span>
                </span>
              </div>
              <div className="tag-row">
                {orderedTags(w.tags)
                  .filter(([k]) => TAG_ORDER.includes(k))
                  .map(([k, v]) => (
                    <TagPill key={k} k={k} v={v} />
                  ))}
              </div>
            </div>
          )
        })}
      </div>
    </section>
  )
}
