import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type MouseEvent, useEffect, useRef, useState } from "react";
import {
  dictationCancel,
  dictationPinApp,
  dictationSetSessionPolish,
  dictationStop,
  dictationUnpinApp,
  type Polish,
} from "../lib/dictation";
import { applyTheme, type Theme } from "../lib/theme";
import "./hud.css";

type HudState = "idle" | "preparing-model" | "recording" | "processing" | "pasting" | "error";

interface StateEvent {
  state: HudState;
  message: string | null;
  text: string | null;
}

interface TargetApp {
  name: string | null;
  bundleId: string | null;
  polish: Polish;
  style: string;
  pinned: boolean;
}

const BAR_COUNT = 5;

const STATE_LABEL: Record<HudState, string> = {
  idle: "Done",
  "preparing-model": "Preparing…",
  recording: "Listening",
  processing: "Transcribing…",
  pasting: "Inserting…",
  error: "Error",
};

const POLISH_LEVELS: { value: Polish; label: string }[] = [
  { value: "raw", label: "Raw" },
  { value: "clean", label: "Clean" },
  { value: "polished", label: "Polished" },
];
const polishLabel = (p: Polish) => POLISH_LEVELS.find((l) => l.value === p)?.label ?? "Clean";

/**
 * The pill owns its own theme, independent of the main window: a floating
 * overlay should stay legible over whatever app is behind it, not follow the
 * app's own light/dark choice. Persisted under a separate key.
 */
const HUD_THEME_KEY = "arya-hud-theme";
const THEME_CYCLE: Theme[] = ["system", "light", "dark"];
const THEME_LABEL: Record<Theme, string> = { system: "Auto", light: "Light", dark: "Dark" };
function loadHudTheme(): Theme {
  const t = localStorage.getItem(HUD_THEME_KEY);
  return t === "light" || t === "dark" || t === "system" ? t : "system";
}

/**
 * The dictation "command pill": a floating, draggable overlay at the bottom of
 * the screen that shows live input level, streams the transcript as you speak,
 * and puts the controls inline — a theme dot (Auto/Light/Dark), the mode, and
 * an AI-polish chip that cycles and reveals a level picker in place. In
 * hands-free mode it adds Stop (insert) and ✕ (cancel), and the chips collapse
 * to icons to make room. It only appears while a dictation is active; the Rust
 * service shows/hides the window, kept sized to this card so it grows upward.
 */
