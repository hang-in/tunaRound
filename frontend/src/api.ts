// 대시보드 브로커 API 타입과 호출 헬퍼(roster 폴, SSE 구독, goal 제출)를 모은 모듈.

// GET /dashboard/roster 응답 요소(등록된 관리자 전체 + online 플래그를 서버가 계산해 내려준다).
export type Agent = {
  uuid: string
  tags: Record<string, string>
  display_name: string | null
  last_heartbeat: string
  online: boolean
  // 마지막 사람 프롬프트 시각(UserPromptSubmit 핑). 총감독=이 값 최신 세션(설계 v2-42). null=핑 없음.
  human_input_at: string | null
}

// SQLite datetime('now')는 "YYYY-MM-DD HH:MM:SS" UTC 문자열이다. 사람이 읽는 상대시간으로 바꾼다
// (대시보드가 UTC를 그대로 찍어 한국시간과 어긋나 보이던 문제 해소).
export function relativeTime(sqlUtc: string): string {
  const t = Date.parse(sqlUtc.replace(' ', 'T') + 'Z')
  if (Number.isNaN(t)) return sqlUtc
  const sec = Math.max(0, Math.round((Date.now() - t) / 1000))
  if (sec < 5) return '방금'
  if (sec < 60) return sec + '초 전'
  const min = Math.floor(sec / 60)
  if (min < 60) return min + '분 전'
  const hr = Math.floor(min / 60)
  if (hr < 24) return hr + '시간 전'
  return Math.floor(hr / 24) + '일 전'
}

// task 아티팩트의 한 조각.
export type Part = { text?: string }

// task 산출물.
export type Artifact = {
  artifactId?: string
  name?: string | null
  parts: Part[]
}

// A2A 메시지(요청 원문·상태 메시지). parts[].text 에 사람이 읽는 텍스트가 담긴다.
// history[0]=접수 당시 요청, state=failed 일 때 statusMessage=실패 사유.
export type Message = { role?: string; parts?: Part[] }

// 브로커 task 스냅샷.
export type Task = {
  id: string
  contextId?: string
  fromAgent: string
  toAgent: string
  state: string
  statusMessage?: Message
  artifacts: Artifact[]
  history?: Message[]
  runner?: string
  createdAt: string
  updatedAt: string
}

// SSE 이벤트 메시지(event.data 를 파싱한 형태).
export type TaskEventMsg = {
  event: 'status' | 'completed'
  task: Task
}

// online 관리자 목록을 가져온다. 실패는 던져서 호출부가 콘솔 로깅만 하도록 한다.
export async function fetchRoster(signal?: AbortSignal): Promise<Agent[]> {
  const res = await fetch('/dashboard/roster', { signal })
  if (!res.ok) {
    throw new Error('roster 조회 실패: ' + res.status)
  }
  return (await res.json()) as Agent[]
}

// 머신별 presence 스캐너 도달성(GET /dashboard/health 의 요소).
export type ScannerHealth = {
  machine: string
  last_heartbeat: string
  age_secs: number
  online: boolean
}

// tasks 테이블 상태별 라이브 카운트(StatTiles 서버소스, v2-53). working=진행 중(open)=
// submitted+working+input_required. 피드에서 세지 않아 리로드에도 안정.
export type TaskCounts = {
  working: number
  completed: number
  failed: number
}

// 브로커 헬스 요약(GET /dashboard/health). 브로커 버전 + 열린 task 수 + 미배달/고착 집계
// + 상태별 카운트(task_counts) + 스캐너 도달성 + 브로커 uptime(기동 후 경과 초) + WAL 사이드카 크기(바이트).
// uptime/WAL은 임계 없는 게이지.
export type BrokerHealth = {
  version: string
  open_tasks: number
  no_consumer: number
  stuck: number
  task_counts: TaskCounts
  scanners: ScannerHealth[]
  now: string
  uptime_secs: number
  wal_bytes: number
}

// mesh 건강 요약을 가져온다. 실패는 던져서 호출부가 콘솔 로깅만 하도록 한다.
export async function fetchHealth(signal?: AbortSignal): Promise<BrokerHealth> {
  const res = await fetch('/dashboard/health', { signal })
  if (!res.ok) {
    throw new Error('health 조회 실패: ' + res.status)
  }
  return (await res.json()) as BrokerHealth
}

// 위임 이력 검색 결과 한 건(P6a 색인: speaker=`a2a/<agent>`, content=요청/결과 원문).
export type SearchResult = { speaker: string; content: string }
export type SearchResponse = { query: string; results: SearchResult[] }

// 위임 이력을 검색한다(GET /dashboard/search?q=). 실패는 던져서 호출부가 상태 표시하도록 한다.
export async function searchHistory(q: string, signal?: AbortSignal): Promise<SearchResponse> {
  const res = await fetch('/dashboard/search?q=' + encodeURIComponent(q), { signal })
  if (!res.ok) {
    throw new Error('검색 실패: ' + res.status)
  }
  return (await res.json()) as SearchResponse
}


// presence 타임라인 이벤트 한 건(GET /dashboard/presence-timeline, v2-50). 세션 등장·소멸·사람입력의
// raw edge. event_type = 'appear' | 'disappear' | 'human_input'. detail = disappear 사유(stale|deregister) 등.
export type PresenceEvent = {
  id: number
  at: string
  event_type: 'appear' | 'disappear' | 'human_input'
  agent_uuid: string
  machine: string | null
  runner: string | null
  project: string | null
  display_name: string | null
  detail: string | null
}

// presence 타임라인(최신순)을 가져온다. 실패는 던져서 호출부가 상태 표시하도록 한다.
export async function fetchPresenceTimeline(
  limit = 100,
  signal?: AbortSignal,
): Promise<PresenceEvent[]> {
  const res = await fetch('/dashboard/presence-timeline?limit=' + limit, { signal })
  if (!res.ok) {
    throw new Error('presence-timeline 조회 실패: ' + res.status)
  }
  return (await res.json()) as PresenceEvent[]
}


// POST /dashboard/goal 성공 응답: 대상별로 생성된 task 를 알려준다.
export type GoalCreated = { taskId: string; toAgent: string }
type GoalResponse = { created: GoalCreated[]; errors?: unknown[] }

// goal 제출 결과를 호출부에 알리는 판별 유니온.
export type SendGoalOutcome =
  | { kind: 'ok'; created: GoalCreated[] }
  | { kind: 'forbidden' }
  | { kind: 'error'; message: string }

// 선택한 관리자 uuid 목록에게 목표를 전달한다(loopback 무인증, 원격은 403).
export async function sendGoal(text: string, targets: string[]): Promise<SendGoalOutcome> {
  let res: Response
  try {
    res = await fetch('/dashboard/goal', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text, targets }),
    })
  } catch (err) {
    return { kind: 'error', message: err instanceof Error ? err.message : String(err) }
  }

  if (res.status === 403) {
    return { kind: 'forbidden' }
  }
  if (!res.ok) {
    return { kind: 'error', message: 'goal 제출 실패: ' + res.status }
  }
  const data = (await res.json()) as GoalResponse
  return { kind: 'ok', created: data.created }
}
