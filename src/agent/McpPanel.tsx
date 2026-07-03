import { useCallback, useEffect, useState } from "react";
import { type McpServer, mcpAddServer, mcpListServers, mcpRemoveServer } from "../lib/ecosystem";

/** MCP server management: add stdio servers, view their tools, remove them. */
export function McpPanel() {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [lastTools, setLastTools] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setServers(await mcpListServers());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <section>
      <h2>MCP servers</h2>
      <p>
        <small>
          Connect external tool providers (Model Context Protocol). Their tools appear to the agent
          and always ask before running.
        </small>
      </p>
      {error ? <p role="alert">{error}</p> : null}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (!name.trim() || !command.trim()) return;
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
        }}
        style={{ display: "flex", flexDirection: "column", gap: 6, maxWidth: 520 }}
      >
        <input
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
        <button type="submit">Add server</button>
      </form>
      {lastTools.length > 0 ? (
        <p>
          Connected. Tools: <code>{lastTools.join(", ")}</code>
        </p>
      ) : null}
      <ul aria-label="mcp servers" style={{ listStyle: "none", padding: 0 }}>
        {servers.map((server) => (
          <li key={server.id} style={{ padding: "6px 0" }}>
            <strong>{server.name}</strong> <code>{server.command}</code>{" "}
            <button type="button" onClick={() => void mcpRemoveServer(server.id).then(refresh)}>
              Remove
            </button>
          </li>
        ))}
        {servers.length === 0 ? <li>No MCP servers configured.</li> : null}
      </ul>
    </section>
  );
}
