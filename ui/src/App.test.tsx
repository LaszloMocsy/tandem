import { renderToString } from 'react-dom/server'
import { expect, test } from 'vitest'
import App from './App.tsx'

test('renders the app title', () => {
  expect(renderToString(<App />)).toContain('Tandem')
})
