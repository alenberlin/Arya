import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { HudApp } from "./hud/HudApp";

// One bundle serves every window; the label decides what renders.
const isHud = getCurrentWebviewWindow().label === "hud";

// biome-ignore lint/style/noNonNullAssertion: #root is guaranteed by index.html
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{isHud ? <HudApp /> : <App />}</React.StrictMode>,
);
