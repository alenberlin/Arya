import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type AgentEvent,
  type AgentMessage,
  type AgentSession,
  agentCancel,
  agentCreateSession,
  agentDeleteSession,
  agentGetMessages,
  agentListModels,
  agentListSessions,
  agentResolveApproval,
  agentSend,
  agentSteer,
  modelPrivacy,
  type ToolInfo,
} from "../lib/agent";
import { agentBranchSession } from "../lib/ecosystem";
import { AgentIcon, CheckIcon, FileWriteIcon, LockIcon, PlusIcon, SendIcon } from "../ui/icons";

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

interface MessageViewProps {
  message: AgentMessage;
  branchable: boolean;
  images: Record<string, string>;
  onBranch: (messageId: string) => void;
  onNeedImage: (path: string) => void;
}

function toolSummary(tool: ToolInfo): string {
  if (tool.result) return tool.result.replace(/\s+/g, " ").slice(0, 80);
  const args = JSON.stringify(tool.args ?? {});
  return args === "{}" ? "" : args.slice(0, 80);
}

/**
 * A single settled message. Memoized so streaming token deltas (which only
 * change the separate live block) never re-render the whole history.
 * contentJson is parsed once here, not on every parent render.
 */
const MessageView = memo(function MessageView({
  message,
  branchable,
  images,
  onBranch,
  onNeedImage,
}: MessageViewProps) {
  const content = useMemo<{ text?: string; reasoning?: string | null; tools?: ToolInfo[] }>(() => {
    try {
      return JSON.parse(message.contentJson);
    } catch {
      return { text: message.contentJson };
    }
  }, [message.contentJson]);

  const imagePaths = useMemo(
    () =>
      (content.tools ?? [])
        .map((tool) => tool.result?.match(/images\/[\w.-]+\.png/)?.[0])
        .filter((p): p is string => Boolean(p)),
    [content.tools],
  );
  useEffect(() => {
    for (const path of imagePaths) onNeedImage(path);
  }, [imagePaths, onNeedImage]);

  if (message.role === "user") {
    return (
      <div style={{ marginBottom: 22 }}>
        <div className="msg-user">{content.text}</div>
      </div>
    );
  }

  return (
    <div className="msg-assistant" style={{ marginBottom: 22 }}>
      <div className="agent-avatar">
        <AgentIcon />
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        {content.reasoning ? (
          <details className="reason">
            <summary>Reasoning</summary>
            <div className="reason-body" style={{ whiteSpace: "pre-wrap" }}>
              {content.reasoning}
            </div>
          </details>
        ) : null}
        {(content.tools ?? []).length > 0 ? (
          <div className="stack" style={{ gap: 8, marginBottom: 14 }}>
            {(content.tools ?? []).map((tool) => {
              const match = tool.result?.match(/images\/[\w.-]+\.png/)?.[0];
              return (
                <div key={tool.callId}>
                  <div className="tool-chip">
                    <CheckIcon className="tool-check" />
                    <span className="tool-name">{tool.name}</span>
                    <span className="tool-args">{toolSummary(tool)}</span>
                  </div>
                  {match && images[match] ? (
                    <img
                      src={images[match]}
                      alt={`generated ${match}`}
                      style={{ display: "block", maxWidth: 360, marginTop: 6, borderRadius: 10 }}
                    />
                  ) : null}
                </div>
              );
            })}
          </div>
        ) : null}
        {content.text ? (
          <div style={{ whiteSpace: "pre-wrap", fontSize: 14, lineHeight: 1.65 }}>
            {content.text}
          </div>
        ) : null}
        {branchable ? (
          <button
            type="button"
            className="btn-ghost btn-sm"
            style={{ marginTop: 6, color: "var(--text-muted)" }}
            onClick={() => onBranch(message.id)}
          >
            Branch here
          </button>
        ) : null}
      </div>
    </div>
  );
});

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
  const activeRef = useRef<string | null>(null);
  activeRef.current = active?.id ?? null;

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
        const result = await invoke<{ path: string }>("agent_generate_image", {
          prompt,
          size: null,
        });
        setImages((m) => ({ ...m, [result.path]: "" }));
        void loadImage(result.path);
        setMessages((mm) => [
          ...mm,
          {
            id: `local-img-${Date.now()}`,
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
        id: `local-${Date.now()}`,
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
      const b64 = await invoke<string>("agent_workspace_read_b64", { path });
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
                  aria-label={`delete ${session.title}`}
                  onClick={() =>
                    void agentDeleteSession(session.id).then(() => {
                      if (active?.id === session.id) {
                        setActive(null);
                        setMessages([]);
                      }
                      return refreshSessions();
                    })
                  }
                >
                  ×
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
                  aria-label="agent composer"
                  placeholder="Ask, or tell Arya to do something…"
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  rows={1}
                  style={{ flex: 1 }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
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
    </div>
  );
}
