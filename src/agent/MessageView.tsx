import { memo, useEffect, useMemo } from "react";
import type { AgentMessage, ToolInfo } from "../lib/agent";
import { AgentIcon, CheckIcon } from "../ui/icons";

interface MessageViewProps {
  message: AgentMessage;
  branchable: boolean;
  images: Record<string, string>;
  onBranch: (messageId: string) => void;
  onNeedImage: (path: string) => void;
}

/** Condenses a tool's result (or args) to a short chip label. */
export function toolSummary(tool: ToolInfo): string {
  if (tool.result) return tool.result.replace(/\s+/g, " ").slice(0, 80);
  const args = JSON.stringify(tool.args ?? {});
  return args === "{}" ? "" : args.slice(0, 80);
}

/**
 * A single settled message. Memoized so streaming token deltas (which only
 * change the separate live block) never re-render the whole history.
 * contentJson is parsed once here, not on every parent render.
 */
export const MessageView = memo(function MessageView({
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
