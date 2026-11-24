import assert from 'node:assert/strict'
import test from 'node:test'
import {
  buildBidAskFlowCandle,
  buildBidAskFlowSeries,
  createBidAskFlowSeries
} from './bidAskFlowSeries.ts'

function makeBar(rawTicks, overrides = {}) {
  return {
    time: 1,
    revision: 1,
    header: { quantity_scale: 2, price_scale: 8 },
    raw_ticks: rawTicks,
    ...overrides
  }
}

test('builds chained bid/ask quantity candles without reading prices', () => {
  const bars = [
    makeBar([
      { price: '999999', qty: '500', is_buyer_maker: 0, time: 1, time_us: 1 },
      { price: '1', qty: '200', is_buyer_maker: 1, time: 1, time_us: 2 }
    ], { time: 1 }),
    makeBar([
      { price: '-999', qty: '100', is_buyer_maker: 0, time: 2, time_us: 1 },
      { price: '0', qty: '400', is_buyer_maker: 1, time: 2, time_us: 2 }
    ], { time: 2 })
  ]

  assert.deepEqual(buildBidAskFlowSeries(bars).map(({
    time, open, high, low, close, askQty, bidQty
  }) => ({ time, open, high, low, close, askQty, bidQty })), [
    {
      time: 1,
      open: 0,
      high: 5,
      low: 0,
      close: 3,
      askQty: 5,
      bidQty: 2
    },
    {
      time: 2,
      open: 3,
      high: 4,
      low: 0,
      close: 0,
      askQty: 1,
      bidQty: 4
    }
  ])
})

test('flow candles skip empty bars and chain from the last valid close', () => {
  const first = makeBar([
    { qty: '300', is_buyer_maker: 0, time: 1, time_us: 1 }
  ], { time: 1 })
  const empty = makeBar([], { time: 2 })
  const last = makeBar([
    { qty: '100', is_buyer_maker: 1, time: 3, time_us: 1 }
  ], { time: 3 })

  assert.equal(buildBidAskFlowCandle(empty), null)
  assert.deepEqual(
    buildBidAskFlowSeries([first, empty, last]).map(({ open, close }) => ({
      open,
      close
    })),
    [
      { open: 0, close: 3 },
      { open: 3, close: 2 }
    ]
  )
})

test('ignores unknown sides and only accumulates known buy and sell flow', () => {
  const candle = buildBidAskFlowCandle(makeBar([
    { qty: '500', is_buyer_maker: 255, time: 1, time_us: 1 },
    { qty: '300', is_buyer_maker: 0, time: 1, time_us: 2 },
    { qty: '100', is_buyer_maker: 1, time: 1, time_us: 3 }
  ]))

  assert.equal(candle.open, 0)
  assert.equal(candle.close, 2)
  assert.equal(candle.askQty, 3)
  assert.equal(candle.bidQty, 1)
})

test('returns no flow candle when all tick sides are unknown', () => {
  const candle = buildBidAskFlowCandle(makeBar([
    { qty: '100', is_buyer_maker: 255, time: 1, time_us: 1 }
  ]))

  assert.equal(candle, null)
})

test('bid/ask flow series defaults to candles and scales visible quantities', () => {
  const series = createBidAskFlowSeries()
  assert.equal(series.params.style.value, 'CANDLE')

  const range = series.valueRange([], [
    null,
    {
      time: 2,
      open: 10,
      high: 12,
      low: 10,
      close: 11,
      askQty: 2,
      bidQty: 1
    }
  ], 1, 2, series.params)

  const enginePadding = (range.hi - range.lo) * 0.06
  assert.ok(Math.abs(range.lo - enginePadding - 9.9) < 1e-12)
  assert.ok(Math.abs(range.hi + enginePadding - 12.1) < 1e-12)
})

test('draws cumulative flow candle borders with the body color', () => {
  const series = createBidAskFlowSeries()
  const borders = []
  const ctx = {
    save() {},
    restore() {},
    setLineDash() {},
    beginPath() {},
    moveTo() {},
    lineTo() {},
    stroke() {},
    fillRect() {},
    strokeRect(...rect) {
      borders.push({ color: this.strokeStyle, rect })
    }
  }
  const values = [
    {
      time: 1,
      open: 0,
      high: 3,
      low: 0,
      close: 3,
      askQty: 3,
      bidQty: 0
    },
    {
      time: 2,
      open: 3,
      high: 3,
      low: 1,
      close: 1,
      askQty: 0,
      bidQty: 2
    }
  ]
  const engine = {
    barWidth: 10,
    viewStart: 0,
    viewEnd: values.length,
    chartW: 100,
    utils: {
      xOf: index => index * 10,
      yOf: value => 100 - value * 10
    }
  }

  series.render(ctx, {}, engine, [], values, series.params, 0, 3)

  assert.deepEqual(borders.map(({ color }) => color), ['#22c55e', '#ef4444'])
})