export function HudApp() {
  const [state, setState] = useState<HudState>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [level, setLevel] = useState(0);
  const [handsFree, setHandsFree] = useState(false);
  const [partial, setPartial] = useState("");
  const [finalText, setFinalText] = useState("");
  const [target, setTarget] = useState<TargetApp | null>(null);
  const [polish, setPolish] = useState<Polish>("clean");
  const [pinned, setPinned] = useState(false);
  const [polishOpen, setPolishOpen] = useState(false);
  const [hudTheme, setHudTheme] = useState<Theme>(loadHudTheme);
  const peakRef = useRef(0);
  const wrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unlistenState = listen<StateEvent>("dictation:state", (event) => {
      setState(event.payload.state);
      setMessage(event.payload.message);
      if (event.payload.state === "recording") {
        setPartial("");
        setFinalText("");
      } else {
        // Recording ended: a settings panel opened mid-dictation has no purpose.
        setPolishOpen(false);
      }
      if (event.payload.state === "idle") {
        setHandsFree(false);
        if (event.payload.text) setFinalText(event.payload.text);
      }
      if (event.payload.state === "error") setHandsFree(false);
    });
    const unlistenPartial = listen<string>("dictation:partial", (event) => {
      setPartial(event.payload);
    });
    const unlistenHandsFree = listen<boolean>("dictation:hands-free", (event) => {
      setHandsFree(event.payload);
    });
    const unlistenLevel = listen<number>("dictation:level", (event) => {
      // Gentle peak-hold so bars feel alive at speech levels.
      peakRef.current = Math.max(event.payload * 3, peakRef.current * 0.85);
      setLevel(Math.min(1, peakRef.current));
    });
    const unlistenTarget = listen<TargetApp>("dictation:target", (event) => {
      setTarget(event.payload);
      setPolish(event.payload.polish);
      setPinned(event.payload.pinned);
    });
    return () => {
      void unlistenState.then((fn) => fn());
      void unlistenPartial.then((fn) => fn());
      void unlistenHandsFree.then((fn) => fn());
      void unlistenLevel.then((fn) => fn());
      void unlistenTarget.then((fn) => fn());
    };
  }, []);

  // Apply the pill's own theme, and follow the OS while on Auto.
  useEffect(() => {
    applyTheme(hudTheme);
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      if (hudTheme === "system") applyTheme("system");
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [hudTheme]);

  // Keep the native window sized to the card, so it grows as the transcript
  // streams in and the panel expands, and never clips or leaves dead space.
  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    let lastW = 0;
    let lastH = 0;
    const send = () => {
      const width = Math.ceil(el.offsetWidth);
      const height = Math.ceil(el.offsetHeight);
      if (width !== lastW || height !== lastH) {
        lastW = width;
        lastH = height;
        void invoke("hud_resize", { width, height });
      }
    };
    const observer = new ResizeObserver(send);
    observer.observe(el);
    send();
    return () => observer.disconnect();
  }, []);

  const active = state === "recording";
  const showControls = handsFree && (state === "recording" || state === "processing");
  // Chips (mode, polish, theme) belong to the interactive moments; hide them
  // while preparing/processing/pasting/erroring to keep the pill quiet.
  const showChips = state === "recording" || state === "idle";
  // Hands-free needs room for Stop/✕, so chips drop to icon-only.
  const compact = showControls;
  const label =
    state === "error" && message
      ? message
      : handsFree && state === "recording"
        ? "Hands-free"
        : STATE_LABEL[state];
  const liveText = active || state === "processing" ? partial : "";
  const isInterim = liveText !== "";
  const text = state === "idle" ? finalText : liveText;
  const stop = (e: MouseEvent) => e.stopPropagation();

  // Changing polish is a one-off for the current dictation, not a global save.
  const applyPolish = (value: Polish) => {
    setPolish(value);
    setPinned(false);
    void dictationSetSessionPolish(value).catch(() => {});
  };
  // Tap the chip: advance to the next level and reveal the whole set in place.
  const cyclePolish = () => {
    const i = POLISH_LEVELS.findIndex((l) => l.value === polish);
    applyPolish(POLISH_LEVELS[(i + 1) % POLISH_LEVELS.length].value);
    setPolishOpen(true);
  };
  // Tap a segment: jump straight to it and collapse.
  const pickPolish = (value: Polish) => {
    applyPolish(value);
    setPolishOpen(false);
  };
  // Pin (or unpin) the current polish as this app's default going forward.
  const togglePin = () => {
    if (pinned) {
      setPinned(false);
      void dictationUnpinApp().catch(() => {});
    } else {
      setPinned(true);
      void dictationPinApp(polish).catch(() => {});
    }
  };
  const cycleTheme = () => {
    const next = THEME_CYCLE[(THEME_CYCLE.indexOf(hudTheme) + 1) % THEME_CYCLE.length];
    localStorage.setItem(HUD_THEME_KEY, next);
    setHudTheme(next);
  };

  return (
    <div className="hud-wrap" ref={wrapRef}>
      <div className={`hud-card hud-${state}`} data-tauri-drag-region>
        <div className="hud-row">
          <span className="hud-brand" aria-hidden>
            <WaveMark />
          </span>
          <div className="hud-wave" aria-hidden>
            {Array.from({ length: BAR_COUNT }, (_, i) => {
              const threshold = (i + 1) / BAR_COUNT;
              const height = active ? Math.max(0.2, Math.min(1, level / threshold)) : 0.2;
              return (
                <span
                  key={`bar-${threshold}`}
                  className="hud-bar"
                  style={{ transform: `scaleY(${height})` }}
                />
              );
            })}
          </div>

          {showChips ? (
            <>
              <span className="hud-chip hud-chip-static" title="Dictate">
                <MicIcon />
                <span className={compact ? "hud-sr" : undefined}>Dictate</span>
              </span>
              <button
                type="button"
                className={`hud-chip${polishOpen ? " is-open" : ""}`}
                aria-label={`AI polish: ${polishLabel(polish)}`}
                aria-expanded={polishOpen}
                onMouseDown={stop}
                onClick={cyclePolish}
              >
                <SparkIcon />
                {compact ? null : <span>{polishLabel(polish)}</span>}
                {compact ? null : <Chevron open={polishOpen} />}
              </button>
            </>
          ) : null}

          <span className="hud-spacer" />
          <span className="hud-label">{label}</span>

          {showChips ? (
            <button
              type="button"
              className="hud-dot"
              aria-label={`Theme: ${THEME_LABEL[hudTheme]}`}
              title={`Theme: ${THEME_LABEL[hudTheme]}`}
              onMouseDown={stop}
              onClick={cycleTheme}
            >
              <ThemeGlyph theme={hudTheme} />
            </button>
          ) : null}

          {showChips && !compact && target?.name ? (
            <span className="hud-chip hud-dest" title={`Inserting into ${target.name}`}>
              <DestIcon />
              <span>{target.name}</span>
            </span>
          ) : null}

          {showControls ? (
            <>
              <button
                type="button"
                className="hud-stop"
                onMouseDown={stop}
                onClick={() => void dictationStop()}
              >
                Stop
              </button>
              <button
                type="button"
                className="hud-x"
                aria-label="Cancel"
                onMouseDown={stop}
                onClick={() => void dictationCancel()}
              >
                ✕
              </button>
            </>
          ) : null}
        </div>

        {polishOpen && showChips ? (
          <div className="hud-panel">
            <fieldset className="hud-seg" aria-label="AI polish level">
              {POLISH_LEVELS.map((l) => (
                <button
                  key={l.value}
                  type="button"
                  className="hud-seg-btn"
                  aria-pressed={polish === l.value}
                  onMouseDown={stop}
                  onClick={() => pickPolish(l.value)}
                >
                  {l.label}
                </button>
              ))}
            </fieldset>
            {target?.name ? (
              <button
                type="button"
                className={`hud-pin${pinned ? " is-pinned" : ""}`}
                aria-pressed={pinned}
                onMouseDown={stop}
                onClick={togglePin}
              >
                <PinIcon filled={pinned} />
                {pinned ? `Pinned for ${target.name}` : `Pin for ${target.name}`}
              </button>
            ) : null}
          </div>
        ) : null}

        {text ? (
          <div className={`hud-text${isInterim ? " is-interim" : ""}`}>
            {text}
            {isInterim ? <span className="hud-caret" aria-hidden /> : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** Compact waveform brand glyph for the pill. */
function WaveMark() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M2 8h1.4M12.6 8H14M4.7 5v6M8 2.5v11M11.3 5.5v5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

function MicIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="5.5" y="1.8" width="5" height="8" rx="2.5" />
      <path d="M3.4 7.6a4.6 4.6 0 0 0 9.2 0" />
      <path d="M8 12.2V14.2" />
    </svg>
  );
}

function DestIcon() {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M2 8h8" />
      <path d="M7 5l3 3-3 3" />
      <path d="M13.5 3v10" />
    </svg>
  );
}

function SparkIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
      <path d="M8 1.6l1.4 4L13.4 7l-4 1.4L8 12.4 6.6 8.4 2.6 7l4-1.4z" />
    </svg>
  );
}

