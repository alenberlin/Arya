import { LockIcon } from "../ui/icons";
import { ModelSettings } from "./ModelSettings";

/** On-device model privacy tiers. */
function PrivacyTiers() {
  return (
    <div className="card" style={{ marginBottom: 14 }}>
      <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 14 }}>Model privacy tiers</div>
      <div className="tier-row">
        <span className="tier-dot" style={{ background: "var(--success)" }} />
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13.5, fontWeight: 500 }}>Local models</div>
          <div className="muted" style={{ fontSize: 12 }}>
            Run on your Mac via Ollama. Nothing leaves the device.
          </div>
        </div>
        <span className="badge badge-success">On-device</span>
      </div>
      <div className="tier-row">
        <span className="tier-dot" style={{ background: "var(--success)" }} />
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13.5, fontWeight: 500 }}>Whisper · speech</div>
          <div className="muted" style={{ fontSize: 12 }}>
            Transcription &amp; dictation, fully offline.
          </div>
        </div>
        <span className="badge badge-success">On-device</span>
      </div>
      <div className="tier-row">
        <span className="tier-dot" style={{ background: "var(--warning)" }} />
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13.5, fontWeight: 500 }}>Cloud models</div>
          <div className="muted" style={{ fontSize: 12 }}>
            Optional. Uses your own API key, sent directly to the provider you choose.
          </div>
        </div>
        <span className="badge badge-warning">Cloud</span>
      </div>
    </div>
  );
}

/** Account: cloud API keys + on-device model setup. Arya is free and local-first. */
export function AccountPanel() {
  return (
    <div className="screen-center">
      <div className="screen-col narrow">
        <h1 style={{ marginBottom: 20 }}>Account</h1>
        <div className="card hstack" style={{ gap: 16, marginBottom: 14 }}>
          <span className="avatar avatar-lg">
            <LockIcon />
          </span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: 17, fontWeight: 600 }}>Local-first</div>
            <div className="muted" style={{ fontSize: 13 }}>
              Everything runs on your Mac — no account needed. Cloud models are optional and use
              your own key.
            </div>
          </div>
        </div>
        <PrivacyTiers />
        <ModelSettings />
        <p className="muted" style={{ fontSize: 12 }}>
          Arya is free. Local models and on-device speech never cost anything. Cloud models are
          optional and billed by your provider against your own key.
        </p>
      </div>
    </div>
  );
}
