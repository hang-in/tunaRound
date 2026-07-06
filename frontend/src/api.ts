// 대시보드 브로커 API 타입과 호출 헬퍼(roster 폴, SSE 구독, goal 제출)를 모은 모듈.

// GET /dashboard/roster 응답 요소(등록된 감독 전체 + online 플래그를 서버가 계산해 내려준다).
export type Agent = {
  uuid: string
  tags: Record<string, string>
  display_name: string | null
  last_heartbeat: string
  online: boolean
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

// online 감독 목록을 가져온다. 실패는 던져서 호출부가 콘솔 로깅만 하도록 한다.
export async function fetchRoster(signal?: AbortSignal): Promise<Agent[]> {
  const res = await fetch('/dashboard/roster', { signal })
  if (!res.ok) {
    throw new Error('roster 조회 실패: ' + res.status)
  }
  return (await res.json()) as Agent[]
}

// GET /dashboard/candidates 응답 요소(발견 리포터가 보고한 미무장 세션 후보).
// armed 는 저장값이 아니라 브로커가 online roster 소속으로 계산한 overlay(무장되면 자동 true).
export type Candidate = {
  uuid: string
  runner: string
  project: string | null
  machine: string | null
  source: string
  age_secs: number
  reported_at: string
  armed: boolean
}

// 발견된 세션 후보 목록을 가져온다(v2-40 S2). 실패는 던져서 호출부가 콘솔 로깅만 하도록 한다.
export async function fetchCandidates(signal?: AbortSignal): Promise<Candidate[]> {
  const res = await fetch('/dashboard/candidates', { signal })
  if (!res.ok) {
    throw new Error('candidates 조회 실패: ' + res.status)
  }
  return (await res.json()) as Candidate[]
}

// POST /dashboard/goal 성공 응답: 대상별로 생성된 task 를 알려준다.
export type GoalCreated = { taskId: string; toAgent: string }
type GoalResponse = { created: GoalCreated[]; errors?: unknown[] }

// goal 제출 결과를 호출부에 알리는 판별 유니온.
export type SendGoalOutcome =
  | { kind: 'ok'; created: GoalCreated[] }
  | { kind: 'forbidden' }
  | { kind: 'error'; message: string }

// 선택한 감독 uuid 목록에게 목표를 전달한다(loopback 무인증, 원격은 403).
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

// codex 직접 제어 결과(POST /dashboard/control 응답).
export type SendControlOutcome =
  | { kind: 'ok'; answer: string }
  | { kind: 'forbidden' }
  | { kind: 'error'; message: string }

// codex app-server 세션(ws)에 turn/start를 직접 주입한다(v2-40 S4, loopback 무인증, 원격은 403).
export async function sendControl(ws: string, text: string): Promise<SendControlOutcome> {
  let res: Response
  try {
    res = await fetch('/dashboard/control', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ ws, text }),
    })
  } catch (err) {
    return { kind: 'error', message: err instanceof Error ? err.message : String(err) }
  }
  if (res.status === 403) {
    return { kind: 'forbidden' }
  }
  if (!res.ok) {
    const detail = await res.text().catch(() => '')
    return { kind: 'error', message: 'codex 제어 실패: ' + res.status + (detail ? ' — ' + detail : '') }
  }
  const data = (await res.json()) as { answer: string }
  return { kind: 'ok', answer: data.answer }
}
