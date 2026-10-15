import { COLUMNS, type Columns } from './columns.ts'
import { Card } from './Card.tsx'

/** The five columns. `now` decides which leases are still live. */
export function Board({ columns, now }: { columns: Columns; now: number }) {
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
                <Card key={card.task.id} card={card} now={now} />
              ))}
            </div>
          </section>
        )
      })}
    </main>
  )
}
