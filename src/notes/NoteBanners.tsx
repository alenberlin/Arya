import type { RecoverableRecording } from "../lib/notes";

interface Props {
  upcoming: { title: string; startsInMin: number } | null;
  meeting: { appName: string } | null;
  systemAudioWarning: string | null;
  recoverables: RecoverableRecording[];
  recording: boolean;
  onRecordMeeting: () => void;
  onDismissUpcoming: () => void;
  onDismissMeeting: () => void;
  onRecover: (sessionId: string) => void;
  onDiscard: (sessionId: string) => void;
}

/** The stack of contextual banners atop the notes list: an upcoming calendar
 * event, a detected meeting, a system-audio warning, and interrupted-recording
 * recovery. Pure presentation — the parent owns the state and side effects. */
export function NoteBanners({
  upcoming,
  meeting,
  systemAudioWarning,
  recoverables,
  recording,
  onRecordMeeting,
  onDismissUpcoming,
  onDismissMeeting,
  onRecover,
  onDiscard,
}: Props) {
  return (
    <>
      {upcoming && !recording ? (
        <div className="banner banner-accent" role="status" style={{ margin: "6px 6px 12px" }}>
          <span>
            <strong>{upcoming.title}</strong> starts in {Math.max(0, upcoming.startsInMin)} min.
          </span>
          <div className="hstack">
            <button type="button" className="btn-sm btn-primary" onClick={onRecordMeeting}>
              Record it
            </button>
            <button type="button" className="btn-sm" onClick={onDismissUpcoming}>
              Dismiss
            </button>
          </div>
        </div>
      ) : null}

      {meeting && !recording ? (
        <div className="banner banner-accent" role="status" style={{ margin: "6px 6px 12px" }}>
          <div className="hstack">
            <span className="dot-pulse" />
            <span className="banner-title">Meeting detected in {meeting.appName}</span>
          </div>
          <div className="hstack">
            <button type="button" className="btn-sm btn-primary" onClick={onRecordMeeting}>
              Record
            </button>
            <button type="button" className="btn-sm" onClick={onDismissMeeting}>
              Dismiss
            </button>
          </div>
        </div>
      ) : null}

      {systemAudioWarning ? (
        <div className="banner banner-warning" role="alert" style={{ margin: "6px 6px 12px" }}>
          <span className="banner-title">System audio unavailable</span>
          <small>
            Recording microphone only. Grant "System audio recording" in System Settings for meeting
            capture.
          </small>
        </div>
      ) : null}

      {recoverables.length > 0 ? (
        <div className="banner banner-warning" role="alert" style={{ margin: "6px 6px 12px" }}>
          <span className="banner-title">Interrupted recording found</span>
          {recoverables.map((r) => (
            <div key={r.sessionId} className="hstack spread">
              <small>
                {r.noteTitle} · {Math.round(r.sizeBytes / 1024)} KB
              </small>
              <div className="hstack">
                <button
                  type="button"
                  className="btn-sm btn-primary"
                  onClick={() => onRecover(r.sessionId)}
                >
                  Recover
                </button>
                <button type="button" className="btn-sm" onClick={() => onDiscard(r.sessionId)}>
                  Discard
                </button>
              </div>
            </div>
          ))}
        </div>
      ) : null}
    </>
  );
}
