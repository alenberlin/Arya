import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}
interface State {
  error: Error | null;
}

/**
 * Catches render/lifecycle errors from the tree below it so a single thrown
 * component (a bad JSON.parse, an undefined access) can't blank the whole
 * window with no recovery. Shows a minimal fallback with a reload action.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // The desktop shell has no crash reporter; surface it to the console so a
    // dev build (or `tauri dev` stderr) still captures the stack.
    console.error("Arya UI crashed:", error, info.componentStack);
  }

  render(): ReactNode {
    if (this.state.error) {
      return (
        <main style={{ fontFamily: "system-ui", padding: "3rem", textAlign: "center" }}>
          <h1>Something went wrong</h1>
          <p style={{ color: "var(--text-secondary)" }}>
            Arya hit an unexpected error. Your notes and data are safe on disk.
          </p>
          <pre
            style={{
              maxWidth: 640,
              margin: "1rem auto",
              overflow: "auto",
              textAlign: "left",
              fontSize: 12,
              color: "var(--text-muted)",
              whiteSpace: "pre-wrap",
            }}
          >
            {this.state.error.message}
          </pre>
          <button type="button" onClick={() => window.location.reload()}>
            Reload
          </button>
        </main>
      );
    }
    return this.props.children;
  }
}
