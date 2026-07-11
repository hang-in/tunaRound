// 위임 이력 검색 섹션(v2-47 #5): P6a로 messages/FTS에 색인된 종결 task(요청·결과)를 형태소+FTS로
// 검색한다(read-only). GET /dashboard/search?q= 를 디바운스로 질의. "지난주 mac에 맡긴 진단이 뭐였지"를 웹에서.
import { useEffect, useState } from 'react'
import type { SearchResult } from '../api'
import { searchHistory } from '../api'

const DEBOUNCE_MS = 400

// speaker(`a2a/<agent>`)에서 표시용 이름(에이전트)만 뽑는다.
function speakerName(speaker: string): string {
  return speaker.startsWith('a2a/') ? speaker.slice(4) : speaker
}

type SearchState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'done'; results: SearchResult[] }
  | { kind: 'error' }

export default function SearchPanel() {
  const [query, setQuery] = useState('')
  const [state, setState] = useState<SearchState>({ kind: 'idle' })

  useEffect(() => {
    const q = query.trim()
    if (q === '') {
      setState({ kind: 'idle' })
      return
    }
    const controller = new AbortController()
    // 입력이 멎은 뒤에만 질의한다(디바운스). 새 입력·언마운트 시 이전 요청은 abort.
    const timer = window.setTimeout(() => {
      setState({ kind: 'loading' })
      searchHistory(q, controller.signal)
        .then((res) => setState({ kind: 'done', results: res.results }))
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

  return (
    <section className="search-section">
      <div className="search-header">
        <h2 className="section-title">위임 이력 검색</h2>
        <span className="search-hint">종결 task 요청·결과(P6a 색인) 형태소 검색</span>
      </div>
      <input
        type="search"
        className="search-box"
        placeholder="예: 진단, mac, flaky 테스트 …"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      <div className="search-results">
        {state.kind === 'idle' ? (
          <div className="search-empty">질의를 입력하면 위임 이력을 검색합니다.</div>
        ) : state.kind === 'loading' ? (
          <div className="search-empty">검색 중…</div>
        ) : state.kind === 'error' ? (
          <div className="search-empty">검색 실패.</div>
        ) : state.results.length === 0 ? (
          <div className="search-empty">일치하는 이력이 없습니다.</div>
        ) : (
          state.results.map((r) => (
            // speaker(a2a/<agent>) 에는 "::" 가 없어 경계 모호성 없이 유일 키가 된다(동일 화자·동일
            // content 중복은 드물고 leaf·전체 교체라 무해).
            <div className="search-hit" key={`${r.speaker}::${r.content}`}>
              <span className="search-hit-speaker">{speakerName(r.speaker)}</span>
              <div className="search-hit-content">{r.content}</div>
            </div>
          ))
        )}
      </div>
    </section>
  )
}
