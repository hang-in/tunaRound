// 로스터 = heartbeat(online) 세션 전부. 총감독 = 사람 입력 최신. 순수 presence 모델(설계 v2-43).
// v2-41 활동(jsonl age)·discover 병합은 제거됨: 전부 autoarm heartbeat라 discover 불필요.
import type { Agent } from './api'

// 로스터 한 행(online armed 세션). 표시용 파생.
export type SessionRow = {
  uuid: string
  displayName: string | null
  tags: Record<string, string>
  machine: string | null
  runner: string | null
  project: string | null
  lastHeartbeat: string
  humanInputAt: string | null // 마지막 사람 프롬프트 시각(총감독 판정)
  busy: boolean // 지금 일하는 중(working task 대상, v2-54). 로스터가 스피너로 표시.
  label: string // 표시 이름(displayName 또는 machine-runner-project). 충돌 시 -B/-C.
}

// 같은 base가 여럿이면 uuid 정렬 순 -B/-C(첫 개 무접미).
function assignLabels(rows: SessionRow[]): void {
  const base = (r: SessionRow) =>
    r.displayName || [r.machine, r.runner, r.project].filter(Boolean).join('-') || r.uuid
  const groups = new Map<string, SessionRow[]>()
  for (const r of rows) {
    const b = base(r)
    if (!groups.has(b)) groups.set(b, [])
    groups.get(b)!.push(r)
  }
  for (const [b, group] of groups) {
    if (group.length === 1) {
      group[0].label = b
      continue
    }
    group.sort((a, b2) => a.uuid.localeCompare(b2.uuid))
    group.forEach((r, i) => {
      r.label = i === 0 ? b : `${b}-${String.fromCharCode(66 + i - 1)}` // B, C, D...
    })
  }
}

export type RosterView = {
  rows: SessionRow[] // 세션 카드 = role=session(+미지정)
  workers: SessionRow[] // 헤드리스 워커 = role=worker(별 섹션, 설계 v2-43 §5-4)
  infra: SessionRow[] // 머신 상주 데몬 = role=infra(카드 없음, 머신 헤더 도트로. 설계 v2-44 §5)
  autoBossUuid: string // 총감독 = online 중 human_input_at 최신. 없으면 ''.
}

// online(heartbeat 신선) 세션만 로스터에. 총감독 = human_input_at 최신(SQL datetime=사전순).
// role=worker(헤드리스)는 관리자 로스터가 아니라 워커 섹션으로 분리한다.
export function buildRoster(agents: Agent[]): RosterView {
  const all: SessionRow[] = agents
    .filter((a) => a.online)
    .map((a) => ({
      uuid: a.uuid,
      displayName: a.display_name,
      tags: a.tags ?? {},
      machine: a.tags?.machine ?? null,
      runner: a.tags?.runner ?? null,
      project: a.tags?.project ?? null,
      lastHeartbeat: a.last_heartbeat,
      humanInputAt: a.human_input_at ?? null,
      busy: a.busy ?? false,
      label: '',
    }))
  assignLabels(all) // 라벨 -B/-C 증분은 관리자·워커 통틀어 유일해야 한다.
  const rows = all.filter((r) => r.tags.role !== 'worker' && r.tags.role !== 'infra')
  const workers = all.filter((r) => r.tags.role === 'worker')
  const infra = all.filter((r) => r.tags.role === 'infra')

  let autoBossUuid = ''
  let best = ''
  for (const r of rows) {
    // 총감독은 사람 자리라 관리자 세션에서만 찾는다(워커는 헤드리스 = 사람 입력 없음).
    if (r.humanInputAt && r.humanInputAt > best) {
      best = r.humanInputAt
      autoBossUuid = r.uuid
    }
  }
  return { rows, workers, infra, autoBossUuid }
}
