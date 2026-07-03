import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import brand from "../../brand.json";
import { getDictationStatus, openAccessibilitySettings } from "../lib/dictation";

const STORAGE_KEY = "arya-onboarded";

export function onboardingComplete(): boolean {
  return localStorage.getItem(STORAGE_KEY) === "true";
}

type Step = "welcome" | "privacy" | "permissions" | "dictation" | "done";
const ORDER: Step[] = ["welcome", "privacy", "permissions", "dictation", "done"];

/**
 * First-run onboarding: one idea per screen. Explains the privacy model,
 * primes the permissions the pillars need, and has the user try dictation
 * once so the core loop is proven before they're dropped into the app.
 */
export function Onboarding({ onFinish }: { onFinish: () => void }) {
  const [step, setStep] = useState<Step>(() => {
    const saved = localStorage.getItem("arya-onboarding-step");
    return (ORDER.includes(saved as Step) ? saved : "welcome") as Step;
  });
  const [accessibility, setAccessibility] = useState(false);
  const [micDevices, setMicDevices] = useState<string[]>([]);
  const [dictationText, setDictationText] = useState<string | null>(null);

  useEffect(() => {
    localStorage.setItem("arya-onboarding-step", step);
  }, [step]);

  useEffect(() => {
    const poll = setInterval(() => {
      void getDictationStatus()
        .then((s) => {
          setAccessibility(s.accessibilityTrusted);
          setMicDevices(s.inputDevices);
        })
        .catch(() => {});
    }, 1000);
    const unlisten = listen<{ state: string; text: string | null }>("dictation:state", (e) => {
      if (e.payload.state === "idle" && e.payload.text) {
        setDictationText(e.payload.text);
      }
    });
    return () => {
      clearInterval(poll);
      void unlisten.then((fn) => fn());
    };
  }, []);

  const go = (next: Step) => setStep(next);
  const finish = () => {
    localStorage.setItem(STORAGE_KEY, "true");
    onFinish();
  };

  return (
    <main
      style={{
        fontFamily: "var(--font-sans)",
        maxWidth: 560,
        margin: "0 auto",
        padding: "var(--space-6)",
        textAlign: "center",
      }}
    >
      {step === "welcome" ? (
        <>
          <h1>Welcome to {brand.name}</h1>
          <p>
            Private AI on your Mac: chat and a local agent, system-wide dictation, and bot-free
            meeting notes. Speech never leaves this machine.
          </p>
          <button type="button" className="primary" onClick={() => go("privacy")}>
            Get started
          </button>
        </>
      ) : null}

      {step === "privacy" ? (
        <>
          <h1>Private by design</h1>
          <ul style={{ textAlign: "left", lineHeight: 1.7 }}>
            <li>
              <strong>Local by default.</strong> Notes, recordings, transcripts, and history live
              only on your Mac.
            </li>
            <li>
              <strong>On-device speech.</strong> Transcription, dictation, and diarization run
              locally, even offline.
            </li>
            <li>
              <strong>Your choice of model.</strong> Free local models, or cloud models routed
              through an open-source proxy that keeps keys off your machine.
            </li>
          </ul>
          <div style={{ display: "flex", gap: "var(--space-2)", justifyContent: "center" }}>
            <button type="button" onClick={() => go("welcome")}>
              Back
            </button>
            <button type="button" className="primary" onClick={() => go("permissions")}>
              Continue
            </button>
          </div>
        </>
      ) : null}

      {step === "permissions" ? (
        <>
          <h1>Two quick permissions</h1>
          <p>Arya needs these to hear you and to type for you. Grant them when macOS asks.</p>
          <div style={{ textAlign: "left", margin: "var(--space-4) 0", lineHeight: 1.8 }}>
            <div>
              {micDevices.length > 0 ? "✓" : "○"} <strong>Microphone</strong> — for dictation and
              meeting notes.
            </div>
            <div>
              {accessibility ? "✓" : "○"} <strong>Accessibility</strong> — so dictation can paste
              into any app.{" "}
              {!accessibility ? (
                <button type="button" onClick={() => void openAccessibilitySettings()}>
                  Open settings
                </button>
              ) : null}
            </div>
          </div>
          <div style={{ display: "flex", gap: "var(--space-2)", justifyContent: "center" }}>
            <button type="button" onClick={() => go("privacy")}>
              Back
            </button>
            <button type="button" className="primary" onClick={() => go("dictation")}>
              Continue
            </button>
          </div>
        </>
      ) : null}

      {step === "dictation" ? (
        <>
          <h1>Try dictation</h1>
          <p>
            Hold <kbd>Ctrl+Alt+D</kbd> anywhere, say a sentence, and release. Your cleaned-up words
            paste into whatever app you're in. Try it in a text field, then come back.
          </p>
          {dictationText ? (
            <p role="status" style={{ color: "var(--success)" }}>
              Nice — you dictated: "{dictationText}"
            </p>
          ) : (
            <p>
              <small>Waiting for your first dictation… (you can skip this)</small>
            </p>
          )}
          <div style={{ display: "flex", gap: "var(--space-2)", justifyContent: "center" }}>
            <button type="button" onClick={() => go("permissions")}>
              Back
            </button>
            <button type="button" className="primary" onClick={() => go("done")}>
              {dictationText ? "Continue" : "Skip for now"}
            </button>
          </div>
        </>
      ) : null}

      {step === "done" ? (
        <>
          <h1>You're set</h1>
          <p>
            Record a meeting from the Notes tab, chat with the agent, search your workspace, or set
            a routine. Everything private, on your Mac.
          </p>
          <button type="button" className="primary" onClick={finish}>
            Open {brand.name}
          </button>
        </>
      ) : null}

      <p style={{ marginTop: "var(--space-6)" }}>
        <button
          type="button"
          onClick={() => {
            void invoke("account_signin_state").catch(() => {});
            finish();
          }}
          style={{ border: "none", background: "none", color: "var(--text-muted)" }}
        >
          Skip onboarding
        </button>
      </p>
    </main>
  );
}
