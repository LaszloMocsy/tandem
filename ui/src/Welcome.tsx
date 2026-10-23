import type { ProjectInfo } from './api.ts'
import { openedAgo, problemLabel } from './projects.ts'

/** The empty state: Open folder and the recent projects. */
export function Welcome({
  projects,
  now,
  onOpenFolder,
  onOpenProject,
  onRemoveProject,
}: {
  projects: readonly ProjectInfo[]
  now: number
  onOpenFolder: () => void
  onOpenProject: (project: ProjectInfo) => void
  onRemoveProject: (project: ProjectInfo) => void
}) {
  return (
    <main className="welcome">
      <div className="welcome-inner">
        <img className="logo" src="/logo.svg" alt="" width={56} height={56} />
        <h2>Welcome to Tandem</h2>
        <p className="welcome-lead">
          A task board you share with your coding agents, one per repo.
        </p>
        <div className="welcome-actions">
          <button type="button" className="welcome-action" onClick={onOpenFolder}>
            <span className="welcome-action-title">Open folder</span>
            <span className="welcome-action-hint">
              Pick a folder in a repo to open its board, or start one
            </span>
          </button>
        </div>
        {projects.length > 0 && (
          <section aria-label="Recent projects">
            <h3 className="welcome-heading">Recent</h3>
            <ul className="recent">
              {projects.map((project) => (
                <li key={project.id}>
                  <button
                    type="button"
                    className="recent-open"
                    title={project.error ?? project.repoPath}
                    onClick={() => onOpenProject(project)}
                  >
                    <span className="recent-name">{project.name}</span>
                    <span className="recent-path">{project.repoPath}</span>
                  </button>
                  {project.error !== null && (
                    <span className="badge invalid" title={project.error}>
                      {problemLabel(project.error)}
                    </span>
                  )}
                  <span className="recent-time">{openedAgo(project.lastOpenedAt, now)}</span>
                  <button
                    type="button"
                    className="recent-remove"
                    aria-label={`Remove ${project.name} from Tandem`}
                    title="Remove from Tandem"
                    onClick={() => onRemoveProject(project)}
                  >
                    ×
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </main>
  )
}
