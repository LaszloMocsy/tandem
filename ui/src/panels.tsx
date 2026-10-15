import { useEffect, useState, type ReactNode } from 'react'
import type { AgentInfo, Api, ProjectInfo, TaskSummary } from './api.ts'
import { elapsed } from './columns.ts'
import { errorMessage } from './edit.ts'
import { AGENTS_SNIPPET, claudeCommand, copyText, mcpJson } from './setup.ts'

/** How recently an agent must have called to count as active: one lease. */
const ACTIVE_MS = 15 * 60_000

/** A panel in the detail pane's place, closed with × or Escape. */
function SidePanel({
  title,
  onClose,
  children,
}: {
  title: string
  onClose: () => void
  children: ReactNode
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  return (
    <aside className="detail" aria-label={title}>
      <button type="button" className="close" aria-label="Close" onClick={onClose}>
        ×
      </button>
      <div className="detail-body">
        <h2 className="panel-title">{title}</h2>
        {children}
      </div>
    </aside>
  )
}

/**
 * The Agents panel. Refetches when `tasks` changes and on every `now`
 * tick, since calls that only read change nothing the board hears about.
 */
export function AgentsPanel({
  project,
  tasks,
  now,
  onOpenTask,
  onClose,
  api,
}: {
  project: string
  tasks: readonly TaskSummary[]
  now: number
  onOpenTask: (id: string) => void
  onClose: () => void
  api: Pick<Api, 'listAgents'>
}) {
  const [agents, setAgents] = useState<AgentInfo[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let current = true
    api.listAgents(project).then(
      (agents) => {
        if (!current) return
        setAgents(agents)
        setError(null)
      },
      (error: unknown) => {
        if (current) setError(errorMessage(error))
      },
    )
    return () => {
      current = false
    }
  }, [api, project, tasks, now])

  return (
    <SidePanel title="Agents" onClose={onClose}>
      <p className="hint">Agents seen in the last day, and any that still hold a task.</p>
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      {agents === null ? (
        !error && <p className="muted">Loading…</p>
      ) : (
        <AgentList agents={agents} now={now} onOpenTask={onOpenTask} />
      )}
    </SidePanel>
  )
}

/** The agents, each with when it was last seen and the tasks it holds. */
export function AgentList({
  agents,
  now,
  onOpenTask,
}: {
  agents: readonly AgentInfo[]
  now: number
  onOpenTask: (id: string) => void
}) {
  if (agents.length === 0) {
    return <p className="muted">No agents yet. Connect one from Setup.</p>
  }
  return (
    <ul className="agents">
      {agents.map((agent) => {
        const ago = now - Date.parse(agent.lastSeenAt)
        const active = ago < ACTIVE_MS
        return (
          <li key={agent.agentId}>
            <div className="agent-head">
              <span
                className={active ? 'dot active' : 'dot'}
                role="img"
                aria-label={active ? 'Active' : 'Idle'}
              />
              <code>{agent.agentId}</code>
              <span className="muted">
                {ago < 60_000 ? 'seen just now' : `seen ${elapsed(ago)} ago`}
              </span>
            </div>
            {agent.tasks.length > 0 ? (
              <ul className="held">
                {agent.tasks.map((task) => (
                  <li key={task.id}>
                    <button type="button" className="link" onClick={() => onOpenTask(task.id)}>
                      {task.title || task.id}
                    </button>
                    <code className="muted">{task.id}</code>
                    {!task.live && <span className="badge">lease stale</span>}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="muted held-none">Holds no task.</p>
            )}
          </li>
        )
      })}
    </ul>
  )
}

/** The Setup panel: the MCP URL and the configs that connect an agent. */
export function SetupPanel({ project, onClose }: { project: ProjectInfo; onClose: () => void }) {
  const url = project.mcpUrl
  return (
    <SidePanel title="Setup" onClose={onClose}>
      {url === null ? (
        <p className="problem">
          The MCP server isn't running, so agents can't connect. It starts with the app; another
          copy of Tandem may be using port 4317.
        </p>
      ) : (
        <>
          <CopyBlock title="MCP URL" text={url} hint="This project's endpoint. The app must be running." />
          <CopyBlock
            title="Claude Code"
            text={claudeCommand(url)}
            hint="Run in the repo to add the server for Claude Code."
          />
          <CopyBlock
            title=".mcp.json"
            text={mcpJson(url)}
            hint="Or commit this at the repo root so every Claude Code session gets it."
          />
        </>
      )}
      <CopyBlock
        title="AGENTS.md snippet"
        text={AGENTS_SNIPPET}
        hint="Add to AGENTS.md or CLAUDE.md so agents follow the task workflow."
      />
    </SidePanel>
  )
}

function CopyBlock({ title, text, hint }: { title: string; text: string; hint: string }) {
  const [state, setState] = useState<'idle' | 'copied' | 'failed'>('idle')

  useEffect(() => {
    if (state === 'idle') return
    const timer = setTimeout(() => setState('idle'), 2000)
    return () => clearTimeout(timer)
  }, [state])

  return (
    <section className="detail-section" aria-label={title}>
      <div className="copy-head">
        <h3>{title}</h3>
        <button
          type="button"
          className="link"
          onClick={() => copyText(text).then(() => setState('copied'), () => setState('failed'))}
        >
          {state === 'copied' ? 'Copied' : state === 'failed' ? 'Copy failed' : 'Copy'}
        </button>
      </div>
      <p className="hint">{hint}</p>
      <pre className="raw copy-text">{text}</pre>
    </section>
  )
}
