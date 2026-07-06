// 헤더 밑 요약 통계 4타일(온라인 관리자/진행중/완료/실패). 목업 "요약 통계" 섹션 이식.

type Props = {
  onlineCount: number
  totalSups: number
  workingCount: number
  completedCount: number
  failedCount: number
}

export default function StatTiles({
  onlineCount,
  totalSups,
  workingCount,
  completedCount,
  failedCount,
}: Props) {
  return (
    <section className="stat-grid">
      <div className="stat-tile">
        <span className="stat-label">
          <span className="stat-label-dot" style={{ background: 'var(--ok)' }} />
          온라인 관리자
        </span>
        <span className="stat-value-row">
          <span className="stat-value">{onlineCount}</span>
          <span className="stat-total">/ {totalSups}</span>
        </span>
      </div>
      <div className="stat-tile">
        <span className="stat-label">
          <span className="stat-label-dot" style={{ background: 'var(--warn)' }} />
          진행중 task
        </span>
        <span className="stat-value">{workingCount}</span>
      </div>
      <div className="stat-tile">
        <span className="stat-label">
          <span className="stat-label-dot" style={{ background: 'var(--info)' }} />
          완료 task
        </span>
        <span className="stat-value">{completedCount}</span>
      </div>
      <div className="stat-tile">
        <span className="stat-label">
          <span className="stat-label-dot" style={{ background: 'var(--err)' }} />
          실패
        </span>
        <span className="stat-value">{failedCount}</span>
      </div>
    </section>
  )
}
