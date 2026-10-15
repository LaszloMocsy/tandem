import { COLUMNS, type Columns } from './columns.ts'
import { Card } from './Card.tsx'

/** The five columns. `now` decides which leases are still live. */
export function Board({
  columns,
  now,
  selected,
  onOpen,
}: {
  columns: Columns
  now: number
  /** The id of the task open in the detail pane. */
  selected: string | null
  onOpen: (id: string) => void
}) {
  return (
    <main className="board">
      {COLUMNS.map(({ status, title }) => {
        const cards = columns[status]
        return (
          <section key={status} className="column" aria-label={title}>
            <h2 className="column-title">
              {title}
              <span className="count">{cards.length}</span>
            </h2>
            <div className="cards">
              {cards.map((card) => (
                <Card
                  key={card.task.id}
                  card={card}
                  now={now}
                  selected={card.task.id === selected}
                  onOpen={onOpen}
                />
              ))}
            </div>
          </section>
        )
      })}
    </main>
  )
}
