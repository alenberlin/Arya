import { useCallback, useEffect, useRef, useState } from "react";
import { agentListModels } from "../lib/agent";
import {
  type KbMessage,
  type KbSession,
  kbAsk,
  kbCreateSession,
  kbDeleteSession,
  kbGetMessages,
  kbListSessions,
} from "../lib/kb";
import { PlusIcon, SendIcon, TrashIcon } from "../ui/icons";

/**
 * Grounded chat for one collection. Questions are answered only from the
 * collection's documents, with inline `[D#]` citations and a Sources list. Fully
 * on-device: retrieval and the model both run locally.
 */
export function KbChat({
  collectionId,
  embedderAvailable,
  hasReadyDocs,
}: {
  collectionId: string;
  embedderAvailable: boolean | null;
  hasReadyDocs: boolean;
}) {
  const [sessions, setSessions] = useState<KbSession[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [messages, setMessages] = useState<KbMessage[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Mirror the AgentPanel's model list so KB chat picks an Ollama model that
  // actually exists on this Mac, instead of the Rust default fallback
  // (SuperGemma4-26b) which is rarely installed. The user can pick a different
  // model from the dropdown; we keep the choice sticky per-session.
  const [models, setModels] = useState<string[]>([]);
  const [model, setModel] = useState<string>("");
  // `agentListModels` returns qualified names like "ollama:ornith-1.5:35b".
  // kb_ask's Rust handler re-prefixes the model it receives, so we strip the
  // provider prefix before sending — otherwise the request goes out as
  // "ollama:ollama:ornith-…", Ollama 404s, and the UI shows
  // "is Ollama running?" even though Ollama is up.
  const stripProvider = (qualified: string) =>
    qualified.replace(/^ollama:/, "").replace(/^openai:/, "");
  const threadRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const activeRef = useRef<string | null>(null);
  activeRef.current = activeId;

  /**
   * Size the composer to its content, capped by the CSS max-height. Without
   * this the box stays one line tall: even the placeholder is clipped when it
   * wraps in a narrow column, and typed text scrolls inside a 40px slot.
   */
  const fitComposer = useCallback(() => {
    const box = composerRef.current;
    if (!box) return;
    box.style.height = "auto";
    // scrollHeight excludes the border under border-box sizing, and it accounts
    // for the placeholder when the field is empty.
    box.style.height = `${box.scrollHeight + box.offsetHeight - box.clientHeight}px`;
  }, []);

  // input is a trigger, not a read: the height comes from the DOM, not the value.
  // biome-ignore lint/correctness/useExhaustiveDependencies: refit whenever the text changes
  useEffect(fitComposer, [fitComposer, input]);

  // Wrapping changes with the column width — collapsing the documents panel or
  // resizing the window has to re-fit the box. Only width is acted on, or
  // fitting would retrigger the observer through its own height change.
  useEffect(() => {
    const box = composerRef.current;
    if (!box) return;
    let lastWidth = box.clientWidth;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? lastWidth;
      if (width === lastWidth) return;
      lastWidth = width;
      fitComposer();
    });
    observer.observe(box);
    return () => observer.disconnect();
  }, [fitComposer]);

  const refreshSessions = useCallback(async () => {
    const list = await kbListSessions(collectionId);
    setSessions(list);
    return list;
  }, [collectionId]);

  useEffect(() => {
    void refreshSessions()
      .then((list) => setActiveId(list[0]?.id ?? null))
      .catch((e) => setError(String(e)));
  }, [refreshSessions]);

  // Load the agent's model list once so we have an Ollama model that exists.
  // kb_ask's Rust default (SuperGemma4-26b) almost never matches what's
  // installed locally — without this, every chat turns into "is Ollama running?"
  useEffect(() => {
    agentListModels()
      .then((list) => {
        setModels(list);
        setModel((current) => current || list[0] || "");
      })
      .catch(() => {
        // Swallow: an empty list leaves the model as "" and the Rust fallback
        // takes over. The user can still chat; they just won't get the model
        // picker.
      });
  }, []);

  useEffect(() => {
    if (!activeId) {
      setMessages([]);
      return;
    }
    void kbGetMessages(activeId)
      .then(setMessages)
      .catch((e) => setError(String(e)));
  }, [activeId]);

  // Keep the newest message in view. messages/busy are triggers, not reads.
  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll when the thread grows or the thinking row toggles
  useEffect(() => {
    threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight });
  }, [messages, busy]);

  const newChat = async () => {
    try {
      const s = await kbCreateSession(collectionId);
      setSessions((prev) => [s, ...prev]);
      setActiveId(s.id);
      setMessages([]);
    } catch (e) {
      setError(String(e));
    }
  };

  const deleteChat = async (id: string) => {
    try {
      await kbDeleteSession(id);
      const list = await refreshSessions();
      if (activeRef.current === id) setActiveId(list[0]?.id ?? null);
    } catch (e) {
      setError(String(e));
    }
  };

  const send = async () => {
    const q = input.trim();
    if (!q || busy) return;
    setBusy(true);
    setInput("");
    setError(null);
    // Optimistically show the question while the model works.
    const pending: KbMessage = {
      id: "pending-user",
      sessionId: activeId ?? "",
      role: "user",
      content: q,
      citations: [],
      createdAt: "",
    };
    setMessages((prev) => [...prev, pending]);
    try {
      let sid = activeRef.current;
      if (!sid) {
        const s = await kbCreateSession(collectionId);
        sid = s.id;
        setActiveId(s.id);
        setSessions((prev) => [s, ...prev]);
      }
      const answer = await kbAsk(sid, q, model ? stripProvider(model) : undefined);
      setMessages((prev) => [
        ...prev.filter((m) => m.id !== "pending-user"),
        answer.userMessage,
        answer.assistantMessage,
      ]);
      await refreshSessions();
    } catch (e) {
      setError(String(e));
      setMessages((prev) => prev.filter((m) => m.id !== "pending-user"));
      setInput(q);
    } finally {
      setBusy(false);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  };

  const disabledReason =
    embedderAvailable === false
      ? "Start Ollama to chat with this collection."
      : !hasReadyDocs
        ? "Add and index a document first, then ask away."
        : null;

  return (
    <div className="kb-chat">
      <div className="kb-chat-head">
        <select
          aria-label="Chat session"
          className="kb-chat-sessions"
          value={activeId ?? ""}
          onChange={(e) => setActiveId(e.target.value || null)}
        >
          {sessions.length === 0 ? <option value="">New chat</option> : null}
          {sessions.map((s) => (
            <option key={s.id} value={s.id}>
              {s.title}
            </option>
          ))}
        </select>
        {models.length > 1 ? (
          <select
            aria-label="Model"
            className="kb-chat-model"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            title="Model used to answer questions"
          >
            {models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        ) : null}
        <button
          type="button"
          className="btn-icon"
          aria-label="New chat"
          title="New chat"
          onClick={() => void newChat()}
        >
          <PlusIcon />
        </button>
        {activeId ? (
          <button
            type="button"
            className="btn-icon"
            aria-label="Delete chat"
            title="Delete chat"
            onClick={() => void deleteChat(activeId)}
          >
            <TrashIcon />
          </button>
        ) : null}
      </div>

      <div className="kb-chat-thread" ref={threadRef}>
        {messages.length === 0 && !busy ? (
          <div className="kb-chat-empty">
            <h3>Ask your documents</h3>
            <p className="muted">
              Answers come only from this collection, with citations to the exact source.
            </p>
          </div>
        ) : null}
        {messages.map((m) => (
          <div key={m.id} className={`kb-msg ${m.role}`}>
            <div className="kb-msg-body">{m.content}</div>
            {m.citations.length > 0 ? (
              <div className="kb-sources">
                <div className="kb-sources-label">Sources</div>
                {m.citations.map((c) => (
                  <div key={c.key} className="kb-source">
                    <span className="kb-source-ref">{c.key}</span>
                    <span className="kb-source-doc">
                      {c.filename}
                      {c.page ? ` · p.${c.page}` : ""}
                    </span>
                    <span className="kb-source-quote">{c.quote}</span>
                  </div>
                ))}
              </div>
            ) : null}
          </div>
        ))}
        {busy ? (
          <div className="kb-msg assistant">
            <div className="kb-msg-body kb-thinking">Searching your documents…</div>
          </div>
        ) : null}
      </div>

      {error ? (
        <p role="alert" className="kb-chat-error">
          {error}
        </p>
      ) : null}

      <div className="kb-chat-composer">
        <textarea
          ref={composerRef}
          aria-label="Ask a question"
          placeholder={disabledReason ?? "Ask a question about this collection…"}
          value={input}
          rows={1}
          disabled={busy}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <button
          type="button"
          className="btn-icon btn-send"
          aria-label="Send"
          disabled={busy || !input.trim()}
          onClick={() => void send()}
        >
          <SendIcon />
        </button>
      </div>
    </div>
  );
}
