import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import React from "react";
import "./styles/app.css";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { HudApp } from "./hud/HudApp";
import { installFileDropGuard } from "./lib/fileDrop";
import { initTheme } from "./lib/theme";
import { ErrorBoundary } from "./ui/ErrorBoundary";

// One bundle serves every window; the label decides what renders.
const isHud = getCurrentWebviewWindow().label === "hud";
if (!isHud) {
  initTheme();
  // Before React mounts: the main window has no OS drag handler, so an
  // uncancelled file drop would navigate the webview away from the app.
  installFileDropGuard();
}

// biome-ignore lint/style/noNonNullAssertion: #root is guaranteed by index.html
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>{isHud ? <HudApp /> : <App />}</ErrorBoundary>
  </React.StrictMode>,
);
