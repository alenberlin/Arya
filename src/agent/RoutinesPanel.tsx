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
    <section>
      <h2>Routines</h2>
      <p>
        <small>Scheduled agent tasks. Local models run for free.</small>
      </p>
      {error ? <p role="alert">{error}</p> : null}
      <form
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
        style={{ display: "flex", flexDirection: "column", gap: 6, maxWidth: 520 }}
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
        <div style={{ display: "flex", gap: 6 }}>
          <select
            value={model}
            onChange={(e) => setModel(e.target.value)}
            aria-label="routine model"
          >
            {models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          <label>
            every{" "}
            <input
              type="number"
              min={1}
              value={interval}
              onChange={(e) => setIntervalMin(Math.max(1, Number(e.target.value)))}
              aria-label="routine interval minutes"
              style={{ width: 70 }}
            />{" "}
            min
          </label>
          <button type="submit">Add routine</button>
        </div>
      </form>

      <ul aria-label="routines" style={{ listStyle: "none", padding: 0 }}>
        {routines.map((routine) => (
          <li key={routine.id} style={{ borderTop: "1px solid var(--border)", padding: "8px 0" }}>
            <strong>{routine.title}</strong> — every {routine.intervalMinutes} min ·{" "}
            {routine.enabled ? "enabled" : "paused"}
            <div style={{ fontSize: 12, color: "var(--text-secondary)" }}>{routine.prompt}</div>
            <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
              <button
                type="button"
                onClick={() =>
                  void routineSetEnabled(routine.id, routine.enabled === 0).then(refresh)
                }
              >
                {routine.enabled ? "Pause" : "Resume"}
              </button>
              <button type="button" onClick={() => void loadRuns(routine.id)}>
                History
              </button>
              <button type="button" onClick={() => void routineDelete(routine.id).then(refresh)}>
                Delete
              </button>
            </div>
            {runsByRoutine[routine.id] ? (
              <ul aria-label={`runs for ${routine.title}`}>
                {runsByRoutine[routine.id].map((run) => (
                  <li key={run.id}>
                    <small>
                      {run.startedAt} · {run.status}
                      {run.detail ? ` · ${run.detail}` : ""}
                    </small>
                  </li>
                ))}
                {runsByRoutine[routine.id].length === 0 ? <li>No runs yet.</li> : null}
              </ul>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}
