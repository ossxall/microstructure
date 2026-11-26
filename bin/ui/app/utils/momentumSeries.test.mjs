import assert from 'node:assert/strict'
import test from 'node:test'
import {
  calculateMomentumSeries,
  calculateMomentumValues
} from './momentumSeries.ts'

function makeBar(index, { buy = 600, sell = 400, up = 60, down = 40 } = {}, overrides = {}) {
  return {
    source: 'binance',
    symbol: 'BTCUSDT',
    timeframe: '5m',
    bar_key: `binance/BTCUSDT/5m/${index}`,
    revision: '1',
    timestamp: String(1_600_000_000_000 + index * 300_000),
    time: (1_600_000_000_000 + index * 300_000) / 1_000,
    price: { close: '123456789012345678' },
    volume_flow: {
      buy_notional: String(buy),
      sell_notional: String(sell)
    },
    price_changes: {
      positive_change_magnitude: String(up),
      negative_change_magnitude: String(down)
    },
    integrity: {},
    ...overrides
  }
}

function repeat(count, flow, options = {}) {
  return Array.from({ length: count }, (_, index) => makeBar(index, flow, options))
}

function lastValue(bars, side = 'bullish', options) {
  return calculateMomentumSeries(bars, options).at(-1)?.[side]
}

function closeTo(actual, expected) {
  assert.ok(Math.abs(actual - expected) <= 1e-6, `${actual} ≉ ${expected}`)
}

test('matches the required fixed-intensity, smoothing, cap, and warmup examples', () => {
  const a = { buy: 600, sell: 400, up: 60, down: 40 }
  const b = { buy: 4_000, sell: 0, up: 100, down: 0 }

  closeTo(lastValue(repeat(60, a)), 0.2)
  closeTo(lastValue(repeat(60, { buy: 950, sell: 50, up: 45, down: 55 })), 0.45)
  closeTo(lastValue(repeat(60, { buy: 1_000, sell: 0, up: 0, down: 100 })), 0.5)
  closeTo(lastValue([...repeat(60, a), makeBar(60, b)]), 0.44)
  closeTo(lastValue([...repeat(60, a), makeBar(60, b), makeBar(61, b)]), 0.640416)
  closeTo(lastValue([
    ...repeat(60, a),
    makeBar(60, { buy: 100_000, sell: 0, up: 100, down: 0 })
  ]), 0.573333)

  const warmup = calculateMomentumSeries(repeat(30, a))
  assert.equal(warmup.every(point => point.bullish === null && point.bearish === null), true)
})

test('a prefix calculation agrees with the same point in the full series', () => {
  const bars = [
    ...repeat(60, { buy: 600, sell: 400, up: 60, down: 40 }),
    makeBar(60, { buy: 4_000, sell: 0, up: 100, down: 0 }),
    makeBar(61, { buy: 4_000, sell: 0, up: 100, down: 0 })
  ]
  const full = calculateMomentumSeries(bars)

  for (let count = 31; count <= bars.length; count++) {
    const prefix = calculateMomentumSeries(bars.slice(0, count))
    closeTo(prefix.at(-1).bullish, full[count - 1].bullish)
    closeTo(prefix.at(-1).bearish, full[count - 1].bearish)
  }
})

test('keeps the highest revision regardless of arrival order; equal revisions use the last arrival', () => {
  const lowRevision = makeBar(0, { buy: 600, sell: 400, up: 60, down: 40 }, {
    bar_key: 'same',
    revision: '2'
  })
  const highRevision = makeBar(0, { buy: 4_000, sell: 0, up: 100, down: 0 }, {
    bar_key: 'same',
    revision: '9'
  })

  closeTo(calculateMomentumSeries([lowRevision, highRevision], { warmup: 0 }).at(-1).bullish, 1)
  closeTo(calculateMomentumSeries([highRevision, lowRevision], { warmup: 0 }).at(-1).bullish, 1)

  const equalRevisionLast = makeBar(0, { buy: 600, sell: 400, up: 60, down: 40 }, {
    bar_key: 'same',
    revision: '9'
  })
  closeTo(calculateMomentumSeries([highRevision, equalRevisionLast], { warmup: 0 }).at(-1).bullish, 0.2)

  const aligned = calculateMomentumValues([lowRevision, highRevision], { warmup: 0 })
  assert.equal(aligned[0], null)
  assert.equal(aligned[1].bullish, 1)
})

test('keeps momentum state independent between source/symbol/timeframe groups', () => {
  const a = { buy: 600, sell: 400, up: 60, down: 40 }
  const b = { buy: 4_000, sell: 0, up: 100, down: 0 }
  const primary = [...repeat(60, a), makeBar(60, b)]
  const other = makeBar(61, b, { source: 'dydx', bar_key: 'other/BTCUSDT/5m/61' })
  const values = calculateMomentumValues([...primary, other], { warmup: 0 })

  closeTo(values[60].bullish, 0.44)
  closeTo(values.at(-1).bullish, 1)
})

test('uses exact integer text for large notionals and flags optional integrity fields', () => {
  const bar = makeBar(0, {
    buy: '1000000000000000000000000000000',
    sell: '3000000000000000000000000000000',
    up: 0,
    down: 0
  }, {
    integrity: { dropped_trade_count: '2' }
  })
  const point = calculateMomentumSeries([bar], { warmup: 0 }).at(-1)

  closeTo(point.bullish, 0)
  closeTo(point.bearish, 0.25)
  assert.equal(point.doubtful, true)
  assert.equal(point.close, 123456789012345678n)
})

test('supports configured timestamp units while validating the date in UTC', () => {
  const timestampSeconds = makeBar(0, {}, {
    timestamp: '1600000000'
  })
  const point = calculateMomentumSeries(
    [timestampSeconds],
    { timestampUnit: 's', warmup: 0 }
  ).at(-1)

  assert.equal(point.time, 1_600_000_000)
})

test('rejects missing required fields and timestamps outside the supported era', () => {
  const missing = makeBar(0, {}, { price_changes: undefined })
  assert.throws(
    () => calculateMomentumSeries([missing]),
    /price_changes, línea 1/
  )

  const badDate = makeBar(0, {}, { timestamp: '946684799999' })
  assert.throws(
    () => calculateMomentumSeries([badDate]),
    /fuera del rango 2000–2100/
  )
  assert.throws(() => calculateMomentumSeries([]), /entrada está vacía/)
})
