import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
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

  const renderMessage = (message: AgentMessage) => {
    let content: { text?: string; reasoning?: string | null; tools?: ToolInfo[] } = {};
    try {
      content = JSON.parse(message.contentJson);
    } catch {
      content = { text: message.contentJson };
    }
    return (
      <article
        key={message.id}
        style={{
          margin: "8px 0",
          padding: 8,
          borderRadius: 8,
          background: message.role === "user" ? "#eef2ff" : "#f8fafc",
        }}
      >
        <small>{message.role === "user" ? "You" : "Arya"}</small>{" "}
        {active && !message.id.startsWith("local-") ? (
          <button
            type="button"
            style={{ fontSize: 11 }}
            onClick={() =>
              void agentBranchSession(active.id, message.id)
                .then((s) => {
                  void refreshSessions();
                  return openSession(s);
                })
                .catch((e) => setError(String(e)))
            }
          >
            Branch here
          </button>
        ) : null}
        {content.reasoning ? (
          <details>
            <summary>Reasoning</summary>
            <pre style={{ whiteSpace: "pre-wrap" }}>{content.reasoning}</pre>
          </details>
        ) : null}
        {(content.tools ?? []).map((tool) => {
          const match = tool.result?.match(/images\/[\w.-]+\.png/);
          if (match && images[match[0]] === undefined) {
            setImages((m) => ({ ...m, [match[0]]: "" }));
            void loadImage(match[0]);
          }
          return (
            <div key={tool.callId} style={{ fontFamily: "monospace", fontSize: 12 }}>
              ⚙ {tool.name}({JSON.stringify(tool.args)})
              {tool.result ? ` → ${tool.result.slice(0, 120)}` : ""}
              {match && images[match[0]] ? (
                <img
                  src={images[match[0]]}
                  alt={`generated ${match[0]}`}
                  style={{ display: "block", maxWidth: 360, marginTop: 4 }}
                />
              ) : null}
            </div>
          );
        })}
        <div style={{ whiteSpace: "pre-wrap" }}>{content.text}</div>
      </article>
    );
  };

  return (
    <div style={{ display: "flex", gap: 16, alignItems: "flex-start" }}>
      <aside style={{ width: 230, flexShrink: 0 }}>
        <div style={{ display: "flex", gap: 6, flexDirection: "column" }}>
          <select aria-label="agent model" value={model} onChange={(e) => setModel(e.target.value)}>
            {models.map((m) => (
              <option key={m} value={m}>
                {m} {modelPrivacy(m) === "local" ? "(local, free)" : "(cloud)"}
              </option>
            ))}
          </select>
          <button type="button" onClick={() => void onNewSession()} disabled={!model}>
            New session
          </button>
        </div>
        <ul aria-label="agent sessions" style={{ listStyle: "none", padding: 0 }}>
          {sessions.map((session) => (
            <li key={session.id} style={{ marginTop: 6 }}>
              <button type="button" onClick={() => void openSession(session)}>
                {session.title}
              </button>
              <button
                type="button"
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
      </aside>

      <section style={{ flex: 1, minWidth: 0 }}>
        {error ? (
          <p role="alert">
            {error}{" "}
            <button type="button" onClick={() => setError(null)}>
              Dismiss
            </button>
          </p>
        ) : null}
        {!active ? (
          <p>Create a session to chat with the agent. Local models are free and private.</p>
        ) : (
          <>
            <p>
              <strong>{active.title}</strong> · {active.model} · {active.mode}
            </p>
            <div role="log" aria-label="agent conversation">
              {messages.map(renderMessage)}
              {live ? (
                <article style={{ margin: "8px 0", padding: 8, background: "#f0fdf4" }}>
                  <small>Arya (working…)</small>
                  {live.reasoning ? (
                    <details open>
                      <summary>Reasoning</summary>
                      <pre style={{ whiteSpace: "pre-wrap" }}>{live.reasoning}</pre>
                    </details>
                  ) : null}
                  {live.tools.map((tool) => (
                    <div key={tool.callId} style={{ fontFamily: "monospace", fontSize: 12 }}>
                      ⚙ {tool.name}({JSON.stringify(tool.args)})
                      {tool.result ? ` → ${tool.result.slice(0, 120)}` : " …"}
                    </div>
                  ))}
                  <div style={{ whiteSpace: "pre-wrap" }}>{live.text}</div>
                </article>
              ) : null}
            </div>

            {approvals.map((approval) => (
              <div
                key={approval.callId}
                role="alertdialog"
                aria-label={`approve ${approval.name}`}
                style={{ padding: 8, background: "#fef3c7", margin: "8px 0" }}
              >
                <strong>Approval needed:</strong> {approval.description}
                <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
                  <button type="button" onClick={() => decide(approval.callId, "once")}>
                    Allow once
                  </button>
                  <button type="button" onClick={() => decide(approval.callId, "session")}>
                    Allow for session
                  </button>
                  <button type="button" onClick={() => decide(approval.callId, "deny")}>
                    Deny
                  </button>
                </div>
              </div>
            ))}

            {running ? (
              <div style={{ display: "flex", gap: 6, margin: "8px 0" }}>
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
              style={{ display: "flex", gap: 6, marginTop: 8 }}
            >
              <textarea
                aria-label="agent composer"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                rows={3}
                style={{ flex: 1 }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void onSend();
                  }
                }}
              />
              <button type="submit" disabled={running || !input.trim()}>
                Send
              </button>
            </form>
          </>
        )}
      </section>
    </div>
  );
}
