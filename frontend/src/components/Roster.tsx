// online 감독 로스터를 Card 목록으로 보여주는 표시 전용 패널(폴링은 App 이 담당).
import { Card, Tag, Text, VStack, HStack, Heading } from 'daleui'
import type { Agent } from '../api'

type Props = {
  agents: Agent[]
}

export default function Roster({ agents }: Props) {
  return (
    <VStack align="stretch" gap="12">
      <Heading level={2}>감독 로스터</Heading>
      {agents.length === 0 ? (
        <Text muted>online 감독 없음.</Text>
      ) : (
        agents.map((a) => (
          <Card key={a.uuid} outline>
            <Card.Body>
              <Card.Title>
                {a.uuid}
                {a.display_name ? ' (' + a.display_name + ')' : ''}
              </Card.Title>
              <VStack align="stretch" gap="8">
                <HStack gap="4">
                  <Tag tone="success">online</Tag>
                  {Object.entries(a.tags).map(([k, v]) => (
                    <Tag key={k} tone="neutral">
                      {k}={v}
                    </Tag>
                  ))}
                </HStack>
                <Text as="small" muted>
                  최근 heartbeat: {a.last_heartbeat}
                </Text>
              </VStack>
            </Card.Body>
          </Card>
        ))
      )}
    </VStack>
  )
}
