import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import { useState, type ReactNode } from 'react'
import type { Status, TaskSummary } from './api.ts'
import { Card } from './Card.tsx'
import { COLUMNS, type CardData, type Columns } from './columns.ts'
import { moveFor } from './moves.ts'

/**
 * The five columns. Dragging a card onto another column asks `onMove` to
 * apply the transition between them; while dragging, the columns it can
 * go to are highlighted and the rest dimmed. `now` decides which leases
 * are still live.
 */
export function Board({
  columns,
  now,
  selected,
  readOnly,
  onOpen,
  onMove,
}: {
  columns: Columns
  now: number
  /** The id of the task open in the detail pane. */
  selected: string | null
  readOnly: boolean
  onOpen: (id: string) => void
  onMove: (task: TaskSummary, to: Status) => void
}) {
  const sensors = useSensors(
    // A short move before a drag starts, so a click still opens the card.
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    // Space picks a card up and drops it, so Enter still opens it.
    useSensor(KeyboardSensor, {
      keyboardCodes: { start: ['Space'], cancel: ['Escape'], end: ['Space'] },
    }),
  )
  const [dragged, setDragged] = useState<CardData | null>(null)
  const find = (id: unknown) =>
    COLUMNS.flatMap(({ status }) => columns[status]).find((card) => card.task.id === id) ?? null

  function onDragEnd({ active, over }: DragEndEvent) {
    setDragged(null)
    const card = find(active.id)
    const to = over?.id as Status | undefined
    if (card && to && to !== card.task.status) onMove(card.task, to)
  }

  return (
    <DndContext
      sensors={sensors}
      onDragStart={({ active }) => setDragged(find(active.id))}
      onDragEnd={onDragEnd}
      onDragCancel={() => setDragged(null)}
    >
      <main className="board">
        {COLUMNS.map(({ status, title }) => (
          <Column
            key={status}
            status={status}
            title={title}
            count={columns[status].length}
            from={dragged?.task.status ?? null}
          >
            {columns[status].map((card) => (
              <DraggableCard
                key={card.task.id}
                card={card}
                now={now}
                selected={card.task.id === selected}
                disabled={readOnly}
                onOpen={onOpen}
              />
            ))}
          </Column>
        ))}
      </main>
      <DragOverlay dropAnimation={null}>
        {dragged && <Card card={dragged} now={now} overlay />}
      </DragOverlay>
    </DndContext>
  )
}

function Column({
  status,
  title,
  count,
  from,
  children,
}: {
  status: Status
  title: string
  count: number
  /** The status of the card being dragged, if any. */
  from: Status | null
  children: ReactNode
}) {
  const { setNodeRef, isOver } = useDroppable({ id: status })
  const other = from !== null && from !== status
  const allowed = other && moveFor(from, status) !== null
  const className = [
    'column',
    other && (allowed ? 'drop-target' : 'drop-blocked'),
    allowed && isOver && 'drop-over',
  ]
    .filter(Boolean)
    .join(' ')
  return (
    <section ref={setNodeRef} className={className} aria-label={title}>
      <h2 className="column-title">
        {title}
        <span className="count">{count}</span>
      </h2>
      <div className="cards">{children}</div>
    </section>
  )
}

function DraggableCard({
  card,
  now,
  selected,
  disabled,
  onOpen,
}: {
  card: CardData
  now: number
  selected: boolean
  disabled: boolean
  onOpen: (id: string) => void
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: card.task.id,
    disabled,
  })
  return (
    <Card
      ref={setNodeRef}
      card={card}
      now={now}
      selected={selected}
      onOpen={onOpen}
      attributes={attributes}
      listeners={listeners}
      dragging={isDragging}
    />
  )
}
