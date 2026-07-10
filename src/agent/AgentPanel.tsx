import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  type AgentEvent,
  type AgentMessage,
  type AgentSession,
  agentCancel,
  agentCreateSession,
  agentDeleteSession,
  agentGenerateImage,
  agentGetMessages,
  agentListModels,
  agentListSessions,
  agentResolveApproval,
  agentSend,
  agentSteer,
  agentWorkspaceReadB64,
  convertSessionToNote,
  modelPrivacy,
  type ToolInfo,
} from "../lib/agent";
import { agentBranchSession } from "../lib/ecosystem";
import { TRANSLATE_LANGUAGES, translateInstruction } from "../lib/languages";
import { getNote, updateNote } from "../lib/notes";
import { aiTransform } from "../lib/transform";
import {
  AgentIcon,
  CheckIcon,
  FileWriteIcon,
  LockIcon,
  MoreIcon,
  PlusIcon,
  SendIcon,
} from "../ui/icons";
import { MessageView, toolSummary } from "./MessageView";

interface PendingApproval {
  callId: string;
  name: string;
  description: string;
}

interface LiveTurn {
  text: string;
  reasoning: string;
  tools: ToolInfo[];
}

/** Agent chat: sessions, streaming turns, tool approvals, steering. */
export function AgentPanel() {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [model, setModel] = useState<string>("");
  const [active, setActive] = useState<AgentSession | null>(null);
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [live, setLive] = useState<LiveTurn | null>(null);
  const [approvals, setApprovals] = useState<PendingApproval[]>([]);
  const [running, setRunning] = useState(false);
  const [input, setInput] = useState("");
  const [steerText, setSteerText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [images, setImages] = useState<Record<string, string>>({});
  const [notice, setNotice] = useState<string | null>(null);
  const [sessionMenu, setSessionMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const sessionMenuRef = useRef<HTMLDivElement | null>(null);
  const activeRef = useRef<string | null>(null);
  activeRef.current = active?.id ?? null;
  const composerRef = useRef<HTMLTextAreaElement>(null);

  // Grow the composer with its content — from a five-row floor (CSS min-height)
  // up to a cap, past which it scrolls — so pasting or writing several lines
  // stays readable without a manual drag-resize. `input` drives it: it's the
  // signal to re-measure after each commit (typing *and* the clear on send),
  // even though the measurement reads the DOM rather than the value itself.
  // biome-ignore lint/correctness/useExhaustiveDependencies: input is the intended re-measure trigger
  useLayoutEffect(() => {
    const el = composerRef.current;
    if (!el) return;
    el.style.height = "auto";
    if (el.scrollHeight > 0) el.style.height = `${el.scrollHeight}px`;
  }, [input]);

  const refreshSessions = useCallback(async () => {
    try {
      setSessions(await agentListSessions());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const openSession = useCallback(async (session: AgentSession) => {
    setActive(session);
    setLive(null);
    setApprovals([]);
    try {
      setMessages(await agentGetMessages(session.id));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // Auto-dismiss transient notices.
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(t);
  }, [notice]);

  const openSessionMenu = (id: string, clientX: number, clientY: number) => {
    const x = Math.max(8, Math.min(clientX, window.innerWidth - 232));
    const y = Math.max(8, Math.min(clientY, window.innerHeight - 300));
    setSessionMenu({ id, x, y });
  };

  // Close the session ⋯ menu on any outside click or Escape.
  useEffect(() => {
    if (!sessionMenu) return;
    const onDown = (e: MouseEvent) => {
      if (
        sessionMenuRef.current &&
        e.target instanceof Node &&
        sessionMenuRef.current.contains(e.target)
      ) {
        return;
      }
      setSessionMenu(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setSessionMenu(null);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [sessionMenu]);

  /** Convert a chat to a note (markdown transcript). */
  const convertChatToNote = (id: string) => {
    setSessionMenu(null);
    setNotice("Creating note…");
    void convertSessionToNote(id)
      .then(() => setNotice("Note created — see the Notes tab."))
      .catch((e) => {
        setNotice(null);
        setError(String(e));
      });
  };

  /** Convert a chat to a note, then append its translation side-by-side. The
   * note is fresh markdown (no rich blocks), so appending is lossless. */
  const translateChatToNote = (id: string, lang: string) => {
    setSessionMenu(null);
    setNotice(`Translating chat to ${lang}…`);
    void convertSessionToNote(id)
      .then(async (noteId) => {
        const note = await getNote(noteId);
        const translated = await aiTransform(note.bodyMd, translateInstruction(lang));
        const combined = `${note.bodyMd}\n\n---\n\n## ${lang}\n\n${translated}`;
        await updateNote(noteId, { bodyMd: combined });
        setNotice(`Translated chat to ${lang} — see the Notes tab.`);
      })
      .catch((e) => {
        setNotice(null);
        setError(String(e));
      });
  };

  useEffect(() => {
    void refreshSessions();
    void agentListModels()
      .then((list) => {
        setModels(list);
        setModel((current) => current || list[0] || "");
      })
      .catch((e) => setError(String(e)));

    const unlisten = listen<{ sessionId: string; event: AgentEvent }>("agent:event", (raw) => {
      const { sessionId, event } = raw.payload;
      if (sessionId !== activeRef.current) return;
      switch (event.kind) {
        case "turn-started":
          setRunning(true);
          setLive({ text: "", reasoning: "", tools: [] });
          break;
        case "text-delta":
          setLive((t) => (t ? { ...t, text: t.text + event.delta } : t));
          break;
        case "reasoning-delta":
          setLive((t) => (t ? { ...t, reasoning: t.reasoning + event.delta } : t));
          break;
        case "tool-call":
          setLive((t) =>
            t
              ? {
                  ...t,
                  tools: [...t.tools, { callId: event.callId, name: event.name, args: event.args }],
                }
              : t,
          );
          break;
        case "tool-result":
          setLive((t) =>
            t
              ? {
                  ...t,
                  tools: t.tools.map((tool) =>
                    tool.callId === event.callId ? { ...tool, result: event.result } : tool,
                  ),
                }
              : t,
          );
          break;
        case "tool-approval-required":
          setApprovals((list) => [
            ...list,
            { callId: event.callId, name: event.name, description: event.description },
          ]);
          break;
        case "turn-finished":
          setRunning(false);
          setLive(null);
          setApprovals([]);
          if (activeRef.current) {
            void agentGetMessages(activeRef.current).then(setMessages);
          }
          void refreshSessions();
          break;
        case "error":
          setError(event.message);
          break;
        default:
          break;
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refreshSessions]);

  const onNewSession = async () => {
    if (!model) return;
    try {
      const session = await agentCreateSession(model);
      await refreshSessions();
      await openSession(session);
    } catch (e) {
      setError(String(e));
    }
  };

  const onSend = async () => {
    if (!active || !input.trim()) return;
    const text = input.trim();
    setInput("");
    // /image <prompt>: direct text-to-image without engaging the model.
    if (text.startsWith("/image ")) {
      const prompt = text.slice(7).trim();
      try {
        const result = await agentGenerateImage(prompt);
        setImages((m) => ({ ...m, [result.path]: "" }));
        void loadImage(result.path);
        setMessages((mm) => [
          ...mm,
          {
            id: `local-img-${crypto.randomUUID()}`,
            role: "assistant",
            contentJson: JSON.stringify({ text: `Image saved to ${result.path}`, tools: [] }),
            createdAt: new Date().toISOString(),
          },
        ]);
      } catch (e) {
        setError(String(e));
      }
      return;
    }
    setMessages((m) => [
      ...m,
      {
        id: `local-${crypto.randomUUID()}`,
        role: "user",
        contentJson: JSON.stringify({ text, tools: [] }),
        createdAt: new Date().toISOString(),
      },
    ]);
    try {
      await agentSend(active.id, text);
    } catch (e) {
      setError(String(e));
    }
  };

  const loadImage = async (path: string) => {
    try {
      const b64 = await agentWorkspaceReadB64(path);
      setImages((m) => ({ ...m, [path]: `data:image/png;base64,${b64}` }));
    } catch {
      // leave placeholder
    }
  };

  const decide = (callId: string, decision: string) => {
    if (!active) return;
    setApprovals((list) => list.filter((a) => a.callId !== callId));
    void agentResolveApproval(active.id, callId, decision).catch((e) => setError(String(e)));
  };

  const onBranch = useCallback(
    (messageId: string) => {
      if (!active) return;
      void agentBranchSession(active.id, messageId)
        .then((s) => {
          void refreshSessions();
          return openSession(s);
        })
        .catch((e) => setError(String(e)));
    },
    [active, refreshSessions, openSession],
  );

  const tier = model ? modelPrivacy(model) : null;

  return (
    <div className="screen">
      {/* SESSION PANEL */}
      <div className="panel" style={{ width: 270, flex: "0 0 270px" }}>
        <div className="panel-head">
          <button
            type="button"
            className="btn"
            style={{ width: "100%" }}
            onClick={() => void onNewSession()}
            disabled={!model}
          >
            <PlusIcon /> New chat
          </button>
        </div>
        <div style={{ padding: 12, borderBottom: "1px solid var(--border-subtle)" }}>
          <div className="section-label" style={{ marginBottom: 8, padding: "0 4px" }}>
            Model
          </div>
          <select aria-label="agent model" value={model} onChange={(e) => setModel(e.target.value)}>
            {models.map((m) => (
              <option key={m} value={m}>
                {m} {modelPrivacy(m) === "local" ? "(local, free)" : "(cloud)"}
              </option>
            ))}
          </select>
          {tier ? (
            <div className="hstack" style={{ marginTop: 6, padding: "0 4px", fontSize: 11 }}>
              <span
                className="tier-dot"
                style={{ background: tier === "local" ? "var(--success)" : "var(--warning)" }}
              />
              <span style={{ color: tier === "local" ? "var(--success)" : "var(--warning)" }}>
                {tier === "local" ? "On-device · free · private" : "Cloud"}
              </span>
            </div>
          ) : null}
        </div>
        <div className="panel-body">
          <div className="section-label" style={{ padding: "4px 8px" }}>
            Recent
          </div>
          <ul aria-label="agent sessions" className="plain">
            {sessions.map((session) => (
              <li key={session.id} className="hstack" style={{ gap: 2 }}>
                <button
                  type="button"
                  className="list-item"
                  style={{ flex: 1, minWidth: 0 }}
                  aria-current={active?.id === session.id ? "true" : undefined}
                  onClick={() => void openSession(session)}
                >
                  <div className="item-title">{session.title}</div>
                </button>
                <button
                  type="button"
                  className="btn-icon bare"
                  style={{ color: "var(--text-muted)" }}
                  aria-label={`actions for ${session.title}`}
                  title="Chat actions"
                  onClick={(e) => {
                    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                    openSessionMenu(session.id, r.right - 210, r.bottom + 4);
                  }}
                >
                  <MoreIcon />
                </button>
              </li>
            ))}
          </ul>
        </div>
      </div>

      {/* CONVERSATION PANEL */}
      <div className="panel panel-grow">
        {active ? (
          <div className="panel-head hstack spread">
            <div className="panel-title truncate">{active.title}</div>
            <span className="mono muted" style={{ fontSize: 11 }}>
              {active.model} · {active.mode}
            </span>
          </div>
        ) : null}

        {error ? (
          <div className="banner banner-danger" role="alert" style={{ margin: "12px 22px 0" }}>
            <div className="spread hstack">
              <span>{error}</span>
              <button type="button" className="btn-sm" onClick={() => setError(null)}>
                Dismiss
              </button>
            </div>
          </div>
        ) : null}

        {!active ? (
          <div className="panel-body">
            <div className="empty">
              <AgentIcon className="muted" />
              <p style={{ marginTop: 8 }}>
                Create a session to chat with the agent. Local models are free and private.
              </p>
            </div>
          </div>
        ) : (
          <>
            <div className="panel-body" role="log" aria-label="agent conversation">
              {messages.map((message) => (
                <MessageView
                  key={message.id}
                  message={message}
                  branchable={!message.id.startsWith("local-")}
                  images={images}
                  onBranch={onBranch}
                  onNeedImage={(path) => {
                    if (images[path] === undefined) {
                      setImages((m) => ({ ...m, [path]: "" }));
                      void loadImage(path);
                    }
                  }}
                />
              ))}
              {live ? (
                <div className="msg-assistant" style={{ marginBottom: 22 }}>
                  <div className="agent-avatar">
                    <AgentIcon />
                  </div>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    {live.reasoning ? (
                      <details className="reason" open>
                        <summary>Reasoning</summary>
                        <div className="reason-body" style={{ whiteSpace: "pre-wrap" }}>
                          {live.reasoning}
                        </div>
                      </details>
                    ) : null}
                    {live.tools.length > 0 ? (
                      <div className="stack" style={{ gap: 8, marginBottom: 14 }}>
                        {live.tools.map((tool) => (
                          <div key={tool.callId} className="tool-chip">
                            {tool.result ? (
                              <CheckIcon className="tool-check" />
                            ) : (
                              <span className="spinner" />
                            )}
                            <span className="tool-name">{tool.name}</span>
                            <span className="tool-args">
                              {tool.result ? toolSummary(tool) : "working…"}
                            </span>
                          </div>
                        ))}
                      </div>
                    ) : null}
                    <div style={{ whiteSpace: "pre-wrap", fontSize: 14, lineHeight: 1.65 }}>
                      {live.text}
                      <span className="caret" />
                    </div>
                  </div>
                </div>
              ) : null}

              {approvals.map((approval) => (
                <div
                  key={approval.callId}
                  role="alertdialog"
                  className="approval-card"
                  aria-label={`approve ${approval.name}`}
                >
                  <div className="approval-head">
                    <span className="approval-icon">
                      <FileWriteIcon />
                    </span>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 3 }}>
                        Allow Arya to run {approval.name}?
                      </div>
                      <div
                        style={{ fontSize: 12.5, color: "var(--text-secondary)", lineHeight: 1.5 }}
                      >
                        {approval.description}
                      </div>
                    </div>
                  </div>
                  <div className="approval-actions">
                    <button
                      type="button"
                      className="btn-primary"
                      onClick={() => decide(approval.callId, "once")}
                    >
                      Allow once
                    </button>
                    <button type="button" onClick={() => decide(approval.callId, "session")}>
                      Allow for session
                    </button>
                    <button
                      type="button"
                      className="deny"
                      onClick={() => decide(approval.callId, "deny")}
                    >
                      Deny
                    </button>
                  </div>
                </div>
              ))}
            </div>

            <div style={{ padding: "14px 22px 18px", borderTop: "1px solid var(--border-subtle)" }}>
              {running ? (
                <div className="hstack" style={{ marginBottom: 8 }}>
                  <input
                    aria-label="steer"
                    placeholder="Steer the agent…"
                    value={steerText}
                    onChange={(e) => setSteerText(e.target.value)}
                  />
                  <button
                    type="button"
                    onClick={() => {
                      if (active && steerText.trim()) {
                        void agentSteer(active.id, steerText.trim());
                        setSteerText("");
                      }
                    }}
                  >
                    Steer
                  </button>
                  <button type="button" onClick={() => active && void agentCancel(active.id)}>
                    Stop
                  </button>
                </div>
              ) : null}
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void onSend();
                }}
                className="composer"
              >
                <textarea
                  ref={composerRef}
                  aria-label="agent composer"
                  placeholder="Ask, paste, or tell Arya to do something…"
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  rows={5}
                  style={{ flex: 1 }}
                  onKeyDown={(e) => {
                    // Enter submits; Shift+Enter inserts a newline; Cmd/Ctrl+Enter
                    // also submits, for hands-on-keyboard sending from anywhere in
                    // a multi-line draft.
                    if (e.key !== "Enter") return;
                    if (e.metaKey || e.ctrlKey) {
                      e.preventDefault();
                      void onSend();
                    } else if (!e.shiftKey) {
                      e.preventDefault();
                      void onSend();
                    }
                  }}
                />
                <button
                  type="submit"
                  className="btn-primary btn-icon"
                  aria-label="send"
                  disabled={running || !input.trim()}
                >
                  <SendIcon />
                </button>
              </form>
              <div className="privacy-note">
                {tier === "cloud" ? null : <LockIcon />}
                {tier === "cloud"
                  ? "Cloud model — your prompt is sent to the provider via Arya"
                  : "Running locally — nothing leaves your Mac"}
              </div>
            </div>
          </>
        )}
      </div>

      {notice ? (
        <div
          role="status"
          style={{
            position: "fixed",
            bottom: 16,
            left: "50%",
            transform: "translateX(-50%)",
            zIndex: 950,
            padding: "8px 14px",
            background: "var(--surface-raise)",
            border: "1px solid var(--border)",
            borderRadius: "var(--r-md)",
            boxShadow: "var(--shadow-lg)",
            fontSize: "var(--fs-sm)",
            color: "var(--text)",
          }}
        >
          {notice}
        </div>
      ) : null}

      {sessionMenu ? (
        <div
          ref={sessionMenuRef}
          className="context-menu"
          style={{ top: sessionMenu.y, left: sessionMenu.x }}
          role="menu"
        >
          <button type="button" role="menuitem" onClick={() => convertChatToNote(sessionMenu.id)}>
            Convert to note
          </button>
          <div className="context-menu-label">Translate to</div>
          <div className="context-menu-scroll">
            {TRANSLATE_LANGUAGES.map((l) => (
              <button
                key={l}
                type="button"
                role="menuitem"
                onClick={() => translateChatToNote(sessionMenu.id, l)}
              >
                {l}
              </button>
            ))}
          </div>
          <div className="context-menu-sep" />
          <button
            type="button"
            role="menuitem"
            className="danger"
            onClick={() => {
              const id = sessionMenu.id;
              setSessionMenu(null);
              void agentDeleteSession(id).then(() => {
                if (active?.id === id) {
                  setActive(null);
                  setMessages([]);
                }
                return refreshSessions();
              });
            }}
          >
            Delete
          </button>
        </div>
      ) : null}
    </div>
  );
}
