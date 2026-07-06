// 대시보드 브로커 API 타입과 호출 헬퍼(roster 폴, SSE 구독, goal 제출)를 모은 모듈.

// GET /dashboard/roster 응답 요소(online 감독만 서버가 필터해 반환한다).
export type Agent = {
  uuid: string
  tags: Record<string, string>
  display_name: string | null
  last_heartbeat: string
}

// task 아티팩트의 한 조각.
export type Part = { text?: string }

// task 산출물.
export type Artifact = {
  artifactId?: string
  name?: string | null
  parts: Part[]
}

// 브로커 task 스냅샷.
export type Task = {
  id: string
  contextId?: string
  fromAgent: string
  toAgent: string
  state: string
  statusMessage?: unknown
  artifacts: Artifact[]
  history?: unknown[]
  runner?: string
  createdAt: string
  updatedAt: string
}

// SSE 이벤트 메시지(event.data 를 파싱한 형태).
export type TaskEventMsg = {
  event: 'status' | 'completed'
  task: Task
}

// POST /a2a 성공 응답.
type SendMessageResult = {
  jsonrpc: string
  id: number
  result?: Task
  error?: { code: number; message: string }
}

// goal 제출 결과를 호출부에 알리는 판별 유니온.
export type SendGoalOutcome =
  | { kind: 'ok'; taskId: string; toAgent: string }
  | { kind: 'unauthorized' }
  | { kind: 'error'; message: string }

// 유니크 messageId 생성기. crypto.randomUUID 가 있으면 쓰고 없으면 폴백한다.
export function newMessageId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return 'm-' + Date.now() + '-' + Math.random().toString(16).slice(2)
}

// online 감독 목록을 가져온다. 실패는 던져서 호출부가 콘솔 로깅만 하도록 한다.
export async function fetchRoster(signal?: AbortSignal): Promise<Agent[]> {
  const res = await fetch('/dashboard/roster', { signal })
  if (!res.ok) {
    throw new Error('roster 조회 실패: ' + res.status)
  }
  return (await res.json()) as Agent[]
}

// Select 대상 값 규약: "sel:role=supervised"(→ toSelector) / "agent:<uuid>"(→ toAgent).
// 모든 감독을 뜻하는 기본 셀렉터 값.
export const ALL_SUPERVISORS = 'sel:role=supervised'

// goal 을 브로커에 제출한다. target 은 Select 값 규약(sel: / agent:)을 따른다.
export async function sendGoal(
  token: string,
  target: string,
  text: string,
): Promise<SendGoalOutcome> {
  // 대상 값을 toAgent 또는 toSelector 로 배타 변환한다.
  const params: {
    message: { messageId: string; role: 'user'; parts: Part[] }
    fromAgent: string
    toAgent?: string
    toSelector?: string
  } = {
    message: { messageId: newMessageId(), role: 'user', parts: [{ text }] },
    fromAgent: 'dashboard',
  }
  if (target.startsWith('agent:')) {
    params.toAgent = target.slice('agent:'.length)
  } else if (target.startsWith('sel:')) {
    params.toSelector = target.slice('sel:'.length)
  } else {
    // 방어적 기본값: 알 수 없는 값은 모든 감독 셀렉터로 처리한다.
    params.toSelector = 'role=supervised'
  }

  const res = await fetch('/a2a', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: 'Bearer ' + token,
    },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'SendMessage', params }),
  })

  if (res.status === 401) {
    return { kind: 'unauthorized' }
  }

  const data = (await res.json()) as SendMessageResult
  if (data.error) {
    return { kind: 'error', message: data.error.message }
  }
  if (data.result) {
    return { kind: 'ok', taskId: data.result.id, toAgent: data.result.toAgent }
  }
  return { kind: 'error', message: '알 수 없는 응답 형식입니다.' }
}
