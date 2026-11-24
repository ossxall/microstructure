import assert from 'node:assert/strict'
import test from 'node:test'
import {
  buildDeltaCandle,
  buildDeltaSeries,
  createDeltaCandleSeries
} from './deltaCandleSeries.ts'

function makeBar(rawTicks, overrides = {}) {
  return {
    time: 1,
    open: 999,
    high: 999,
    low: 999,
    close: 999,
    volume: 0,
    revision: 1,
    header: { quantity_scale: 2, price_scale: 8 },
    raw_ticks: rawTicks,
    ...overrides
  }
}

test('builds per-bar OHLC from signed quantities in tick order, not prices', () => {
  const candle = buildDeltaCandle(makeBar([
    { price: '999999', qty: '200', is_buyer_maker: 1, time: 2, time_us: 2 },
    { price: '1', qty: '100', is_buyer_maker: 0, time: 2, time_us: 1 },
    { price: '123456', qty: '300', is_buyer_maker: 0, time: 3, time_us: 3 }
  ]))

  assert.deepEqual(candle, {
    time: 1,
    open: 0,
    high: 2,
    low: -1,
    close: 2,
    askQty: 4,
    bidQty: 2
  })
})

test('CVD carries the prior close while PER_BAR resets each candle to zero', () => {
  const bars = [
    makeBar([
      { qty: '500', is_buyer_maker: 0, time: 1, time_us: 1 },
      { qty: '200', is_buyer_maker: 1, time: 1, time_us: 2 }
    ], { time: 1 }),
    makeBar([
      { qty: '100', is_buyer_maker: 0, time: 2, time_us: 1 },
      { qty: '400', is_buyer_maker: 1, time: 2, time_us: 2 }
    ], { time: 2 })
  ]

  const perBar = buildDeltaSeries(bars, 'PER_BAR')
  const cvd = buildDeltaSeries(bars, 'CVD')

  assert.deepEqual(perBar.map(({ open, high, low, close }) => (
    { open, high, low, close }
  )), [
    { open: 0, high: 5, low: 0, close: 3 },
    { open: 0, high: 1, low: -3, close: -3 }
  ])
  assert.deepEqual(cvd.map(({ open, high, low, close }) => (
    { open, high, low, close }
  )), [
    { open: 0, high: 5, low: 0, close: 3 },
    { open: 3, high: 4, low: 0, close: 0 }
  ])
})

test('CVD full rebuilds and incremental updates preserve the retained-window base', () => {
  const bars = [
    makeBar([{ qty: '200', is_buyer_maker: 0, time: 1 }], { time: 1 }),
    makeBar([{ qty: '100', is_buyer_maker: 1, time: 2 }], { time: 2 })
  ]
  const baseClose = 7
  const expected = buildDeltaSeries(bars, 'CVD', baseClose)
  assert.deepEqual(expected.map(candle => [candle.open, candle.close]), [
    [7, 9],
    [9, 8]
  ])

  const series = createDeltaCandleSeries()
  series.params.mode.value = 'CVD'
  series.params.baseClose.value = baseClose
  let values = series.compute(bars, series.params)
  assert.deepEqual(values, expected)

  const revisedBars = [
    makeBar([{ qty: '300', is_buyer_maker: 0, time: 1 }], { time: 1 }),
    bars[1]
  ]
  values = series.compute(revisedBars, series.params)
  assert.deepEqual(values, buildDeltaSeries(revisedBars, 'CVD', baseClose))

  const appended = makeBar(
    [{ qty: '100', is_buyer_maker: 0, time: 3 }],
    { time: 3 }
  )
  revisedBars.push(appended)
  series.updateIncremental(revisedBars, values, true, series.params)
  assert.deepEqual(
    values,
    buildDeltaSeries(revisedBars, 'CVD', baseClose)
  )
})

test('skips empty or invalid raw tick bars and defaults the chart to PER_BAR', () => {
  assert.equal(buildDeltaCandle(makeBar([])), null)
  assert.equal(buildDeltaCandle(makeBar([
    { qty: '-1', is_buyer_maker: 0, time: 1, time_us: 1 }
  ])), null)

  const series = createDeltaCandleSeries()
  assert.equal(series.params.mode.value, 'PER_BAR')
  assert.equal(series.params.mode.affectsCompute, true)
  assert.equal(series.params.baseClose.value, 0)
  assert.equal(series.params.baseClose.affectsCompute, true)
})
