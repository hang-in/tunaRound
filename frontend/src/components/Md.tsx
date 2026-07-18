// task 본문(요청·결과·이력)의 markdown 렌더러. raw HTML 은 렌더하지 않는다(react-markdown 기본값)
// - task 본문은 에이전트가 만든 신뢰 불가 텍스트라 XSS 표면을 열지 않는 것이 관제탑 원칙.
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

export default function Md({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  )
}
