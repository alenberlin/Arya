import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
  /** Stable key the parent uses so a reset remounts the pane. */
  paneKey: string;
  /** Human-readable name shown in the fallback. */
  paneName: string;
}

interface State {
  error: Error | null;
}

/**
 * Per-pane error boundary. A crash inside one workspace (a bad JSON.parse in a
 * note, a GraphQL-shaped render error in Galaxy) used to blank the whole
 * window because the shell only had the root boundary. With one of these per
 * panel, the other panes stay interactive and the user can recover without a
 * full reload.
 */
export class PaneErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // Surface to devtools; the desktop shell has no crash reporter today.
    console.error(`Pane "${this.props.paneName}" crashed:`, error, info.componentStack);
  }

  reset = (): void => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    if (this.state.error) {
      return (
        <div className="screen-center">
          <div className="screen-col" style={{ maxWidth: 480 }}>
            <h1 className="hero-title">{this.props.paneName} hit a snag</h1>
            <p className="muted">
              This pane crashed, but the rest of Arya is still working. Your notes and data are safe
              on disk.
            </p>
            <pre
              style={{
                maxWidth: 480,
                margin: "12px 0",
                padding: 12,
                background: "var(--card-sunken)",
                borderRadius: 8,
                overflow: "auto",
                textAlign: "left",
                fontSize: 12,
                color: "var(--text-muted)",
                whiteSpace: "pre-wrap",
              }}
            >
              {this.state.error.message}
            </pre>
            <div className="hstack" style={{ gap: 8 }}>
              <button
                type="button"
                className="btn-primary"
                onClick={this.reset}
                aria-label={`recover ${this.props.paneName}`}
              >
                Recover this pane
              </button>
            </div>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
