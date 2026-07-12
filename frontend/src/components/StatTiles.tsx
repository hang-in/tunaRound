// 요약 통계 4타일(온라인 관리자/진행중/완료/실패). 목업 .tiles 이식. 진행중/완료/실패는 서버소스
// (health.task_counts = tasks 테이블 라이브 집계)라 피드 리로드에 흔들리지 않는다(서버 집계 뱃지 표기).
import type { BrokerHealth } from '../api'

type Props = {
  // roster online 관리자 수(클라 파생 - 유일하게 서버 집계 아님).
  onlineCount: number
  // 서버 헬스(진행중/완료/실패). null이면(조회 전·실패) 대시로 표시.
  health: BrokerHealth | null
}

// 서버 카운트가 아직 없을 때(health null) 대시로 표시(0 위장 금지).
function n(v: number | undefined): string {
  return v === undefined ? '–' : String(v)
}

export default function StatTiles({ onlineCount, health }: Props) {
  const tc = health?.task_counts
  return (
    <section className="tiles">
      <div className="tile">
        <div className="n">{onlineCount}</div>
        <div className="k label">온라인 관리자</div>
      </div>
      <div className="tile srv good">
        <div className="n">{n(tc?.working)}</div>
        <div className="k label">진행 중</div>
      </div>
      <div className="tile srv">
        <div className="n">{n(tc?.completed)}</div>
        <div className="k label">완료</div>
      </div>
      <div className="tile srv err">
        <div className="n">{n(tc?.failed)}</div>
        <div className="k label">실패</div>
      </div>
    </section>
  )
}
