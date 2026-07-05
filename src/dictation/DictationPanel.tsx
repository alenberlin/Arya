import { useCallback, useEffect, useRef, useState } from "react";
import {
  clearDictationHistory,
  convertDictationToNote,
  copyToClipboard,
  createDictionaryEntry,
  type DictationSettings,
  type DictationStatus,
  type DictionaryItem,
  deleteDictationHistoryItem,
  deleteDictionaryEntry,
  deleteSpeakerProfile,
  dictationPrepareStreaming,
  enrollSpeakerProfile,
  getDictationSettings,
  getDictationStatus,
  type HistoryItem,
  listDictationHistory,
  listDictionaryEntries,
  listOllamaModels,
  listSpeakerProfiles,
  openAccessibilitySettings,
  type SpeakerProfile,
  setDictationSettings,
} from "../lib/dictation";
import { TypeToConfirmDialog } from "../ui/dialogs";
import { CheckIcon, CopyIcon } from "../ui/icons";

const STYLES: { value: DictationSettings["style"]; label: string }[] = [
  { value: "standard", label: "Standard" },
  { value: "casual-lowercase", label: "Casual" },
  { value: "formal", label: "Formal" },
];

// Curated target languages; the value is the language name passed to the model.
const LANGUAGES = [
  "German",
  "Spanish",
  "French",
  "Italian",
  "Portuguese",
  "Dutch",
  "Polish",
  "Russian",
  "Turkish",
  "Japanese",
  "Korean",
  "Chinese",
  "Arabic",
  "Hindi",
];

/** Dictation settings (hotkey, style, mic, voice profiles, dictionary) beside a
 * recent-dictations column. */
