import React from "react"
import ReactDOM from "react-dom/client"
import App from "./App"
import "./index.css"
import { ConfigProvider } from "./hooks/useConfig"

const hideBootSplash = () => {
  const splash = document.getElementById("boot-splash")
  if (!splash) {
    return
  }

  splash.classList.add("boot-splash-hidden")
  window.setTimeout(() => {
    splash.remove()
  }, 220)
}

window.addEventListener("resh-app-ready", hideBootSplash, { once: true })
window.setTimeout(hideBootSplash, 5000)

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ConfigProvider>
      <App />
    </ConfigProvider>
  </React.StrictMode>,
)