function PinIcon({ filled }: { filled: boolean }) {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 16 16"
      fill={filled ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M8 2c-2.2 0-4 1.7-4 3.8 0 2.8 4 8.2 4 8.2s4-5.4 4-8.2C12 3.7 10.2 2 8 2z" />
    </svg>
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      width="11"
      height="11"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={open ? "M4 10l4-4 4 4" : "M4 6l4 4 4-4"} />
    </svg>
  );
}

function ThemeGlyph({ theme }: { theme: Theme }) {
  if (theme === "light") {
    return (
      <svg
        width="13"
        height="13"
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        aria-hidden="true"
      >
        <circle cx="8" cy="8" r="3.1" />
        <path d="M8 1.6v1.5M8 12.9v1.5M1.6 8h1.5M12.9 8h1.5M3.5 3.5l1 1M11.5 11.5l1 1M12.5 3.5l-1 1M4.5 11.5l-1 1" />
      </svg>
    );
  }
  if (theme === "dark") {
    return (
      <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
        <path d="M13 9.7A5.5 5.5 0 0 1 6.3 3 5.5 5.5 0 1 0 13 9.7z" />
      </svg>
    );
  }
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      aria-hidden="true"
    >
      <circle cx="8" cy="8" r="5.5" />
      <path d="M8 2.5a5.5 5.5 0 0 1 0 11z" fill="currentColor" stroke="none" />
    </svg>
  );
}
