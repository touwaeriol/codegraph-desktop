const fs = require("node:fs");
const readline = require("node:readline");

if (process.argv.includes("status")) {
  console.log(JSON.stringify({
    initialized: true,
    projectPath: process.cwd(),
    index: { state: "complete" },
    fileCount: 1,
    nodeCount: 1,
    edgeCount: 0,
  }));
  process.exit(0);
}

const input = readline.createInterface({ input: process.stdin });
input.on("line", (line) => {
  const request = JSON.parse(line);
  if (request.id === undefined) return;
  let result;
  switch (request.method) {
    case "initialize":
      if (fs.existsSync("fail-start")) process.exit(1);
      result = {
        protocolVersion: request.params.protocolVersion,
        capabilities: { tools: {} },
        serverInfo: { name: "runtime-test", version: "1.0.0" },
      };
      break;
    case "tools/list":
      result = { tools: [{
        name: "codegraph_explore",
        inputSchema: { type: "object", properties: { query: { type: "string" } } },
      }] };
      break;
    case "tools/call":
      if (request.params.arguments.query === "exit") process.exit(1);
      result = { content: [{ type: "text", text: "ok" }] };
      break;
    default:
      result = {};
  }
  console.log(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }));
});
input.on("close", () => process.exit(0));
