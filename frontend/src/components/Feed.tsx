// /dashboard/events SSE 를 구독해 task 이벤트를 실시간으로 최신순 표시하는 피드(상위 200개 유지).
import { useEffect, useRef, useState } from 'react'
import { Card, Tag, Text, VStack, HStack, Heading } from 'daleui'
import type { TaskEventMsg } from '../api'

// 화면 목록용 항목. seq 로 안정적인 React key 를 부여한다(같은 task 가 여러 번 와도 구분).
type FeedRow = { seq: number; msg: TaskEventMsg }

// task id 앞 8자리만 축약해 보여준다.
function short(id: string): string {
  return id.slice(0, 8)
}

type Props = {
  // 피드 SSE 연결 상태를 상위(헤더 연결 뱃지)로 올린다.
  onConnectedChange: (connected: boolean) => void
}

export default function Feed({ onConnectedChange }: Props) {
  const [rows, setRows] = useState<FeedRow[]>([])
  // seq는 useEffect 재실행(StrictMode 이중호출 포함)에도 유지돼야 key 중복이 안 난다.
  const seqRef = useRef(0)

  useEffect(() => {
    const source = new EventSource('/dashboard/events')

    source.onopen = () => onConnectedChange(true)

    source.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data) as TaskEventMsg
        seqRef.current += 1
        const row: FeedRow = { seq: seqRef.current, msg }
        // 최신을 위로 prepend 하고 상위 200개만 유지한다.
        setRows((prev) => [row, ...prev].slice(0, 200))
      } catch (err) {
        console.error('[feed] 이벤트 파싱 실패.', err)
      }
    }

    source.onerror = (err) => {
      onConnectedChange(false)
      console.error('[feed] SSE 오류.', err)
    }

    return () => {
      source.close()
      onConnectedChange(false)
    }
  }, [onConnectedChange])

  return (
    <VStack align="stretch" gap="12">
      <Heading level={2}>라이브 task 피드</Heading>
      {rows.length === 0 ? (
        <Text muted>task 이벤트 대기 중.</Text>
      ) : (
        rows.map(({ seq, msg }) => {
          const t = msg.task
          const firstArtifact = t.artifacts[0]?.parts[0]?.text
          return (
            <Card key={seq} outline>
              <Card.Body>
                <VStack align="stretch" gap="4">
                  <HStack gap="4">
                    <Tag tone={msg.event === 'completed' ? 'success' : 'info'}>
                      {msg.event}
                    </Tag>
                    <Text weight="bold">{short(t.id)}</Text>
                    <Tag tone="neutral">{t.state}</Tag>
                  </HStack>
                  <Text as="small" muted>
                    {t.fromAgent} -&gt; {t.toAgent}
                  </Text>
                  {firstArtifact ? <Text size="sm">{firstArtifact}</Text> : null}
                </VStack>
              </Card.Body>
            </Card>
          )
        })
      )}
    </VStack>
  )
}
