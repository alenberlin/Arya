import { useCallback, useEffect, useState } from "react";
import { agentListModels } from "../lib/agent";
import {
  type Routine,
  type RoutineRun,
  routineCreate,
  routineDelete,
  routineList,
  routineRuns,
  routineSetEnabled,
} from "../lib/ecosystem";
import { RoutinesIcon } from "../ui/icons";

/** Scheduled agent routines with run history. */
export function RoutinesPanel() {
  const [routines, setRoutines] = useState<Routine[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [title, setTitle] = useState("");
  const [prompt, setPrompt] = useState("");
  const [model, setModel] = useState("");
  const [interval, setIntervalMin] = useState(60);
  const [runsByRoutine, setRunsByRoutine] = useState<Record<string, RoutineRun[]>>({});
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setRoutines(await routineList());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    void agentListModels()
      .then((list) => {
        setModels(list);
        setModel((m) => m || list[0] || "");
      })
      .catch(() => {});
  }, [refresh]);

  const loadRuns = async (id: string) => {
    try {
      setRunsByRoutine((prev) => ({ ...prev, [id]: [] }));
      const runs = await routineRuns(id);
      setRunsByRoutine((prev) => ({ ...prev, [id]: runs }));
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="screen-center">
      <div className="screen-col">
        <div className="screen-head">
          <div>
            <h1>Routines</h1>
            <p>Let the agent run a prompt on a schedule. Local models run for free.</p>
          </div>
        </div>

        {error ? (
          <p role="alert" style={{ marginBottom: 12 }}>
            {error}
          </p>
        ) : null}

        <form
          className="card"
          onSubmit={(e) => {
            e.preventDefault();
            if (!title.trim() || !prompt.trim() || !model) return;
            void routineCreate(title.trim(), prompt.trim(), model, interval)
              .then(() => {
                setTitle("");
                setPrompt("");
                return refresh();
              })
              .catch((err) => setError(String(err)));
          }}
          style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 16 }}
        >
          <input
            placeholder="Title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            aria-label="routine title"
          />
          <textarea
            placeholder="Prompt the agent should run"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            aria-label="routine prompt"
            rows={2}
          />
          <div className="hstack" style={{ gap: 10 }}>
            <select
              value={model}
              onChange={(e) => setModel(e.target.value)}
              aria-label="routine model"
              style={{ width: "auto" }}
            >
              {models.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
            <label className="hstack" style={{ gap: 6, color: "var(--text-secondary)" }}>
              every
              <input
                type="number"
                min={1}
                value={interval}
                onChange={(e) => setIntervalMin(Math.max(1, Number(e.target.value)))}
                aria-label="routine interval minutes"
                style={{ width: 70 }}
              />
              min
            </label>
            <button type="submit" className="btn-primary" style={{ marginLeft: "auto" }}>
              Add routine
            </button>
          </div>
        </form>

        <ul aria-label="routines" className="plain stack">
          {routines.map((routine) => {
            const enabled = routine.enabled !== 0;
            const runs = runsByRoutine[routine.id];
            return (
              <li key={routine.id} className="routine-card">
                <div
                  className="spread hstack"
                  style={{ marginBottom: 10, alignItems: "flex-start" }}
                >
                  <div className="hstack" style={{ gap: 12, minWidth: 0 }}>
                    <div className={`routine-icon${enabled ? "" : " off"}`}>
                      <RoutinesIcon />
                    </div>
                    <div style={{ minWidth: 0 }}>
                      <div style={{ fontSize: 15, fontWeight: 600 }}>{routine.title}</div>
                      <div className="muted" style={{ fontSize: 12.5 }}>
                        "{routine.prompt}"
                      </div>
                    </div>
                  </div>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={enabled}
                    aria-label={`${enabled ? "Pause" : "Resume"} ${routine.title}`}
                    className="switch bare"
                    onClick={() =>
                      void routineSetEnabled(routine.id, routine.enabled === 0).then(refresh)
                    }
                  />
                </div>
                <div className="routine-meta">
                  <span>every {routine.intervalMinutes} min</span>
                  <span>{enabled ? "enabled" : "paused"}</span>
                  <span style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                    <button
                      type="button"
                      className="btn-sm btn-ghost"
                      onClick={() => void loadRuns(routine.id)}
                    >
                      History
                    </button>
                    <button
                      type="button"
                      className="btn-sm btn-ghost"
                      onClick={() => void routineDelete(routine.id).then(refresh)}
                    >
                      Delete
                    </button>
                  </span>
                </div>
                {runs ? (
                  <ul
                    aria-label={`runs for ${routine.title}`}
                    className="plain"
                    style={{ marginTop: 10 }}
                  >
                    {runs.map((run) => (
                      <li
                        key={run.id}
                        className="mono muted"
                        style={{ fontSize: 11, padding: "2px 0" }}
                      >
                        {run.startedAt} · {run.status}
                        {run.detail ? ` · ${run.detail}` : ""}
                      </li>
                    ))}
                    {runs.length === 0 ? (
                      <li className="muted" style={{ fontSize: 12 }}>
                        No runs yet.
                      </li>
                    ) : null}
                  </ul>
                ) : null}
              </li>
            );
          })}
          {routines.length === 0 ? (
            <li className="empty">
              No routines yet. Add one above to have the agent run on a schedule.
            </li>
          ) : null}
        </ul>
      </div>
    </div>
  );
}
