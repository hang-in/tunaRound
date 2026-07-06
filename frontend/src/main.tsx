import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
// 폰트는 유지(목업이 Pretendard/JetBrains Mono 를 쓴다). daleui 스타일은 더 이상 쓰지 않는다.
import 'pretendard/dist/web/variable/pretendardvariable.css'
import '@fontsource-variable/jetbrains-mono/index.css'
// 대시보드 디자인 토큰 + 리셋 + 컴포넌트 클래스(목업 이식).
import './index.css'
import App from './App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
