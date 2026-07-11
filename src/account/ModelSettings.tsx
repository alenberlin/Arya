import { useCallback, useEffect, useRef, useState } from "react";
import {
  deleteSpeechModel,
  downloadSpeechModel,
  formatBytes,
  type KeyProvider,
  type KeysStatus,
  keysClear,
  keysSet,
  keysStatus,
  type OllamaStatus,
  ollamaStatus,
  onSpeechDownloadProgress,
  type SpeechModelStatus,
  speechModelsStatus,
} from "../lib/models";

/**
 * Model setup: the three things a fresh install needs before its AI features
 * work — a cloud API key (or), a running Ollama (or), and a downloaded speech
 * model. Rendered inside the Account screen in local mode.
 */
export function ModelSettings() {
  return (
    <>
      <ApiKeysCard />
      <OllamaCard />
      <SpeechModelsCard />
    </>
  );
}

// ---------------------------------------------------------------------------
// Cloud API keys
// ---------------------------------------------------------------------------

const PROVIDERS: {
  id: KeyProvider;
  label: string;
  placeholder: string;
  href: string;
}[] = [
  {
    id: "anthropic",
    label: "Anthropic (Claude)",
    placeholder: "sk-ant-…",
    href: "https://console.anthropic.com/settings/keys",
  },
  {
    id: "openai",
    label: "OpenAI (GPT)",
    placeholder: "sk-…",
    href: "https://platform.openai.com/api-keys",
  },
];

function ApiKeysCard() {
  const [status, setStatus] = useState<KeysStatus>({ openai: false, anthropic: false });

  useEffect(() => {
    void keysStatus()
      .then(setStatus)
      .catch(() => {});
  }, []);

  return (
    <div className="card" style={{ marginBottom: 14 }}>
      <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 4 }}>Cloud API keys</div>
      <div className="muted" style={{ fontSize: 12, marginBottom: 14 }}>
        Optional. Add your own key to use frontier models when you don't run local ones. Keys are
        stored in your macOS Keychain and sent only to the provider — never to us.
      </div>
      {PROVIDERS.map((p) => (
        <ApiKeyRow key={p.id} provider={p} isSet={status[p.id]} onChange={setStatus} />
      ))}
    </div>
  );
}

