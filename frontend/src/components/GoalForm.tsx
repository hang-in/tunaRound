// 목표 텍스트와 대상 감독, 브로커 토큰을 입력받아 POST /a2a 로 task 를 생성하는 폼.
import { useState } from 'react'
import {
  Card,
  Heading,
  Text,
  Button,
  TextInput,
  PasswordInput,
  Select,
  VStack,
} from 'daleui'
import type { Agent, SendGoalOutcome } from '../api'
import { ALL_SUPERVISORS, sendGoal } from '../api'

// 토큰을 세션 동안 보관해 재입력을 줄인다(Authorization 헤더로만 전송, 로깅 금지).
const TOKEN_KEY = 'tuna_dash_token'

type Props = {
  // 대상 Select 옵션을 채우기 위한 현재 로스터.
  agents: Agent[]
}

export default function GoalForm({ agents }: Props) {
  const [token, setToken] = useState<string>(() => {
    try {
      return sessionStorage.getItem(TOKEN_KEY) ?? ''
    } catch {
      return ''
    }
  })
  const [goal, setGoal] = useState('')
  const [target, setTarget] = useState<string>(ALL_SUPERVISORS)
  const [status, setStatus] = useState('')
  const [submitting, setSubmitting] = useState(false)

  // 상태 메시지를 사람이 읽는 문구로 변환한다.
  const describe = (outcome: SendGoalOutcome): string => {
    if (outcome.kind === 'ok') {
      return '생성됨: task ' + outcome.taskId.slice(0, 8) + ' -> ' + outcome.toAgent
    }
    if (outcome.kind === 'unauthorized') {
      return '인증 실패: 토큰 확인.'
    }
    return outcome.message
  }

  const onSubmit = async () => {
    const t = token.trim()
    const g = goal.trim()
    if (!t) {
      setStatus('브로커 토큰을 입력하세요.')
      return
    }
    if (!g) {
      setStatus('목표를 입력하세요.')
      return
    }
    try {
      sessionStorage.setItem(TOKEN_KEY, t)
    } catch {
      // 세션 저장 불가 환경은 무시하고 진행한다.
    }
    setSubmitting(true)
    setStatus('')
    try {
      const outcome = await sendGoal(t, target, g)
      setStatus(describe(outcome))
      if (outcome.kind === 'ok') {
        // 성공 시 목표 필드만 비운다(토큰/대상은 유지).
        setGoal('')
      }
    } catch (err) {
      console.error('[goal] 제출 실패.', err)
      setStatus('제출 실패: 네트워크 또는 서버 오류.')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Card>
      <Card.Body>
        <VStack align="stretch" gap="12">
          <Heading level={2}>목표 제출</Heading>
          <PasswordInput
            label="브로커 토큰"
            placeholder="Bearer 토큰"
            value={token}
            onChange={(e) => setToken(e.target.value)}
          />
          <TextInput
            label="목표"
            placeholder="예: 대시보드 T2 스켈레톤 생성"
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
          />
          <Select
            label="대상"
            aria-label="대상 감독"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
          >
            <option value={ALL_SUPERVISORS}>모든 감독 (role=supervised)</option>
            {agents.map((a) => (
              <option key={a.uuid} value={'agent:' + a.uuid}>
                {a.display_name ? a.display_name + ' (' + a.uuid + ')' : a.uuid}
              </option>
            ))}
          </Select>
          <Button
            tone="brand"
            variant="solid"
            type="button"
            loading={submitting}
            disabled={submitting}
            onClick={onSubmit}
          >
            제출
          </Button>
          {status ? <Text>{status}</Text> : null}
        </VStack>
      </Card.Body>
    </Card>
  )
}
