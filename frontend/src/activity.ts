// 로스터(armed+heartbeat)와 발견 후보(discover jsonl+age)를 세션 uuid로 병합해
// 활동(age) 기준으로 활성/유휴로 나누는 순수 로직. 설계 v2-41.
import type { Agent, Candidate } from './api'

// 60분. 이보다 오래 무활동이면 유휴(발견됨으로 강등).
export const IDLE_SECS = 3600

// 병합된 세션 한 행(로스터/발견 공용 표시 모델).
export type SessionRow = {
  uuid: string // 세션 id(있으면) 또는 agent uuid
  displayName: string | null
  tags: Record<string, string>
  machine: string | null
  runner: string | null
  project: string | null
  armed: boolean // 로스터에 poll 등록됨 = A2A 수신 가능
  online: boolean // heartbeat 신선(armed 세션만 의미)
  lastHeartbeat: string | null
  ageSecs: number // 활동 경과(작을수록 최근). jsonl age 또는 heartbeat 폴백.
  hasJsonlAge: boolean // age가 실제 jsonl 활동에서 온 것(원격 heartbeat 폴백과 구분)
  source: 'roster' | 'candidate' | 'both'
}

// agent가 candidate(세션)와 같은 세션인지: agent uuid==세션id 또는 agent의 session 태그==세션id.
function matchesSession(agent: Agent, sessionUuid: string): boolean {
  return agent.uuid === sessionUuid || agent.tags.session === sessionUuid
}

export type MergeResult = {
  active: SessionRow[]
  idle: SessionRow[]
  autoBossUuid: string // 자동 총감독 후보(활성+jsonl age 최소). 없으면 ''.
}

// roster + candidates를 병합해 활성/유휴로 나눈다. 총감독 자동후보(활성 중 jsonl age 최소)도 낸다.
export function mergeSessions(agents: Agent[], candidates: Candidate[], idleSecs = IDLE_SECS): MergeResult {
  const rows: SessionRow[] = []
  const usedAgents = new Set<string>()

  // 1) candidate(=jsonl 세션) 기준 행. 매칭되는 armed agent가 있으면 합친다.
  for (const c of candidates) {
    const agent = agents.find((a) => matchesSession(a, c.uuid))
    if (agent) usedAgents.add(agent.uuid)
    // 미무장 후보는 agent 태그가 없으니 candidate 필드로 태그를 합성(뱃지/세션줄 렌더 통일).
    const fallbackTags: Record<string, string> = { session: c.uuid }
    if (c.machine) fallbackTags.machine = c.machine
    if (c.runner) fallbackTags.runner = c.runner
    if (c.project) fallbackTags.project = c.project
    rows.push({
      uuid: c.uuid,
      displayName: agent?.display_name ?? null,
      tags: agent?.tags ?? fallbackTags,
      machine: c.machine,
      runner: c.runner,
      project: c.project,
      armed: !!agent,
      online: agent?.online ?? false,
      lastHeartbeat: agent?.last_heartbeat ?? null,
      ageSecs: c.age_secs,
      hasJsonlAge: true,
      source: agent ? 'both' : 'candidate',
    })
  }

  // 2) candidate와 안 맞은 roster agent(원격 = 로컬 discover 커버 밖). heartbeat로 age 폴백.
  for (const a of agents) {
    if (usedAgents.has(a.uuid)) continue
    rows.push({
      uuid: a.uuid,
      displayName: a.display_name,
      tags: a.tags,
      machine: a.tags.machine ?? null,
      runner: a.tags.runner ?? null,
      project: a.tags.project ?? null,
      armed: true,
      online: a.online,
      lastHeartbeat: a.last_heartbeat,
      ageSecs: a.online ? 0 : idleSecs + 1, // online=활성 취급, offline=유휴
      hasJsonlAge: false,
      source: 'roster',
    })
  }

  const active = rows.filter((r) => r.ageSecs < idleSecs)
  const idle = rows.filter((r) => r.ageSecs >= idleSecs)
  // age 오름차순(최근 먼저) 정렬.
  active.sort((a, b) => a.ageSecs - b.ageSecs)
  idle.sort((a, b) => a.ageSecs - b.ageSecs)

  // 총감독 자동후보 = 활성 중 실제 jsonl 활동 age 최소(사람이 지금 입력하는 로컬 세션).
  // 원격 heartbeat 폴백(hasJsonlAge=false)은 boss 자동선정에서 제외(입력 세션이 아님).
  let autoBossUuid = ''
  let best = Infinity
  for (const r of active) {
    if (r.hasJsonlAge && r.ageSecs < best) {
      best = r.ageSecs
      autoBossUuid = r.uuid
    }
  }

  return { active, idle, autoBossUuid }
}
