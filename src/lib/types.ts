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
  language: "en" | "zh-CN";
  codegraphEntry: string | null;
  serenaEntry?: string | null;
  morphKeyHint?: string | null;
  morphKeyValidatedAt?: string | null;
  indexConcurrency: number;
  closeBehavior: "tray" | "exit";
  activeClient?: Client;
  appDataDir: string;
}
export interface Activation {
  /** Clients whose configuration changed. */
  clients: Client[];
  /** Whether the engine runs afterwards. */
  running: boolean;
  warnings: string[];
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
export type Client = "codex" | "claude" | "cursor" | "devin";
export type Engine = "codegraph" | "serena" | "warpgrep" | "rea";
export interface SerenaSnapshot {
  state: RuntimeSnapshot["state"];
  pid: number | null;
  endpoint: string | null;
  startedAt: string | null;
  tools: string[];
  error: { code: string; message: string } | null;
}
export type WarpGrepSnapshot = SerenaSnapshot;
export type ReaEnvironment = Record<string, string>;
export interface ReaEnvironmentSaved {
  environment: ReaEnvironment;
  updated: string[];
  failed: string[];
}
export interface ReaCheck {
  name: string;
  ok: boolean;
  detail: string | null;
  remediation: string | null;
}
export interface EngineToolLists {
  known: Partial<Record<Engine, string[]>>;
  disabled: Partial<Record<Engine, string[]>>;
}
export interface DisabledToolsSaved {
  disabled: string[];
  updated: string[];
  failed: string[];
}
export interface ReaTools {
  tools: { id: string; installed: string | null; inUse: boolean }[];
  installing: boolean;
  directory: string;
}
export interface ReaToolProgress {
  tool: string;
  item: string;
  phase: "download" | "verify" | "extract" | "configure" | "done" | "failed";
  downloaded: number;
  total: number | null;
}
