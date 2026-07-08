import type { DictationTranslation, HistoryItem } from "../lib/dictation";
import { CheckIcon, CopyIcon } from "../ui/icons";

interface Props {
  history: HistoryItem[];
  /** On-demand translations (F8) keyed by dictation id, shown below the original. */
  translations: Record<string, DictationTranslation[]>;
  notice: string | null;
  copiedId: string | null;
  onClearAll: () => void;
  onOpenMenu: (id: string, x: number, y: number) => void;
  onCopy: (id: string, text: string) => void;
}

/** The right-hand column of the Dictation screen: the recent-dictation history
 * with copy / ⋯ actions and a "Clear all" affordance. */
export function RecentDictations({
  history,
  translations,
  notice,
  copiedId,
  onClearAll,
  onOpenMenu,
  onCopy,
}: Props) {
  return (
    <div className="panel" style={{ width: 300, flex: "0 0 300px" }}>
      <div className="panel-head hstack spread">
        <span className="section-label">Recent dictations</span>
        {history.length > 0 ? (
          <button type="button" className="btn-sm btn-danger" onClick={onClearAll}>
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
                onOpenMenu(item.id, e.clientX, e.clientY);
              }}
            >
              {item.translatedText ? (
                <>
                  <div style={{ fontSize: 13, lineHeight: 1.55, marginBottom: 3 }}>
                    {item.translatedText}
                  </div>
                  <div className="muted" style={{ fontSize: 12, lineHeight: 1.5, marginBottom: 6 }}>
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
              {(translations[item.id] ?? []).map((t) => (
                <div key={t.id} style={{ fontSize: 13, lineHeight: 1.55, marginTop: 4 }}>
                  <span className="mono muted" style={{ fontSize: 9.5, marginRight: 6 }}>
                    {t.lang}
                  </span>
                  {t.text}
                </div>
              ))}
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
                    onClick={() => onCopy(item.id, item.translatedText ?? item.cleanText)}
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
                    onClick={(e) => onOpenMenu(item.id, e.clientX, e.clientY)}
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
  );
}
