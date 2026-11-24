import assert from 'node:assert/strict'
import test from 'node:test'
import {
  computeInstitutionalTradeProfile,
  createInstitutionalTradeSeries
} from './institutionalTradeSeries.ts'

function makeBar(rawTicks, scales = { price_scale: 0, quantity_scale: 0 }) {
  return {
    time: 1,
    open: 100,
    high: 400,
    low: 100,
    close: 300,
    volume: 0,
    revision: 1,
    header: scales,
    raw_ticks: rawTicks
  }
}

test('selects top-notional raw trades and groups their flow by price', () => {
  const bar = makeBar([
    { price: '100', qty: '1', is_buyer_maker: 0 },
    { price: '200', qty: '1', is_buyer_maker: 1 },
    { price: '300', qty: '1', is_buyer_maker: 0 },
    { price: '300', qty: '2', is_buyer_maker: 1 },
    { price: '400', qty: '2', is_buyer_maker: 0 },
    { price: '999', qty: '99', is_buyer_maker: 2 }
  ])

  const profile = computeInstitutionalTradeProfile([bar], { topPercentile: 50 })

  assert.equal(profile.totalTradeCount, 5)
  assert.equal(profile.selectedTradeCount, 3)
  assert.equal(profile.thresholdNotional, 300n)
  assert.deepEqual(profile.levels.map(level => level.price), [300, 400])
  assert.deepEqual(
    profile.levels.map(level => [level.buyCount, level.sellCount]),
    [[1, 1], [1, 0]]
  )
  assert.deepEqual(
    profile.levels.map(level => [level.buyNotional, level.sellNotional]),
    [[300n, 600n], [800n, 0n]]
  )
})

test('ranks notional using scaled integer raw ticks and rejects incompatible scales', () => {
  const bar = makeBar([
    { price: '6000000000000', qty: '100000000', is_buyer_maker: 0 },
    { price: '6000000000000', qty: '200000000', is_buyer_maker: 1 }
  ], { price_scale: 8, quantity_scale: 8 })
  const incompatible = makeBar([
    { price: '1', qty: '999999999999', is_buyer_maker: 0 }
  ], { price_scale: 0, quantity_scale: 0 })

  const profile = computeInstitutionalTradeProfile([bar, incompatible], {
    topPercentile: 90
  })

  assert.equal(profile.totalTradeCount, 2)
  assert.equal(profile.selectedTradeCount, 1)
  assert.equal(profile.thresholdNotional, 1_200_000_000_000_000_000_000n)
  assert.equal(profile.levels[0]?.price, 60_000)
  assert.equal(profile.levels[0]?.sellNotional, 1_200_000_000_000_000_000_000n)
})

test('returns an empty profile when no valid, sided raw trades are available', () => {
  const profile = computeInstitutionalTradeProfile([
    makeBar([
      { price: '0', qty: '10', is_buyer_maker: 0 },
      { price: '100', qty: '10', is_buyer_maker: 4 }
    ])
  ])

  assert.equal(profile.totalTradeCount, 0)
  assert.equal(profile.selectedTradeCount, 0)
  assert.equal(profile.thresholdNotional, null)
  assert.deepEqual(profile.levels, [])
})

test('exposes a configurable foreground marker gap', () => {
  const series = createInstitutionalTradeSeries(() => [])

  assert.equal(series.layer, 'foreground')
  assert.equal(series.params.markerGapPixels.value, 20)
  assert.equal(series.params.markerGapPixels.affectsCompute, false)
})
