import { useCallback, useEffect, useRef, useState } from "react";
import { type McpServer, mcpAddServer, mcpListServers, mcpRemoveServer } from "../lib/ecosystem";
import { ConfirmDialog } from "../ui/dialogs";
import { McpIcon, PlusIcon } from "../ui/icons";

/** MCP server management: add stdio servers, view their tools, remove them. */
export function McpPanel() {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [lastTools, setLastTools] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [confirmAddOpen, setConfirmAddOpen] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  const refresh = useCallback(async () => {
    try {
      setServers(await mcpListServers());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // Adding a server launches an external process that gains tool access to the
  // agent, so it runs only after an explicit in-app confirmation.
  const doAddServer = () => {
    setConfirmAddOpen(false);
    const argv = args.trim() ? args.trim().split(/\s+/) : [];
    void mcpAddServer(name.trim(), command.trim(), argv, {})
      .then((tools) => {
        setLastTools(tools);
        setName("");
        setCommand("");
        setArgs("");
        return refresh();
      })
      .catch((err) => setError(String(err)));
  };

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="screen-center">
      <div className="screen-col">
        <div className="screen-head">
          <h1>MCP servers</h1>
          <button type="button" onClick={() => nameRef.current?.focus()}>
            <PlusIcon /> Add server
          </button>
        </div>
        <p className="muted" style={{ margin: "0 0 20px" }}>
          Connect external tools that extend what the agent can do. Their tools appear to the agent
          and always ask before running.
        </p>

        {error ? (
          <p role="alert" style={{ marginBottom: 12 }}>
            {error}
          </p>
        ) : null}

        <form
          className="card"
          onSubmit={(e) => {
            e.preventDefault();
            if (!name.trim() || !command.trim()) return;
            setConfirmAddOpen(true);
          }}
          style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 16 }}
        >
          <input
            ref={nameRef}
            placeholder="name (e.g. filesystem)"
            value={name}
            onChange={(e) => setName(e.target.value)}
            aria-label="mcp name"
          />
          <input
            placeholder="command (e.g. npx)"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            aria-label="mcp command"
          />
          <input
            placeholder="args (space separated)"
            value={args}
            onChange={(e) => setArgs(e.target.value)}
            aria-label="mcp args"
          />
          <button type="submit" className="btn-primary" style={{ alignSelf: "flex-start" }}>
            Add server
          </button>
        </form>

        {lastTools.length > 0 ? (
          <p className="muted" style={{ fontSize: 13, marginBottom: 12 }}>
            Connected. Tools: <code>{lastTools.join(", ")}</code>
          </p>
        ) : null}

        <ul aria-label="mcp servers" className="plain mcp-grid">
          {servers.map((server) => (
            <li key={server.id} className="mcp-card">
              <div className="hstack" style={{ marginBottom: 10 }}>
                <div className="mcp-icon">
                  <McpIcon />
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 14, fontWeight: 600 }} className="truncate">
                    {server.name}
                  </div>
                </div>
                {/* Reflects the stored enabled flag, not a live process probe —
                    "Connected" was hardcoded and lied about disabled/crashed
                    servers. A real status probe would need a backend command. */}
                {server.enabled !== 0 ? (
                  <span
                    className="hstack mono"
                    style={{ fontSize: 11, color: "var(--success)", gap: 5 }}
                  >
                    <span className="tier-dot" style={{ background: "var(--success)" }} />
                    Enabled
                  </span>
                ) : (
                  <span
                    className="hstack mono"
                    style={{ fontSize: 11, color: "var(--text-muted)", gap: 5 }}
                  >
                    <span className="tier-dot" style={{ background: "var(--text-muted)" }} />
                    Disabled
                  </span>
                )}
              </div>
              <div className="muted" style={{ fontSize: 12.5, marginBottom: 10 }}>
                <code>{server.command}</code>
              </div>
              <button
                type="button"
                className="btn-sm btn-danger"
                onClick={() => void mcpRemoveServer(server.id).then(refresh)}
              >
                Remove
              </button>
            </li>
          ))}
          <li>
            <button type="button" className="mcp-add bare" onClick={() => nameRef.current?.focus()}>
              <PlusIcon /> Add a server
            </button>
          </li>
        </ul>
      </div>

      <ConfirmDialog
        open={confirmAddOpen}
        title={`Add "${name.trim()}"?`}
        message={`Arya will launch: ${command.trim()}${args.trim() ? ` ${args.trim()}` : ""}. Only add servers you trust — its tools can act on your behalf.`}
        confirmLabel="Add & launch"
        onConfirm={doAddServer}
        onCancel={() => setConfirmAddOpen(false)}
      />
    </div>
  );
}
