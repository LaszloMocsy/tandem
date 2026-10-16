import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import type { ProjectInfo } from './api.ts'

/**
 * The project on screen, as a menu: the other projects to switch to, New
 * board, Open board and Remove. Arrow keys move between items and Escape
 * closes it.
 */
export function ProjectSwitcher({
  projects,
  current,
  onSwitch,
  onNewBoard,
  onOpenBoard,
  onRemove,
}: {
  projects: readonly ProjectInfo[]
  current: ProjectInfo
  onSwitch: (project: ProjectInfo) => void
  onNewBoard: () => void
  onOpenBoard: () => void
  onRemove: (project: ProjectInfo) => void
}) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  const button = useRef<HTMLButtonElement>(null)
  const menu = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    menu.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus()
    const onPointerDown = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    return () => document.removeEventListener('pointerdown', onPointerDown)
  }, [open])

  function close() {
    setOpen(false)
    button.current?.focus()
  }

  function choose(action: () => void) {
    setOpen(false)
    action()
  }

  function onKeyDown(event: KeyboardEvent) {
    // Keep keys from the board's shortcuts and the pane behind.
    event.stopPropagation()
    if (event.key === 'Escape' || event.key === 'Tab') {
      if (event.key === 'Escape') event.preventDefault()
      close()
      return
    }
    const step = { ArrowDown: 1, ArrowUp: -1 }[event.key]
    const edge = { Home: 'first', End: 'last' }[event.key]
    if (step === undefined && edge === undefined) return
    event.preventDefault()
    const items = [...(menu.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])]
    const at = items.indexOf(document.activeElement as HTMLElement)
    const next =
      edge === 'first'
        ? 0
        : edge === 'last'
          ? items.length - 1
          : (at + (step ?? 0) + items.length) % items.length
    items[next]?.focus()
  }

  return (
    <div className="switcher" ref={root}>
      <button
        ref={button}
        type="button"
        className="switcher-button"
        aria-haspopup="menu"
        aria-expanded={open}
        title={current.repoPath}
        onClick={() => setOpen((open) => !open)}
      >
        <span className={current.readOnly === null ? 'dot live' : 'dot read-only'} />
        {current.name}
        <span className="chevron" aria-hidden="true">
          ▾
        </span>
      </button>
      {open && (
        <div ref={menu} className="menu" role="menu" aria-label="Projects" onKeyDown={onKeyDown}>
          {projects.map((project) => {
            const isCurrent = project.id === current.id
            return (
              <button
                key={project.id}
                type="button"
                role="menuitem"
                className={project.open ? 'menu-item' : 'menu-item unavailable'}
                aria-current={isCurrent || undefined}
                title={project.error ?? project.repoPath}
                onClick={() => choose(() => !isCurrent && onSwitch(project))}
              >
                <span className="menu-check" aria-hidden="true">
                  {isCurrent ? '✓' : ''}
                </span>
                <span className="menu-name">{project.name}</span>
                {/* Isolated, so the right-to-left trick that cuts the start keeps its slashes. */}
                <span className="menu-path">
                  <bdi>{project.repoPath}</bdi>
                </span>
              </button>
            )
          })}
          <div className="menu-separator" role="separator" />
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            onClick={() => choose(onNewBoard)}
          >
            <span className="menu-check" aria-hidden="true">
              ＋
            </span>
            New board…
          </button>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            onClick={() => choose(onOpenBoard)}
          >
            <span className="menu-check" />
            Open board…
          </button>
          <div className="menu-separator" role="separator" />
          <button
            type="button"
            role="menuitem"
            className="menu-item danger"
            onClick={() => choose(() => onRemove(current))}
          >
            <span className="menu-check" />
            Remove “{current.name}” from Tandem…
          </button>
        </div>
      )}
    </div>
  )
}
