import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import React from "react";
import "./styles/app.css";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { HudApp } from "./hud/HudApp";
import { initTheme } from "./lib/theme";

// One bundle serves every window; the label decides what renders.
const isHud = getCurrentWebviewWindow().label === "hud";
if (!isHud) {
  initTheme();
}

// biome-ignore lint/style/noNonNullAssertion: #root is guaranteed by index.html
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{isHud ? <HudApp /> : <App />}</React.StrictMode>,
);
