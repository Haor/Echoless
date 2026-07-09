import "./devBrowserShim";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { frontendLog } from "./api";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { LangProvider } from "./i18n";
import "./styles.css";

// 全局兜底落盘(logs/echoless-*.log):ErrorBoundary 只逮渲染期异常,
// 事件回调/异步里的错误走这两个钩子。fire-and-forget,自身绝不再抛。
window.addEventListener("error", (e) => {
  frontendLog("error", `window.onerror: ${e.error?.stack ?? e.message}`);
});
window.addEventListener("unhandledrejection", (e) => {
  const r = e.reason;
  frontendLog(
    "error",
    `unhandledrejection: ${r instanceof Error ? (r.stack ?? r.message) : String(r)}`,
  );
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <LangProvider>
      <ErrorBoundary label="root">
        <App />
      </ErrorBoundary>
    </LangProvider>
  </React.StrictMode>,
);
