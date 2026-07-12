// 요약 통계 4타일(온라인 관리자/진행중/완료/실패). 목업 .tiles 이식. 진행중/완료/실패는 서버소스
// (health.task_counts = tasks 테이블 라이브 집계)라 피드 리로드에 흔들리지 않는다(서버 집계 뱃지 표기).
import type { BrokerHealth } from '../api'

type Props = {
  // roster online 관리자 수(클라 파생 - 유일하게 서버 집계 아님).
  onlineCount: number
  // 서버 헬스(진행중/완료/실패). null이면(조회 전·실패) 대시로 표시.
  health: BrokerHealth | null
  // 최신 health 폴이 성공했는지. false면 서버소스 3타일은 last-good을 stale로 표시(Footer와 일관, fail-visible).
  ok: boolean
}

// 서버 카운트가 아직 없을 때(health null) 대시로 표시(0 위장 금지).
function n(v: number | undefined): string {
  return v === undefined ? '–' : String(v)
}

export default function StatTiles({ onlineCount, health, ok }: Props) {
  const tc = health?.task_counts
  // health가 있는데 최신 폴이 실패면 서버소스 타일은 옛 값 = stale(디밍 + "갱신 실패" 마커, Footer와 일관).
  // health가 아직 null이면 stale 아님(그냥 로딩, 값은 대시).
  const stale = health !== null && !ok
  const srv = 'tile srv' + (stale ? ' stale' : '')
  return (
    <section className="tiles">
      <div className="tile">
        <div className="n">{onlineCount}</div>
        <div className="k label">온라인 관리자</div>
      </div>
      <div className={srv + ' good'}>
        <div className="n">{n(tc?.working)}</div>
        <div className="k label">진행 중</div>
      </div>
      <div className={srv}>
        <div className="n">{n(tc?.completed)}</div>
        <div className="k label">완료</div>
      </div>
      <div className={srv + ' err'}>
        <div className="n">{n(tc?.failed)}</div>
        <div className="k label">실패</div>
      </div>
    </section>
  )
}
