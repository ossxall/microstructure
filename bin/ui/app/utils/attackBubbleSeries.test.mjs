import assert from 'node:assert/strict'
import test from 'node:test'
import {
  buildAttackBubbleValues,
  countAttackBubbles,
  createAttackBubbleSeries
} from './attackBubbleSeries.ts'

function makeBar(time, flow, candle = {}) {
  return {
    time,
    open: 100,
    high: 110,
    low: 90,
    close: 108,
    volume: 100,
    revision: 1,
    header: { quantity_scale: 0 },
    volume_flow: {
      buy_qty: flow.buy,
      sell_qty: flow.sell
    },
    ...candle
  }
}

function baselineBars(count = 5) {
  return Array.from({ length: count }, (_, index) => (
    makeBar(index + 1, { buy: 50, sell: 50 })
  ))
}

test('marks a high-volume aggressive-buy attack confirmed by price', () => {
  const bars = [
    ...baselineBars(),
    makeBar(6, { buy: 180, sell: 20 })
  ]

  const values = buildAttackBubbleValues(bars)
  assert.equal(values.slice(0, -1).every(value => value === null), true)
  assert.equal(values.at(-1)?.side, 'bullish')
  assert.equal(values.at(-1)?.dominance, 0.8)
  assert.equal(values.at(-1)?.relativeVolume, 2)
  assert.ok(values.at(-1).radius > 5)
  assert.equal(countAttackBubbles(bars), 1)
})

test('marks an aggressive-sell attack and handles a weak close at the low', () => {
  const bars = [
    ...baselineBars(),
    makeBar(6, { buy: 15, sell: 185 }, {
      open: 105,
      high: 110,
      low: 90,
      close: 91
    })
  ]

  assert.equal(buildAttackBubbleValues(bars).at(-1)?.side, 'bearish')
})

test('does not label balanced flow, low relative volume, or contradicted price action as an attack', () => {
  const baseline = baselineBars()
  const cases = [
    [...baseline, makeBar(6, { buy: 100, sell: 100 })],
    [...baseline, makeBar(6, { buy: 90, sell: 10 })],
    [...baseline, makeBar(6, { buy: 180, sell: 20 }, {
      open: 105,
      high: 110,
      low: 90,
      close: 91
    })]
  ]

  for (const bars of cases) {
    assert.equal(buildAttackBubbleValues(bars).at(-1), null)
  }
})

test('uses aggressor side from raw ticks when aggregated flow is missing', () => {
  const bars = baselineBars()
  bars.push(makeBar(6, { buy: Number.NaN, sell: Number.NaN }, {
    raw_ticks: [
      { qty: '180', is_buyer_maker: 0 },
      { qty: '20', is_buyer_maker: 1 }
    ]
  }))

  assert.equal(buildAttackBubbleValues(bars).at(-1)?.side, 'bullish')
})

test('warms up the rolling volume baseline and exposes editable compute thresholds', () => {
  assert.equal(
    buildAttackBubbleValues([
      ...baselineBars(4),
      makeBar(5, { buy: 180, sell: 20 })
    ]).at(-1),
    null
  )

  const series = createAttackBubbleSeries()
  assert.equal(series.id, 'aggressive-attack-bubbles')
  assert.equal(series.params.minDominance.affectsCompute, true)
  assert.equal(series.params.minRelativeVolume.value, 1.25)
  assert.equal(series.params.baselineBars.value, 20)
})
