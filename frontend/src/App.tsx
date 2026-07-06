// 총감독 대시보드 루트. roster 폴링을 소유하고 헤더/로스터/피드/goal 폼 3요소를 배치한다.
import { useCallback, useEffect, useState } from 'react'
import { Heading, Text, Tag, HStack, VStack } from 'daleui'
import type { Agent } from './api'
import { fetchRoster } from './api'
import Roster from './components/Roster'
import Feed from './components/Feed'
import GoalForm from './components/GoalForm'

export default function App() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [connected, setConnected] = useState(false)

  // roster 를 5초 주기로 폴링해 로스터 패널과 goal 폼 Select 가 공유한다.
  useEffect(() => {
    let cancelled = false
    const controller = new AbortController()

    const load = () => {
      fetchRoster(controller.signal)
        .then((list) => {
          if (!cancelled) setAgents(list)
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          console.error('[roster] 조회 실패.', err)
        })
    }

    load()
    const timer = window.setInterval(load, 5000)
    return () => {
      cancelled = true
      controller.abort()
      window.clearInterval(timer)
    }
  }, [])

  // Feed 의 useEffect 재실행(재연결)을 막기 위해 안정적인 콜백으로 감싼다.
  const handleConnected = useCallback((v: boolean) => setConnected(v), [])

  return (
    <div className="dash">
      <VStack align="stretch" gap="20">
        <HStack gap="8">
          <Heading level={1}>총감독 대시보드</Heading>
          <Tag tone={connected ? 'success' : 'neutral'}>
            {connected ? '피드 연결됨' : '피드 끊김'}
          </Tag>
        </HStack>
        <Text muted>
          online 감독을 확인하고, task 이벤트를 실시간으로 보며, 목표를 던집니다.
        </Text>
        <div className="dash-grid">
          <Roster agents={agents} />
          <Feed onConnectedChange={handleConnected} />
        </div>
        <GoalForm agents={agents} />
      </VStack>
    </div>
  )
}
