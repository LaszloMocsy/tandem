// Drives the hero clip: calls Tandem's real MCP tools on a fixed timeline, so
// every take moves the board the same way. See RECORDING.md.
//
//   bun site/demo/drive.ts <mcp-url> [--now]
//
// Run seed.ts first and open the demo repo in Tandem. drive.ts then:
//   1. claims "Retry failed requests with backoff" as Codex, so In progress
//      shows a live agent, and keeps that lease alive until you quit;
//   2. waits for Enter (or starts at once with --now), then, as Claude Code,
//      works "Add a --units flag" on the timeline below.
// Quit with Ctrl-C once you've finished recording.

/** Seconds after Enter. Keep in step with the shot list in RECORDING.md. */
const TIMELINE = {
  start: 2,
  progress: [4.5, 6.5],
  finish: 8,
};
const HEARTBEAT_MS = 60_000;

const UNITS = 'Add a --units flag';
const RETRY = 'Retry failed requests with backoff';
const PROGRESS = ['Parsing --units with a small enum', 'Locale default done, adding tests'];
const SUMMARY = 'Added --units with metric and imperial, defaulting from the locale, plus tests.';

/** A minimal MCP client over Streamable HTTP: JSON-RPC posts, answered as JSON or SSE. */
class McpClient {
  private session: string | undefined;
  private nextId = 1;

  constructor(
    private readonly url: string,
    private readonly clientName: string,
  ) {}

  async connect() {
    await this.request('initialize', {
      protocolVersion: '2025-06-18',
      capabilities: {},
      clientInfo: { name: this.clientName, version: '1.0.0' },
    });
    await this.post({ jsonrpc: '2.0', method: 'notifications/initialized' });
  }

  /** Calls a tool and returns its JSON result; throws on a tool error. */
  async call<T = Record<string, unknown>>(name: string, args: Record<string, unknown>): Promise<T> {
    const result = (await this.request('tools/call', { name, arguments: args })) as {
      content?: { type: string; text?: string }[];
      structuredContent?: T;
      isError?: boolean;
    };
    const text = result.content?.find((c) => c.type === 'text')?.text ?? '';
    if (result.isError) throw new Error(`${name}: ${text}`);
    if (result.structuredContent) return result.structuredContent;
    return (text ? JSON.parse(text) : {}) as T;
  }

  private async request(method: string, params: unknown): Promise<unknown> {
    const id = this.nextId++;
    const res = await this.post({ jsonrpc: '2.0', id, method, params });
    const messages = await readMessages(res);
    const reply = messages.find((m) => m.id === id);
    if (!reply) throw new Error(`${method}: no reply from the server`);
    if (reply.error) throw new Error(`${method}: ${reply.error.message}`);
    return reply.result;
  }

  private async post(body: unknown): Promise<Response> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      Accept: 'application/json, text/event-stream',
    };
    if (this.session) headers['Mcp-Session-Id'] = this.session;
    const res = await fetch(this.url, { method: 'POST', headers, body: JSON.stringify(body) });
    if (!res.ok) throw new Error(`MCP server answered ${res.status}: ${await res.text()}`);
    this.session ??= res.headers.get('Mcp-Session-Id') ?? undefined;
    return res;
  }
}

interface Message {
  id?: number;
  result?: unknown;
  error?: { message: string };
}

async function readMessages(res: Response): Promise<Message[]> {
  const text = await res.text();
  if (!text.trim()) return [];
  if (!(res.headers.get('Content-Type') ?? '').includes('text/event-stream')) {
    const body = JSON.parse(text) as Message | Message[];
    return Array.isArray(body) ? body : [body];
  }
  // SSE: one JSON-RPC message per event, in its data lines.
  return text
    .split(/\r?\n\r?\n/)
    .map((event) =>
      event
        .split(/\r?\n/)
        .filter((line) => line.startsWith('data:'))
        .map((line) => line.slice(5).trimStart())
        .join('\n'),
    )
    .filter((data) => data.trim())
    .map((data) => JSON.parse(data) as Message);
}

interface TaskSummary {
  id: string;
  title: string;
  status: string;
  lease_holder: string | null;
  lease_stale: boolean | null;
}

async function findTask(client: McpClient, title: string): Promise<TaskSummary> {
  const { tasks } = await client.call<{ tasks: TaskSummary[] }>('list_tasks', { query: title });
  const task = tasks.find((t) => t.title === title);
  if (!task) throw new Error(`No task titled "${title}". Is the demo repo seeded and open?`);
  return task;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function log(t0: number, message: string) {
  const s = ((performance.now() - t0) / 1000).toFixed(1).padStart(5);
  console.log(`[${s}s] ${message}`);
}

async function main() {
  const args = process.argv.slice(2);
  const url = args.find((a) => !a.startsWith('--'));
  if (!url) {
    console.error('usage: bun site/demo/drive.ts <mcp-url> [--now]');
    process.exit(1);
  }

  const codex = new McpClient(url, 'codex');
  const claude = new McpClient(url, 'claude-code');
  await codex.connect();
  await claude.connect();

  const units = await findTask(claude, UNITS);
  if (units.status !== 'todo') {
    throw new Error(`"${UNITS}" is ${units.status}, not todo. Rerun seed.ts to reset the board.`);
  }

  // Codex's task: claim it unless a live agent already holds it.
  const retry = await findTask(codex, RETRY);
  let codexAgent: string | undefined;
  if (retry.status === 'todo' || (retry.status === 'in_progress' && retry.lease_stale !== false)) {
    ({ agent: codexAgent } = await codex.call<{ agent: string }>('start_task', { id: retry.id }));
    console.log(`Codex (${codexAgent}) holds "${RETRY}".`);
  } else if (retry.status === 'in_progress') {
    console.log(`"${RETRY}" is already held by ${retry.lease_holder}; leaving it.`);
  } else {
    throw new Error(`"${RETRY}" is ${retry.status}. Rerun seed.ts to reset the board.`);
  }
  // Any call that passes the holder's id extends the lease.
  const heartbeat = codexAgent
    ? setInterval(() => {
        codex.call('get_task', { id: retry.id, agent: codexAgent }).catch((e: unknown) => {
          console.error(`Heartbeat failed: ${String(e)}`);
        });
      }, HEARTBEAT_MS)
    : undefined;

  if (!args.includes('--now')) {
    console.log('\nStart recording, then press Enter to run the timeline.');
    for await (const _ of console) break;
  }

  const t0 = performance.now();
  const at = (seconds: number) => sleep(Math.max(0, t0 + seconds * 1000 - performance.now()));

  await at(TIMELINE.start);
  const { agent } = await claude.call<{ agent: string }>('start_task', { id: units.id });
  log(t0, `start_task ${units.id} as ${agent}`);

  for (const [i, seconds] of TIMELINE.progress.entries()) {
    await at(seconds);
    await claude.call('log_progress', { id: units.id, agent, message: PROGRESS[i] });
    log(t0, `log_progress "${PROGRESS[i]}"`);
  }

  await at(TIMELINE.finish);
  await claude.call('finish_task', {
    id: units.id,
    agent,
    summary: SUMMARY,
    files_changed: ['src/cli.ts', 'src/units.ts'],
    met: [1, 2, 3],
  });
  log(t0, `finish_task ${units.id}: in Review`);

  console.log('\nNow open the card and press R. Ctrl-C when you are done recording.');
  if (!heartbeat) return;
  process.on('SIGINT', () => {
    clearInterval(heartbeat);
    process.exit(0);
  });
}

main().catch((e: unknown) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