function ApiKeyRow({
  provider,
  isSet,
  onChange,
}: {
  provider: (typeof PROVIDERS)[number];
  isSet: boolean;
  onChange: (s: KeysStatus) => void;
}) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);

  const save = async () => {
    if (!value.trim()) return;
    setBusy(true);
    try {
      onChange(await keysSet(provider.id, value.trim()));
      setValue("");
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      onChange(await keysClear(provider.id));
      setValue("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ marginBottom: 12 }}>
      <div className="spread" style={{ alignItems: "baseline", marginBottom: 6 }}>
        <span style={{ fontSize: 13, fontWeight: 500 }}>{provider.label}</span>
        <span className={isSet ? "badge badge-success" : "badge"} style={{ fontSize: 11 }}>
          {isSet ? "Key saved" : "Not set"}
        </span>
      </div>
      <div className="hstack" style={{ gap: 8 }}>
        <input
          type="password"
          className="input"
          style={{ flex: 1, minWidth: 0 }}
          placeholder={isSet ? "Enter a new key to replace" : provider.placeholder}
          value={value}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void save();
          }}
          disabled={busy}
        />
        <button
          type="button"
          className="btn-primary btn-sm"
          onClick={() => void save()}
          disabled={busy || !value.trim()}
        >
          Save
        </button>
        {isSet ? (
          <button
            type="button"
            className="btn-ghost btn-sm"
            onClick={() => void remove()}
            disabled={busy}
          >
            Remove
          </button>
        ) : null}
      </div>
      <a
        className="muted"
        style={{ fontSize: 11 }}
        href={provider.href}
        target="_blank"
        rel="noreferrer"
      >
        Get a key ↗
      </a>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Ollama (local models)
// ---------------------------------------------------------------------------

function OllamaCard() {
  const [status, setStatus] = useState<OllamaStatus | null>(null);
  const [checking, setChecking] = useState(false);

  const check = useCallback(async () => {
    setChecking(true);
    try {
      setStatus(await ollamaStatus());
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    void check();
  }, [check]);

  const reachable = status?.reachable ?? false;
  const noModels = reachable && (status?.modelCount ?? 0) === 0;

  return (
    <div className="card" style={{ marginBottom: 14 }}>
      <div className="spread" style={{ alignItems: "baseline", marginBottom: 4 }}>
        <div style={{ fontSize: 14, fontWeight: 600 }}>Local models (Ollama)</div>
        <span
          className={reachable ? "badge badge-success" : "badge badge-warning"}
          style={{ fontSize: 11 }}
        >
          {reachable
            ? `Connected · ${status?.modelCount} model${status?.modelCount === 1 ? "" : "s"}`
            : "Not detected"}
        </span>
      </div>
      <div className="muted" style={{ fontSize: 12, marginBottom: 12 }}>
        Free, private, and offline — the agent, dictation cleanup, and translation all run on models
        you install with Ollama.
      </div>

      {!reachable ? (
        <ol
          className="muted"
          style={{ fontSize: 12.5, margin: "0 0 12px", paddingLeft: 18, lineHeight: 1.7 }}
        >
          <li>
            Install Ollama from{" "}
            <a href="https://ollama.com/download" target="_blank" rel="noreferrer">
              ollama.com/download
            </a>
            .
          </li>
          <li>
            Open Terminal and pull a model, e.g. <code>ollama pull llama3.2</code>.
          </li>
          <li>Come back and check again — Arya finds it automatically.</li>
        </ol>
      ) : noModels ? (
        <div className="muted" style={{ fontSize: 12.5, marginBottom: 12 }}>
          Ollama is running but has no chat models yet. Pull one, e.g.{" "}
          <code>ollama pull llama3.2</code>.
        </div>
      ) : null}

      <button
        type="button"
        className="btn-ghost btn-sm"
        onClick={() => void check()}
        disabled={checking}
      >
        {checking ? "Checking…" : "Check again"}
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Speech (Whisper) models
// ---------------------------------------------------------------------------

const SPEECH_LABELS: Record<string, string> = {
  "whisper-large-v3-turbo-q5_0": "High accuracy · multilingual",
  "whisper-base.en": "Fast · English only",
  "whisper-tiny.en": "Tiny · English only",
};

function SpeechModelsCard() {
  const [models, setModels] = useState<SpeechModelStatus[]>([]);
  const [progress, setProgress] = useState<Record<string, number>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const busyRef = useRef<string | null>(null);
  busyRef.current = busy;

  const refresh = useCallback(async () => {
    setModels(await speechModelsStatus());
  }, []);

  useEffect(() => {
    void refresh();
    const unlisten = onSpeechDownloadProgress((p) => {
      setProgress((prev) => ({
        ...prev,
        [p.id]: p.total > 0 ? Math.round((p.received / p.total) * 100) : (prev[p.id] ?? 0),
      }));
      if (p.done) void refresh();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh]);

  const download = async (id: string) => {
    setBusy(id);
    setProgress((prev) => ({ ...prev, [id]: 0 }));
    try {
      await downloadSpeechModel(id);
      await refresh();
    } catch {
      // Leave the row as "not downloaded"; the user can retry.
    } finally {
      setBusy(null);
      setProgress((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
    }
  };

  const remove = async (id: string) => {
    setBusy(id);
    try {
      await deleteSpeechModel(id);
      await refresh();
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="card" style={{ marginBottom: 14 }}>
      <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 4 }}>Speech models (Whisper)</div>
      <div className="muted" style={{ fontSize: 12, marginBottom: 14 }}>
        On-device transcription for dictation and meeting notes. Download one ahead of time, or Arya
        fetches your selected model automatically on first use.
      </div>
      {models.map((m) => {
        const downloading = busy === m.id && m.id in progress;
        const pct = progress[m.id] ?? 0;
        return (
          <div key={m.id} style={{ marginBottom: 12 }}>
            <div className="spread" style={{ alignItems: "baseline" }}>
              <span style={{ fontSize: 13, fontWeight: 500 }}>{SPEECH_LABELS[m.id] ?? m.id}</span>
              <span className="mono muted" style={{ fontSize: 11.5 }}>
                {formatBytes(m.approxBytes)}
              </span>
            </div>
            <div className="hstack" style={{ gap: 8, marginTop: 6 }}>
              {m.downloaded ? (
                <>
                  <span className="badge badge-success" style={{ fontSize: 11 }}>
                    Downloaded
                  </span>
                  <button
                    type="button"
                    className="btn-ghost btn-sm"
                    onClick={() => void remove(m.id)}
                    disabled={busy !== null}
                  >
                    Remove
                  </button>
                </>
              ) : downloading ? (
                <div style={{ flex: 1 }}>
                  <div className="meter">
                    <div
                      className="meter-fill"
                      style={{ width: `${pct}%`, background: "var(--accent)" }}
                    />
                  </div>
                  <div className="mono muted" style={{ fontSize: 11, marginTop: 4 }}>
                    Downloading… {pct}%
                  </div>
                </div>
              ) : (
                <button
                  type="button"
                  className="btn-primary btn-sm"
                  onClick={() => void download(m.id)}
                  disabled={busy !== null}
                >
                  Download
                </button>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