export function DictationPanel() {
  const [settings, setSettings] = useState<DictationSettings | null>(null);
  const [status, setStatus] = useState<DictationStatus | null>(null);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [dictionary, setDictionary] = useState<DictionaryItem[]>([]);
  const [profiles, setProfiles] = useState<SpeakerProfile[]>([]);
  const [profileName, setProfileName] = useState("");
  const [enrolling, setEnrolling] = useState(false);
  const [pattern, setPattern] = useState("");
  const [replacement, setReplacement] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [clearOpen, setClearOpen] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [menuFor, setMenuFor] = useState<{ id: string; x: number; y: number } | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [ollamaModels, setOllamaModels] = useState<string[]>([]);
  const menuRef = useRef<HTMLDivElement | null>(null);

  // Open the ⋯ menu, clamped so it never spills off the bottom/right edge.
  const openMenu = (id: string, clientX: number, clientY: number) => {
    const MENU_W = 240;
    const MENU_H = 104;
    const x = Math.max(8, Math.min(clientX, window.innerWidth - MENU_W - 8));
    const y = Math.max(8, Math.min(clientY, window.innerHeight - MENU_H - 8));
    setMenuFor({ id, x, y });
  };

  const copyText = async (id: string, text: string) => {
    try {
      await copyToClipboard(text);
      setCopiedId(id);
      window.setTimeout(() => setCopiedId((c) => (c === id ? null : c)), 1400);
    } catch (e) {
      setError(String(e));
    }
  };

  // Close the ⋯ menu on any outside click or Escape.
  useEffect(() => {
    if (!menuFor) return;
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && e.target instanceof Node && menuRef.current.contains(e.target)) return;
      setMenuFor(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenuFor(null);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [menuFor]);

  const convertToMinutes = (id: string) => {
    setMenuFor(null);
    setNotice("Generating meeting minutes…");
    void convertDictationToNote(id)
      .then(() => setNotice("Meeting minutes created — see the Notes tab."))
      .catch((e) => {
        setNotice(null);
        setError(String(e));
      });
  };

  const refresh = useCallback(async () => {
    try {
      const [s, st, h, d, sp] = await Promise.all([
        getDictationSettings(),
        getDictationStatus(),
        listDictationHistory(),
        listDictionaryEntries(),
        listSpeakerProfiles(),
      ]);
      setSettings(s);
      setStatus(st);
      setHistory(h);
      setDictionary(d);
      setProfiles(sp);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Best-effort: populate the translation-model picker from Ollama.
  useEffect(() => {
    void listOllamaModels()
      .then(setOllamaModels)
      .catch(() => {});
  }, []);

  const save = async (next: DictationSettings) => {
    setSettings(next);
    setSaved(false);
    try {
      await setDictationSettings(next);
      setError(null);
      setSaved(true);
    } catch (e) {
      setError(String(e));
    }
  };

  if (!settings) {
    return (
      <div className="screen">
        <div className="panel panel-grow">
          <div className="panel-body">
            <p style={{ padding: 20 }}>Loading dictation settings…</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="screen">
      {/* SETTINGS PANEL */}
      <div className="panel panel-grow">
        <div className="panel-body" style={{ padding: "26px 30px" }}>
          <h1>Dictation</h1>
          <p className="muted" style={{ margin: "4px 0 24px" }}>
            Hold a hotkey, speak, release — cleaned-up text pastes into any app. Speech never leaves
            your Mac.
          </p>

          {error ? (
            <p role="alert" style={{ marginBottom: 12 }}>
              {error}
            </p>
          ) : null}
          {status && !status.accessibilityTrusted ? (
            <div className="banner banner-warning" role="alert" style={{ marginBottom: 14 }}>
              <span>Accessibility permission is required to insert text.</span>
              <button
                type="button"
                className="btn-sm"
                onClick={() => void openAccessibilitySettings()}
              >
                Open System Settings
              </button>
            </div>
          ) : null}

          <div className="stack" style={{ maxWidth: 560, gap: 14 }}>
            <div className="card-sunken">
              <div className="spread hstack">
                <div>
                  <div style={{ fontSize: 14, fontWeight: 600 }}>Hotkey</div>
                  <div className="muted" style={{ fontSize: 12.5 }}>
                    Hold to dictate
                  </div>
                </div>
                <input
                  className="mono"
                  style={{ width: 130, textAlign: "center" }}
                  value={settings.shortcut}
                  onChange={(e) => setSettings({ ...settings, shortcut: e.target.value })}
                  onBlur={() => void save(settings)}
                  aria-label="dictation hotkey"
                />
              </div>
              <div className="spread hstack" style={{ marginTop: 14 }}>
                <div style={{ fontSize: 14, fontWeight: 600 }}>Mode</div>
                <select
                  aria-label="mode"
                  style={{ width: "auto" }}
                  value={settings.mode}
                  onChange={(e) =>
                    void save({ ...settings, mode: e.target.value as DictationSettings["mode"] })
                  }
                >
                  <option value="push-to-talk">Push to talk</option>
                  <option value="toggle">Toggle</option>
                </select>
              </div>
              {saved ? (
                <small style={{ display: "block", marginTop: 10, color: "var(--success)" }}>
                  Saved.
                </small>
              ) : null}
            </div>

            <div className="card-sunken">
              <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 10 }}>Style</div>
              <div className="seg" style={{ width: "100%", gap: 8 }}>
                {STYLES.map((s) => (
                  <button
                    key={s.value}
                    type="button"
                    className="seg-btn"
                    aria-pressed={settings.style === s.value}
                    style={{ flex: 1 }}
                    onClick={() => void save({ ...settings, style: s.value })}
                  >
                    {s.label}
                  </button>
                ))}
              </div>
            </div>

            <div className="card-sunken">
              <div className="spread hstack">
                <div style={{ fontSize: 14, fontWeight: 600 }}>Translate to</div>
                <select
                  aria-label="translate to"
                  style={{ width: "auto" }}
                  value={settings.translate ?? ""}
                  onChange={(e) => void save({ ...settings, translate: e.target.value || null })}
                >
                  <option value="">Off</option>
                  {LANGUAGES.map((l) => (
                    <option key={l} value={l}>
                      {l}
                    </option>
                  ))}
                </select>
              </div>
              {settings.translate ? (
                <>
                  <div className="spread hstack" style={{ marginTop: 12 }}>
                    <div className="muted" style={{ fontSize: 12.5, maxWidth: 320 }}>
                      You dictate in English; the text lands translated. History keeps both.
                    </div>
                    <select
                      aria-label="translation engine"
                      style={{ width: "auto" }}
                      value={settings.translateProvider}
                      onChange={(e) =>
                        void save({
                          ...settings,
                          translateProvider: e.target
                            .value as DictationSettings["translateProvider"],
                        })
                      }
                    >
                      <option value="local">Local (private)</option>
                      <option value="cloud">Cloud</option>
                    </select>
                  </div>
                  {settings.translateProvider === "local" ? (
                    <div className="spread hstack" style={{ marginTop: 10 }}>
                      <div className="muted" style={{ fontSize: 12.5 }}>
                        Local model
                      </div>
                      <select
                        aria-label="translation model"
                        style={{ width: "auto", maxWidth: 280 }}
                        value={settings.translateModel ?? ""}
                        onChange={(e) =>
                          void save({ ...settings, translateModel: e.target.value || null })
                        }
                      >
                        <option value="">Auto</option>
                        {ollamaModels.map((m) => (
                          <option key={m} value={m}>
                            {m}
                          </option>
                        ))}
                      </select>
                    </div>
                  ) : null}
                </>
              ) : null}
            </div>

            <div className="card-sunken">
              <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 10 }}>Microphone</div>
              <select
                value={settings.microphone ?? ""}
                onChange={(e) => void save({ ...settings, microphone: e.target.value || null })}
                aria-label="microphone device"
              >
                <option value="">System default</option>
                {status?.inputDevices.map((device) => (
                  <option key={device} value={device}>
                    {device}
                  </option>
                ))}
              </select>
            </div>

            <div className="card-sunken">
              <label className="spread hstack" style={{ cursor: "pointer" }}>
                <div>
                  <div style={{ fontSize: 14, fontWeight: 600 }}>Live streaming preview</div>
                  <div className="muted" style={{ fontSize: 12.5 }}>
                    Show words as you speak. Downloads a small model on first use; the inserted text
                    still uses Whisper.
                  </div>
                </div>
                <input
                  type="checkbox"
                  checked={settings.streaming}
                  onChange={(e) => {
                    const on = e.target.checked;
                    void save({ ...settings, streaming: on });
                    if (on) {
                      setNotice("Preparing the streaming model…");
                      void dictationPrepareStreaming()
                        .then(() => setNotice("Streaming preview ready."))
                        .catch((err) => {
                          setNotice(null);
                          setError(String(err));
                        });
                    }
                  }}
                  aria-label="live streaming preview"
                />
              </label>
            </div>

            <div className="card-sunken">
              <div className="spread hstack" style={{ marginBottom: 12 }}>
                <div style={{ fontSize: 14, fontWeight: 600 }}>Voice profiles</div>
              </div>
              <small className="muted" style={{ display: "block", marginBottom: 10 }}>
                Enroll voices so meeting notes use real names. Speak naturally for six seconds after
                pressing Enroll.
              </small>
              <ul aria-label="voice profiles" className="plain hstack wrap" style={{ gap: 10 }}>
                {profiles.map((profile) => (
                  <li key={profile.id}>
                    <span className="chip">
                      <span className="avatar" style={{ width: 22, height: 22, fontSize: 11 }}>
                        {profile.name.charAt(0).toUpperCase()}
                      </span>
                      {profile.name}
                      <button
                        type="button"
                        className="tab-close bare"
                        aria-label={`delete ${profile.name}`}
                        onClick={() => void deleteSpeakerProfile(profile.id).then(refresh)}
                      >
                        ×
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
              <form
                className="hstack"
                style={{ marginTop: 10 }}
                onSubmit={(e) => {
                  e.preventDefault();
                  if (!profileName.trim()) return;
                  setEnrolling(true);
                  void enrollSpeakerProfile(profileName.trim())
                    .then(() => {
                      setProfileName("");
                      return refresh();
                    })
                    .catch((err) => setError(String(err)))
                    .finally(() => setEnrolling(false));
                }}
              >
                <input
                  placeholder="name"
                  value={profileName}
                  onChange={(e) => setProfileName(e.target.value)}
                  aria-label="profile name"
                />
                <button type="submit" disabled={enrolling}>
                  {enrolling ? "Listening…" : "Enroll (6s)"}
                </button>
              </form>
            </div>

            <div className="card-sunken">
              <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 10 }}>
                Custom dictionary
              </div>
              <ul aria-label="dictionary" className="plain hstack wrap" style={{ gap: 7 }}>
                {dictionary.map((entry) => (
                  <li key={entry.id}>
                    <span className="chip chip-mono">
                      {entry.pattern} → {entry.replacement}
                      <button
                        type="button"
                        className="tab-close bare"
                        aria-label={`delete ${entry.pattern}`}
                        onClick={() => void deleteDictionaryEntry(entry.id).then(refresh)}
                      >
                        ×
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
              <form
                className="hstack"
                style={{ marginTop: 10 }}
                onSubmit={(e) => {
                  e.preventDefault();
                  void createDictionaryEntry(pattern, replacement)
                    .then(() => {
                      setPattern("");
                      setReplacement("");
                      return refresh();
                    })
                    .catch((err) => setError(String(err)));
                }}
              >
                <input
                  placeholder="heard as"
                  value={pattern}
                  onChange={(e) => setPattern(e.target.value)}
                  aria-label="dictionary pattern"
                />
                <input
                  placeholder="replace with"
                  value={replacement}
                  onChange={(e) => setReplacement(e.target.value)}
                  aria-label="dictionary replacement"
                />
                <button type="submit">Add</button>
              </form>
            </div>
          </div>
        </div>
      </div>

      {/* RECENT DICTATIONS PANEL */}
      <div className="panel" style={{ width: 300, flex: "0 0 300px" }}>
        <div className="panel-head hstack spread">
          <span className="section-label">Recent dictations</span>
          {history.length > 0 ? (
            <button type="button" className="btn-sm btn-danger" onClick={() => setClearOpen(true)}>
              Clear all
            </button>
          ) : null}
        </div>
        <div className="panel-body">
          {notice ? (
            <p role="status" className="muted" style={{ fontSize: 12, padding: "0 4px 8px" }}>
              {notice}
            </p>
          ) : null}
          <ul aria-label="dictation history" className="plain">
            {history.map((item) => (
              <li
                key={item.id}
                className="card-sunken"
                style={{ margin: "4px 0", padding: 12 }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  openMenu(item.id, e.clientX, e.clientY);
                }}
              >
                {item.translatedText ? (
                  <>
                    <div style={{ fontSize: 13, lineHeight: 1.55, marginBottom: 3 }}>
                      {item.translatedText}
                    </div>
                    <div
                      className="muted"
                      style={{ fontSize: 12, lineHeight: 1.5, marginBottom: 6 }}
                    >
                      <span className="mono" style={{ fontSize: 9.5 }}>
                        {item.targetLang ? `${item.targetLang} ← EN` : "EN"}
                      </span>{" "}
                      {item.cleanText}
                    </div>
                  </>
                ) : (
                  <div style={{ fontSize: 13, lineHeight: 1.55, marginBottom: 6 }}>
                    {item.cleanText}
                  </div>
                )}
                <div className="spread hstack">
                  <span className="mono muted" style={{ fontSize: 10.5 }}>
                    {(item.durationMs / 1000).toFixed(1)}s · ASR {item.asrMs}ms
                    {item.appBundleId ? ` · ${item.appBundleId}` : ""}
                  </span>
                  <span className="hstack" style={{ gap: 2 }}>
                    <button
                      type="button"
                      className="tab-close bare"
                      aria-label={copiedId === item.id ? "copied" : "copy text"}
                      title={copiedId === item.id ? "Copied!" : "Copy text"}
                      onClick={() => void copyText(item.id, item.translatedText ?? item.cleanText)}
                    >
                      {copiedId === item.id ? (
                        <CheckIcon className="hist-action-icon copied" />
                      ) : (
                        <CopyIcon className="hist-action-icon" />
                      )}
                    </button>
                    <button
                      type="button"
                      className="tab-close bare"
                      aria-label="dictation actions"
                      onClick={(e) => openMenu(item.id, e.clientX, e.clientY)}
                    >
                      ⋯
                    </button>
                  </span>
                </div>
              </li>
            ))}
          </ul>
          {history.length === 0 ? (
            <p className="muted" style={{ fontSize: 13, padding: 12 }}>
              No dictations yet. Hold <span className="kbd">Right Shift</span> anywhere and speak.
            </p>
          ) : null}
        </div>
      </div>

      {menuFor ? (
        <div
          ref={menuRef}
          className="context-menu"
          style={{ top: menuFor.y, left: menuFor.x }}
          role="menu"
        >
          <button type="button" role="menuitem" onClick={() => convertToMinutes(menuFor.id)}>
            Convert to meeting minutes
          </button>
          <button
            type="button"
            role="menuitem"
            className="danger"
            onClick={() => {
              const id = menuFor.id;
              setMenuFor(null);
              void deleteDictationHistoryItem(id).then(refresh);
            }}
          >
            Delete
          </button>
        </div>
      ) : null}

      <TypeToConfirmDialog
        open={clearOpen}
        title="Clear all dictation history?"
        message="This permanently deletes every saved dictation and cannot be undone."
        phrase="confirm"
        confirmLabel="Delete everything"
        onConfirm={() => {
          setClearOpen(false);
          void clearDictationHistory()
            .then(refresh)
            .catch((e) => setError(String(e)));
        }}
        onCancel={() => setClearOpen(false)}
      />
    </div>
  );
}
