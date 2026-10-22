// Builds the demo repo for the hero recording: a small CLI called
// pocket-weather with real git history and nine tasks across the board.
// Rerun it to reset the repo. See RECORDING.md.
//
//   bun site/demo/seed.ts [path]        # default /tmp/tandem-demo
//
// The tasks are written by Tandem's own service (the demo_seed example in
// core), never by hand. The task in progress is claimed later by drive.ts,
// through MCP, so its lease is live while you record.
import { existsSync } from 'node:fs';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';

const repo = resolve(process.argv[2] ?? '/tmp/tandem-demo');
const root = resolve(import.meta.dir, '../..');
// Lives inside .git, so it marks the folder as ours without being committed.
const marker = join(repo, '.git', 'tandem-demo');

function run(cmd: string[], cwd = repo) {
  const result = Bun.spawnSync(cmd, { cwd, stdout: 'inherit', stderr: 'inherit' });
  if (result.exitCode !== 0) throw new Error(`${cmd.join(' ')} failed with exit code ${result.exitCode}`);
}

async function write(path: string, contents: string) {
  const full = join(repo, path);
  await mkdir(dirname(full), { recursive: true });
  await writeFile(full, contents);
}

// Only ever delete a folder this script made.
if (existsSync(repo)) {
  if (!existsSync(marker)) {
    console.error(`${repo} exists and wasn't made by seed.ts; refusing to delete it.`);
    process.exit(1);
  }
  await rm(repo, { recursive: true, force: true });
}
await mkdir(repo, { recursive: true });

await write(
  'package.json',
  `${JSON.stringify(
    {
      name: 'pocket-weather',
      version: '0.1.0',
      private: true,
      type: 'module',
      bin: { 'pocket-weather': 'src/cli.ts' },
      scripts: { test: 'bun test' },
    },
    null,
    2,
  )}\n`,
);
await write(
  'README.md',
  '# pocket-weather\n\nThe weather for a city, in one line.\n\n```sh\npocket-weather Lisbon\n```\n',
);
await write(
  'src/cli.ts',
  "#!/usr/bin/env bun\nconst city = process.argv.slice(2).join(' ');\nif (!city) {\n  console.error('usage: pocket-weather <city>');\n  process.exit(1);\n}\nconsole.log(`Weather for ${city}: coming soon`);\n",
);
await write('.gitignore', 'node_modules/\n');

run(['git', 'init', '--quiet', '--initial-branch=main']);
await writeFile(marker, 'Made by tandem/site/demo/seed.ts\n');
run(['git', 'config', 'user.name', 'Sam Lee']);
run(['git', 'config', 'user.email', 'sam@example.com']);
run(['git', 'add', '--all']);
run(['git', 'commit', '--quiet', '-m', 'Initial commit']);

run(['cargo', 'run', '--quiet', '-p', 'tandem-board-core', '--example', 'demo_seed', '--', repo], root);

console.log(`
Seeded ${repo}.

Next:
  1. In Tandem, open the folder (project switcher → Open folder…), or, if it's
     already open, wait for the board to refresh.
  2. Copy the project's MCP URL from the Setup panel.
  3. bun site/demo/drive.ts <mcp-url>
`);
