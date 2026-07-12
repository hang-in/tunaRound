// 상단 헤더(목업 header.hdr): 브랜드+버전 / omnisearch(위임 이력 검색, SearchPanel 흡수) /
// 목표 제출 버튼·테마 토글·알림 토글·연결 배지(브레싱 닷)·시계. 모두 sticky 전폭.
import { useEffect, useState } from 'react'
import { Bell, Eye, Moon, Plus, Search, Sun } from 'lucide-react'
import type { SearchResult } from '../api'
import { searchHistory } from '../api'

type Props = {
  // 헤더 v{version} 표시(health.version). null이면 표시 생략.
  version: string | null
  brokerOk: boolean
  sseOpen: boolean
  remoteViewer: boolean
  notifySupported: boolean
  notifyOn: boolean
  onToggleNotify: () => void
  // 'light' | 'dark' 유효 테마 + 토글.
  theme: 'light' | 'dark'
  onToggleTheme: () => void
  // 목표 제출 모달 오픈(원격이어도 오픈 → 모달이 관전 경고를 띄운다).
  onOpenGoal: () => void
}

const DEBOUNCE_MS = 400

// speaker(`a2a/<agent>`)에서 표시용 이름만 뽑는다.
function speakerName(speaker: string): string {
  return speaker.startsWith('a2a/') ? speaker.slice(4) : speaker
}

type SearchState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'done'; results: SearchResult[] }
  | { kind: 'error' }

// 위임 이력 검색(GET /dashboard/search?q=) 디바운스 질의 + 결과 드롭다운. 결과 read-only. 클릭=전문 펼침.
function Omnisearch() {
  const [query, setQuery] = useState('')
  const [state, setState] = useState<SearchState>({ kind: 'idle' })
  const [openIdx, setOpenIdx] = useState<number | null>(null)

  useEffect(() => {
    const q = query.trim()
    if (q === '') {
      setState({ kind: 'idle' })
      return
    }
    const controller = new AbortController()
    const timer = window.setTimeout(() => {
      setState({ kind: 'loading' })
      searchHistory(q, controller.signal)
        .then((res) => {
          setState({ kind: 'done', results: res.results })
          setOpenIdx(null)
        })
        .catch((err) => {
          if (err instanceof DOMException && err.name === 'AbortError') return
          console.error('[search] 검색 실패.', err)
          setState({ kind: 'error' })
        })
    }, DEBOUNCE_MS)
    return () => {
      controller.abort()
      window.clearTimeout(timer)
    }
  }, [query])

  const showDd = query.trim() !== ''

  return (
    <div className="omnisearch">
      <div className="box">
        <Search size={16} />
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="위임 이력 검색(종결 task)"
          aria-label="위임 이력 검색"
          onKeyDown={(e) => {
            if (e.key === 'Escape') setQuery('')
          }}
        />
        <span className="label">위임 검색</span>
      </div>
      {showDd ? (
        <div className="dd">
          {state.kind === 'loading' ? (
            <div className="foot">검색 중…</div>
          ) : state.kind === 'error' ? (
            <div className="foot">검색 실패.</div>
          ) : state.kind === 'done' && state.results.length === 0 ? (
            <div className="foot">일치하는 이력이 없습니다.</div>
          ) : state.kind === 'done' ? (
            <>
              {state.results.map((r, i) => (
                <button
                  type="button"
                  className={'hit' + (openIdx === i ? ' open' : '')}
                  // speaker(a2a/<agent>)에는 "::"가 없어 speaker::content가 경계 모호성 없는 안정 키다
                  // (index-as-key 지양, 레포 규약). 동일 화자·동일 content 중복은 드물고 leaf·전체 교체라 무해.
                  key={`${r.speaker}::${r.content}`}
                  onClick={() => setOpenIdx((cur) => (cur === i ? null : i))}
                >
                  <span className="who">{speakerName(r.speaker)}</span>
                  <div className="txt">{r.content}</div>
                </button>
              ))}
              <div className="foot">
                {state.results.length}건 · 종결 task 색인(a2a/*) 검색 · 클릭하면 전문
              </div>
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

// "오전/오후 h:mm:ss" 형식의 라이브 시계 문자열을 만든다.
function formatClock(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return pad(d.getHours()) + ':' + pad(d.getMinutes()) + ':' + pad(d.getSeconds())
}

export default function Header({
  version,
  brokerOk,
  sseOpen,
  remoteViewer,
  notifySupported,
  notifyOn,
  onToggleNotify,
  theme,
  onToggleTheme,
  onOpenGoal,
}: Props) {
  const [now, setNow] = useState(() => new Date())

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 1000)
    return () => window.clearInterval(timer)
  }, [])

  const connected = brokerOk && sseOpen

  return (
    <header className="hdr">
      <div className="brand">
        <span className="logo" aria-hidden="true" />
        <span className="name">tunaRound</span>
        {version ? <span className="ver mono">v{version}</span> : null}
      </div>

      <Omnisearch />

      <div className="hdr-actions">
        <button type="button" className="btn" onClick={onOpenGoal}>
          <Plus size={15} />
          목표 제출
        </button>
        <button
          type="button"
          className="iconbtn"
          onClick={onToggleTheme}
          title={theme === 'dark' ? '라이트 모드로' : '다크 모드로'}
          aria-label="테마 전환"
        >
          {theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}
        </button>
        {notifySupported ? (
          <button
            type="button"
            className={'iconbtn' + (notifyOn ? ' on' : '')}
            onClick={onToggleNotify}
            title={notifyOn ? 'task 완료/실패 데스크톱 알림 켜짐' : 'task 완료/실패 데스크톱 알림 받기'}
            aria-pressed={notifyOn}
            aria-label="완료·실패 알림"
          >
            <Bell size={16} />
          </button>
        ) : null}
        {remoteViewer ? (
          <span className="conn" title="원격 접속 = 읽기 전용 관전. 목표 제출·제어는 로컬(loopback)에서만 가능합니다.">
            <Eye size={13} />
            관전
          </span>
        ) : null}
        <span className="conn" title={connected ? '브로커 연결됨 · 피드 SSE 수신 중' : '연결 끊김(브로커 또는 SSE)'}>
          <span className={'dot' + (connected ? '' : ' off')} />
          {connected ? '연결됨' : '끊김'}
        </span>
        <span className="clock mono">{formatClock(now)}</span>
      </div>
    </header>
  )
}
