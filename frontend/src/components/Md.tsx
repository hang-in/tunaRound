// task 본문(요청·결과·이력)의 markdown 렌더러. raw HTML 은 렌더하지 않는다(react-markdown 기본값)
// - task 본문은 에이전트가 만든 신뢰 불가 텍스트라 XSS 표면을 열지 않는 것이 관제탑 원칙.
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

export default function Md({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          // 링크는 새 탭으로 - 현재 탭 이동은 SSE 연결·피드 상태를 잃는다(gemini 리뷰). noopener 로 역참조 차단.
          a: ({ node: _node, ...props }) => <a {...props} target="_blank" rel="noopener noreferrer" />,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  )
}
