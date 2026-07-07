// 로스터(armed+heartbeat)와 발견 후보(discover jsonl+age)를 세션 uuid로 병합해
// 활동(age) 기준으로 활성/유휴로 나누는 순수 로직. 설계 v2-41.
import type { Agent, Candidate } from './api'

// 60분. 이보다 오래 무활동이면 유휴(발견됨으로 강등).
export const IDLE_SECS = 3600

// heartbeat=presence(설계 v2-42): armed 세션은 heartbeat가 presence라 항상 표시.
// 미무장(armed 없는) discover 세션은 jsonl mtime이 이 창 이내일 때만 표시한다. 그 이상 오래된 jsonl은
// 닫힌 세션의 잔존(유령)이라 숨긴다(그 세션에서 타이핑하면 ping 훅이 자동 무장 → armed로 로스터 복귀).
export const FRESH_UNARMED_SECS = 600

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
  humanInputAt: string | null // 마지막 사람 프롬프트 시각(총감독 판정, 설계 v2-42)
  source: 'roster' | 'candidate' | 'both'
  label: string // 표시 이름(displayName 또는 machine-runner-project). 같은 이름 충돌 시 -B/-C 증분.
}

// 각 행의 표시 이름을 정하고, 같은 base가 여럿이면 uuid 정렬 순으로 -B/-C를 붙인다(첫 개는 무접미).
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

// agent가 candidate(세션)와 같은 세션인지: agent uuid==세션id 또는 agent의 session 태그==세션id.
function matchesSession(agent: Agent, sessionUuid: string): boolean {
  return agent.uuid === sessionUuid || agent.tags?.session === sessionUuid
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
    // heartbeat=presence: 미무장(armed 없는) 후보는 최근(FRESH_UNARMED) jsonl만 표시. 오래된 것은
    // 닫힌 세션 잔존(유령)이라 제외 - 같은 세션 옛 jsonl이 -B/-C로 중복되던 것 소멸.
    if (!agent && c.age_secs >= FRESH_UNARMED_SECS) continue
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
      humanInputAt: agent?.human_input_at ?? null,
      source: agent ? 'both' : 'candidate',
      label: '',
    })
  }

  // 2) candidate와 안 맞은 roster agent(원격 = 로컬 discover 커버 밖). heartbeat로 age 폴백.
  // offline(heartbeat 만료) + candidate 없음 = poll watcher 죽음 = 세션 종료/크래시 → 죽은 좀비라 제외
  // (idle이 아니다). candidate가 매칭된 경우는 위 1)에서 age로 배치되므로 여기서 안 걸린다.
  for (const a of agents) {
    if (usedAgents.has(a.uuid)) continue
    if (!a.online) continue
    rows.push({
      uuid: a.uuid,
      displayName: a.display_name,
      tags: a.tags ?? {},
      machine: a.tags?.machine ?? null,
      runner: a.tags?.runner ?? null,
      project: a.tags?.project ?? null,
      armed: true,
      online: true, // 위에서 offline은 continue로 걸렀다
      lastHeartbeat: a.last_heartbeat,
      ageSecs: 0, // heartbeat 신선 = 활성 취급(jsonl age는 discover 커버 밖이라 폴백)
      hasJsonlAge: false,
      humanInputAt: a.human_input_at ?? null,
      source: 'roster',
      label: '',
    })
  }

  assignLabels(rows)

  const active = rows.filter((r) => r.ageSecs < idleSecs)
  const idle = rows.filter((r) => r.ageSecs >= idleSecs)
  // age 오름차순(최근 먼저) 정렬.
  active.sort((a, b) => a.ageSecs - b.ageSecs)
  idle.sort((a, b) => a.ageSecs - b.ageSecs)

  // 총감독 자동후보 = 사람이 마지막으로 프롬프트를 넣은 세션(human_input_at 최신, 설계 v2-42).
  // jsonl mtime(resume/tool로 튐) 대신 사람 입력만 신호로 쓴다. 아무도 핑 없으면 총감독 없음('').
  // human_input_at은 SQL datetime 문자열이라 사전순 비교=시간순.
  let autoBossUuid = ''
  let bestInput = ''
  for (const r of active) {
    if (r.humanInputAt && r.humanInputAt > bestInput) {
      bestInput = r.humanInputAt
      autoBossUuid = r.uuid
    }
  }

  return { active, idle, autoBossUuid }
}
