import { invoke } from "@tauri-apps/api/core";
import type { AgentSession } from "./agent";

export interface McpServer {
  id: string;
  name: string;
  command: string;
  argsJson: string;
  envJson: string;
  enabled: number;
  createdAt: string;
}

export interface Routine {
  id: string;
  title: string;
  prompt: string;
  model: string;
  mode: string;
  intervalMinutes: number;
  enabled: number;
  lastRunAt: string | null;
  nextRunAt: string;
  createdAt: string;
}

export interface RoutineRun {
  id: string;
  sessionId: string | null;
  status: string;
  detail: string | null;
  startedAt: string;
}

export interface WorkspaceEntry {
  name: string;
  isDir: boolean;
  size: number;
}

export const mcpListServers = () => invoke<McpServer[]>("mcp_list_servers");
export const mcpAddServer = (
  name: string,
  command: string,
  args: string[],
  env: Record<string, string>,
) => invoke<string[]>("mcp_add_server", { name, command, args, env });
export const mcpRemoveServer = (id: string) => invoke<void>("mcp_remove_server", { id });

export const routineList = () => invoke<Routine[]>("routine_list");
export const routineCreate = (
  title: string,
  prompt: string,
  model: string,
  intervalMinutes: number,
) => invoke<Routine>("routine_create", { title, prompt, model, intervalMinutes });
export const routineSetEnabled = (id: string, enabled: boolean) =>
  invoke<void>("routine_set_enabled", { id, enabled });
export const routineDelete = (id: string) => invoke<void>("routine_delete", { id });
export const routineRuns = (routineId: string) =>
  invoke<RoutineRun[]>("routine_runs", { routineId });

export const agentBranchSession = (sessionId: string, throughMessageId: string) =>
  invoke<AgentSession>("agent_branch_session", { sessionId, throughMessageId });

export const agentWorkspaceList = (subPath?: string) =>
  invoke<WorkspaceEntry[]>("agent_workspace_list", { subPath: subPath ?? null });
export const agentWorkspaceRead = (path: string) =>
  invoke<string>("agent_workspace_read", { path });
