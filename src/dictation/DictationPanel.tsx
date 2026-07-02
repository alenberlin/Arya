import { useCallback, useEffect, useState } from "react";
import {
  createDictionaryEntry,
  type DictationSettings,
  type DictationStatus,
  type DictionaryItem,
  deleteDictationHistoryItem,
  deleteDictionaryEntry,
  getDictationSettings,
  getDictationStatus,
  type HistoryItem,
  listDictationHistory,
  listDictionaryEntries,
  openAccessibilitySettings,
  setDictationSettings,
} from "../lib/dictation";

/**
 * Dictation settings, history, and dictionary. Functional surface for M3;
 * the polished settings shell lands in M13.
 */
export function DictationPanel() {
  const [settings, setSettings] = useState<DictationSettings | null>(null);
  const [status, setStatus] = useState<DictationStatus | null>(null);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [dictionary, setDictionary] = useState<DictionaryItem[]>([]);
  const [pattern, setPattern] = useState("");
  const [replacement, setReplacement] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [s, st, h, d] = await Promise.all([
        getDictationSettings(),
        getDictationStatus(),
        listDictationHistory(),
        listDictionaryEntries(),
      ]);
      setSettings(s);
      setStatus(st);
      setHistory(h);
      setDictionary(d);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

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
    return <p>Loading dictation settings…</p>;
  }

  return (
    <section>
      <h2>Dictation</h2>
      {error ? <p role="alert">{error}</p> : null}
      {status && !status.accessibilityTrusted ? (
        <p role="alert">
          Accessibility permission is required to insert text.{" "}
          <button type="button" onClick={() => void openAccessibilitySettings()}>
            Open System Settings
          </button>
        </p>
      ) : null}

      <fieldset>
        <legend>Shortcut</legend>
        <label>
          Hotkey{" "}
          <input
            value={settings.shortcut}
            onChange={(e) => setSettings({ ...settings, shortcut: e.target.value })}
            onBlur={() => void save(settings)}
            aria-label="dictation hotkey"
          />
        </label>{" "}
        <label>
          Mode{" "}
          <select
            value={settings.mode}
            onChange={(e) =>
              void save({ ...settings, mode: e.target.value as DictationSettings["mode"] })
            }
          >
            <option value="push-to-talk">Push to talk</option>
            <option value="toggle">Toggle</option>
          </select>
        </label>{" "}
        <label>
          Style{" "}
          <select
            value={settings.style}
            onChange={(e) =>
              void save({ ...settings, style: e.target.value as DictationSettings["style"] })
            }
          >
            <option value="standard">Standard</option>
            <option value="casual-lowercase">Casual lowercase</option>
            <option value="formal">Formal</option>
          </select>
        </label>
        {saved ? <small> Saved.</small> : null}
      </fieldset>

      <fieldset>
        <legend>Microphone</legend>
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
      </fieldset>

      <fieldset>
        <legend>Dictionary</legend>
        <form
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
          />{" "}
          <input
            placeholder="replace with"
            value={replacement}
            onChange={(e) => setReplacement(e.target.value)}
            aria-label="dictionary replacement"
          />{" "}
          <button type="submit">Add</button>
        </form>
        <ul aria-label="dictionary">
          {dictionary.map((entry) => (
            <li key={entry.id}>
              {entry.pattern} → {entry.replacement}{" "}
              <button
                type="button"
                onClick={() => void deleteDictionaryEntry(entry.id).then(refresh)}
              >
                Delete
              </button>
            </li>
          ))}
        </ul>
      </fieldset>

      <fieldset>
        <legend>History</legend>
        <ul aria-label="dictation history">
          {history.map((item) => (
            <li key={item.id}>
              <span>{item.cleanText}</span>{" "}
              <small>
                {(item.durationMs / 1000).toFixed(1)}s · ASR {item.asrMs}ms
                {item.appBundleId ? ` · ${item.appBundleId}` : ""}
              </small>{" "}
              <button
                type="button"
                onClick={() => void deleteDictationHistoryItem(item.id).then(refresh)}
              >
                Delete
              </button>
            </li>
          ))}
        </ul>
        {history.length === 0 ? (
          <p>No dictations yet. Hold {settings.shortcut} and speak.</p>
        ) : null}
      </fieldset>
    </section>
  );
}
