// Persistent REA MCP session for this local investigation. JSON requests arrive
// on stdin; complete responses are saved rather than truncated in a terminal.
import { Client } from 'file:///C:/Users/zenia/AppData/Roaming/npm/node_modules/rea-agents/node_modules/@modelcontextprotocol/client/dist/index.mjs';
import { StdioClientTransport } from 'file:///C:/Users/zenia/AppData/Roaming/npm/node_modules/rea-agents/node_modules/@modelcontextprotocol/client/dist/stdio.mjs';
import { createInterface } from 'node:readline';
import { writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const output = path.join(root, 'docs', 'native');
await mkdir(output, { recursive: true });
const client = new Client({ name: 'archiver-investigation', version: '1.0.0' });
const transport = new StdioClientTransport({
  command: 'C:/Program Files/nodejs/node.exe',
  args: ['C:/Users/zenia/AppData/Roaming/npm/node_modules/rea-agents/scripts/rea.mjs', 'mcp'],
  env: { ...process.env, GHIDRA_INSTALL_DIR: path.join(root, 'tools/ghidra_12.1.4_PUBLIC'), JAVA_HOME: path.join(root, 'tools/jdk-25.0.4.1+1') },
  stderr: 'inherit',
});
await client.connect(transport);
console.log('READY: Send {"name":"tool","arguments":{},"output":"label"}, or {"close":true}.');
try {
  for await (const line of createInterface({ input: process.stdin })) {
    if (!line.trim()) continue;
    try {
      const request = JSON.parse(line);
      if (request.close) { await client.callTool({ name: 'close_binary', arguments: {} }, { timeout: 600000 }); break; }
      if (!/^[a-z0-9_-]+$/i.test(request.output)) throw new Error('Invalid output label');
      const result = await client.callTool({ name: request.name, arguments: request.arguments ?? {} }, { timeout: 1200000 });
      const destination = path.join(output, request.output + '.json');
      await writeFile(destination, JSON.stringify(result, null, 2));
      const value = result.structuredContent ?? result.content?.filter(x => x.type === 'text').map(x => { try { return JSON.parse(x.text); } catch { return x.text; } })[0];
      const normalized = value?.normalized_result;
      const summary = typeof normalized === 'string' ? { characters: normalized.length } : Array.isArray(normalized) ? { matches: normalized.length } : Object.keys(normalized ?? {});
      console.log(JSON.stringify({ saved: destination, isError: result.isError, evidence_id: value?.evidence_id, summary: value?.error ?? value?.result ?? summary }));
    } catch (error) { console.log(JSON.stringify({ error: error.message })); }
  }
} finally { await client.close(); }
