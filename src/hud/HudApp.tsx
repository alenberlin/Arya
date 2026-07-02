import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import "./hud.css";

type HudState = "idle" | "preparing-model" | "recording" | "processing" | "pasting" | "error";

interface StateEvent {
  state: HudState;
  message: string | null;
  text: string | null;
}

const BAR_COUNT = 7;

const STATE_LABEL: Record<HudState, string> = {
  idle: "Done",
  "preparing-model": "Preparing model…",
  recording: "Listening",
  processing: "Transcribing…",
  pasting: "Inserting…",
  error: "Error",
};

/**
 * The dictation pill: a small always-on-top overlay showing live input
 * levels and pipeline state. Shown/hidden by the Rust dictation service.
 */
export function HudApp() {
  const [state, setState] = useState<HudState>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [level, setLevel] = useState(0);
  const peakRef = useRef(0);

  useEffect(() => {
    const unlistenState = listen<StateEvent>("dictation:state", (event) => {
      setState(event.payload.state);
      setMessage(event.payload.message);
    });
    const unlistenLevel = listen<number>("dictation:level", (event) => {
      // Gentle peak-hold so bars feel alive at speech levels.
      peakRef.current = Math.max(event.payload * 3, peakRef.current * 0.85);
      setLevel(Math.min(1, peakRef.current));
    });
    return () => {
      void unlistenState.then((fn) => fn());
      void unlistenLevel.then((fn) => fn());
    };
  }, []);

  const active = state === "recording";
  return (
    <div className={`hud hud-${state}`} data-tauri-drag-region>
      <span className="hud-dot" />
      <div className="hud-bars" aria-hidden>
        {Array.from({ length: BAR_COUNT }, (_, i) => {
          const threshold = (i + 1) / BAR_COUNT;
          const height = active ? Math.max(0.15, Math.min(1, level / threshold) * 0.9) : 0.15;
          return (
            <span
              key={`bar-${threshold}`}
              className="hud-bar"
              style={{ transform: `scaleY(${height})` }}
            />
          );
        })}
      </div>
      <span className="hud-label">
        {state === "error" && message ? message : STATE_LABEL[state]}
      </span>
    </div>
  );
}
