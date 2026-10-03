export interface Project {
  id: string;
  name: string;
  rootPath: string;
  canonicalPath: string;
  previousRoots: string[];
  notes: string;
  autoStart: boolean;
  createdAt: string;
  updatedAt: string;
}
export interface RuntimeSnapshot {
  projectId: string;
  generation: string;
  sequence: number;
  timestamp: string;
  state: "stopped" | "starting" | "running" | "stopping" | "error";
  pid?: number | null;
  port?: number | null;
  startedAt?: string | null;
  sessions: number;
  indexState: "unknown" | "missing" | "ready" | "indexing" | "error";
  indexStats: {
    fileCount: number | null;
    nodeCount: number | null;
    edgeCount: number | null;
    checkedAt: string;
  } | null;
  error?: { code: string; message: string; retryable: boolean } | null;
}
export interface Settings {
  codegraphEntry: string | null;
  indexConcurrency: number;
  closeBehavior: "tray" | "exit";
  appDataDir: string;
}
export interface Environment {
  available: boolean;
  entry: string | null;
  version: string | null;
  error: string | null;
}
export interface LogEntry {
  projectId: string;
  generation: string;
  sequence: number;
  timestamp: string;
  level: string;
  stage: string;
  message: string;
}
export interface TaskProgress {
  startedAt: string;
  operationId: string;
  projectId: string;
  generation: string;
  sequence: number;
  timestamp: string;
  kind: string;
  state: "running" | "completed" | "failed" | "cancelled";
  message: string;
  error?: { code: string; message: string };
}
export interface ConfigPreview {
  previewId: string;
  projectId: string;
  serviceName: string;
  files: {
    client: string;
    path: string;
    before: string;
    after: string;
    existed: boolean;
    conflict: boolean;
  }[];
}
export interface ConfigResult {
  operationId: string;
  backupPath: string;
  files: { path: string; status: string; message: string }[];
}
export interface ConfigStatus {
  client: Client;
  path: string;
  state: "missing" | "configured" | "repair" | "parseError";
  message: string | null;
}
export type Client = "codex" | "claude";
