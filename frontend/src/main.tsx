import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
// 폰트(DaleUI peerDep)와 DaleUI 스타일을 진입점에서 한 번에 로드한다.
import 'pretendard/dist/web/variable/pretendardvariable.css'
import '@fontsource-variable/jetbrains-mono/index.css'
import 'daleui/styles.css'
// 대시보드 레이아웃 클래스(.dash/.dash-grid)는 daleui 스타일 뒤에 로드한다.
import './index.css'
import App from './App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
