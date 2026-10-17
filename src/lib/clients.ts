import type { Client } from "./types";

/** MCP clients whose project configuration the app manages, in display order. */
export const CLIENTS: readonly Client[] = [
  "codex",
  "claude",
  "cursor",
  "devin",
];
export const clientName = (client: Client) =>
  ({
    codex: "Codex",
    claude: "Claude Code",
    cursor: "Cursor",
    devin: "Devin",
  })[client];
/** The project file each client reads its MCP servers from. */
export const clientPath = (client: Client) =>
  ({
    codex: ".codex/config.toml",
    claude: ".mcp.json",
    cursor: ".cursor/mcp.json",
    devin: ".devin/mcp_config.json",
  })[client];
