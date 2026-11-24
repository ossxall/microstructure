import assert from 'node:assert/strict'
import test from 'node:test'
import { RollingDeltaProfile } from './deltaProfileSeries.ts'

const BASE_TIME = Date.UTC(2026, 0, 1)
const SCALE = 8

function makeLevel(price, buy, sell, buyCount = 1, sellCount = 1) {
  const total = buy + sell
  return {
    price: String(price),
    buy_qty: String(buy),
    sell_qty: String(sell),
    buy_notional: String(buy * price),
    sell_notional: String(sell * price),
    buy_count: buyCount,
    sell_count: sellCount,
    delta: String(buy - sell),
    total_qty: String(total),
    total_count: buyCount + sellCount,
    flags: 0
  }
}

function makeBar({
  minute = 0,
  key,
  revision = 1,
  isFinal = true,
  source = 'binance',
  symbol = 'BTCUSDT',
  levels = [makeLevel(60_000n * 10n ** 8n, 10n, 5n)]
} = {}) {
  const timestamp = BASE_TIME + minute * 60_000
  const buy = levels.reduce((sum, level) => sum + BigInt(level.buy_qty), 0n)
  const sell = levels.reduce((sum, level) => sum + BigInt(level.sell_qty), 0n)
  const buyNotional = levels.reduce((sum, level) => sum + BigInt(level.buy_notional), 0n)
  const sellNotional = levels.reduce((sum, level) => sum + BigInt(level.sell_notional), 0n)
  const total = buy + sell
  const tradeCount = levels.reduce((sum, level) => sum + level.total_count, 0)

  return {
    time: timestamp / 1000,
    timestamp: String(timestamp),
    open: 60_000,
    high: 60_001,
    low: 59_999,
    close: 60_000,
    volume: Number(total) / 10 ** SCALE,
    revision,
    is_final: isFinal,
    event_type: 'bar.1m',
    source,
    symbol,
    timeframe: '1m',
    bar_key: key ?? `BTCUSDT:1m:${timestamp}`,
    header: { price_scale: SCALE, quantity_scale: SCALE },
    identity: { n_trades: tradeCount, sequence_contiguous: 1, coverage_flags: 0 },
    volume_flow: {
      total_qty: String(total),
      buy_qty: String(buy),
      sell_qty: String(sell),
      signed_qty: String(buy - sell),
      total_notional: String(buyNotional + sellNotional),
      buy_notional: String(buyNotional),
      sell_notional: String(sellNotional),
      signed_notional: String(buyNotional - sellNotional),
      buy_count: String(levels.reduce((sum, level) => sum + level.buy_count, 0)),
      sell_count: String(levels.reduce((sum, level) => sum + level.sell_count, 0)),
      max_trade_qty: '0',
      max_trade_notional: '0',
      vwap: '0'
    },
    footprint: {
      level_count: levels.length,
      poc_price: levels[0]?.price ?? '0',
      poc_volume: levels[0]?.total_qty ?? '0',
      poc_delta: levels[0]?.delta ?? '0',
      value_area_high: levels[0]?.price ?? '0',
      value_area_low: levels[0]?.price ?? '0',
      max_positive_delta: '0',
      max_negative_delta: '0',
      imbalance_count: 0,
      stacked_buy_imbalance: 0,
      stacked_sell_imbalance: 0,
      unfinished_auction_high: 0,
      unfinished_auction_low: 0,
      levels
    }
  }
}

function distribute(total, count) {
  const base = total / BigInt(count)
  const remainder = Number(total % BigInt(count))
  return Array.from({ length: count }, (_, index) => base + (index < remainder ? 1n : 0n))
}

function referenceBinanceBar() {
  const pocPrice = 6_031_530_000_000n
  const otherBuy = distribute(2_317_000_000n, 381)
  const otherSell = distribute(2_197_900_000n, 381)
  const buyCounts = distribute(1_295n, 382)
  const sellCounts = distribute(1_295n, 382)
  const levels = [
    makeLevel(
      pocPrice,
      590_100_000n,
      200_000n,
      Number(buyCounts[0]),
      Number(sellCounts[0])
    ),
    ...Array.from({ length: 381 }, (_, index) => makeLevel(
      pocPrice + BigInt(index + 1) * 10_000_000n,
      otherBuy[index] ?? 0n,
      otherSell[index] ?? 0n,
      Number(buyCounts[index + 1]),
      Number(sellCounts[index + 1])
    ))
  ]
  const bar = makeBar({ key: 'BTCUSDT:1m:reference-2590', levels })
  bar.identity.n_trades = 2_590
  bar.footprint.poc_price = String(pocPrice)
  bar.footprint.poc_volume = '590300000'
  bar.footprint.poc_delta = '589900000'
  return bar
}

test('converts the Binance 1m reference scales and recomputes POC', () => {
  const profile = new RollingDeltaProfile({ rollingWindowMinutes: 1 })
  assert.equal(profile.upsert(referenceBinanceBar()), true)

  const result = profile.snapshot()
  assert.equal(result.barsInWindow, 1)
  assert.equal(result.levelCount, 382)
  assert.equal(result.totalBuyVolume, 2_907_100_000n)
  assert.equal(result.totalSellVolume, 2_198_100_000n)
  assert.equal(result.totalVolume, 5_105_200_000n)
  assert.equal(result.netDelta, 709_000_000n)
  assert.equal(result.pocPrice, 60_315.3)
  assert.equal(result.pocVolume, 590_300_000n)
  assert.equal(result.maxPositiveDelta, 589_900_000n)
  assert.equal(result.priceScale, SCALE)
})

for (const windowMinutes of [1, 5, 15, 60]) {
  test(`keeps only the last ${windowMinutes} minute bars`, () => {
    const profile = new RollingDeltaProfile({ rollingWindowMinutes: windowMinutes })
    for (let minute = 0; minute < 60; minute++) {
      profile.upsert(makeBar({
        minute,
        levels: [makeLevel(BigInt(60_000 + minute) * 10n ** 8n, BigInt(minute + 1), 0n)]
      }))
    }

    const result = profile.snapshot()
    assert.equal(result.barsInWindow, windowMinutes)
    assert.equal(
      result.totalBuyVolume,
      BigInt(windowMinutes * 60 - (windowMinutes * (windowMinutes - 1)) / 2)
    )
  })
}

test('deduplicates repeated revisions and replaces the prior contribution', () => {
  const profile = new RollingDeltaProfile({ rollingWindowMinutes: 5 })
  const first = makeBar({
    key: 'bar-a',
    levels: [makeLevel(10n, 100n, 0n)]
  })
  const revised = makeBar({
    key: 'bar-a',
    revision: 2,
    levels: [makeLevel(10n, 70n, 30n)]
  })

  assert.equal(profile.upsert(first), true)
  assert.equal(profile.upsert(first), false)
  assert.equal(profile.upsert(revised), true)
  assert.equal(profile.upsert(first), false)
  assert.equal(profile.snapshot().totalVolume, 100n)
  assert.equal(profile.snapshot().netDelta, 40n)
})

test('accepts out-of-order bars and expires bars outside the rolling interval', () => {
  const profile = new RollingDeltaProfile({ rollingWindowMinutes: 2 })
  const barAt = (minute, buy) => makeBar({
    minute,
    key: `bar-${minute}`,
    levels: [makeLevel(10n, buy, 0n)]
  })

  profile.upsert(barAt(0, 100n))
  profile.upsert(barAt(2, 20n))
  profile.upsert(barAt(1, 10n))
  assert.equal(profile.snapshot().barsInWindow, 2)
  assert.equal(profile.snapshot().totalBuyVolume, 30n)
})

test('excludes an in-progress bar by default and includes its final revision', () => {
  const profile = new RollingDeltaProfile({ rollingWindowMinutes: 3 })
  const live = makeBar({
    key: 'live-bar',
    isFinal: false,
    levels: [makeLevel(10n, 20n, 0n)]
  })

  assert.equal(profile.upsert(live), true)
  assert.equal(profile.snapshot().barsInWindow, 0)
  assert.equal(profile.snapshot().totalVolume, 0n)
  assert.equal(profile.upsert({ ...live, revision: 2, is_final: true }), true)
  assert.equal(profile.snapshot().totalVolume, 20n)
})

test('aggregates repeated prices, drops zero levels, and accepts an empty footprint', () => {
  const profile = new RollingDeltaProfile({ rollingWindowMinutes: 3 })
  const repeated = makeBar({
    key: 'repeated',
    levels: [
      makeLevel(10n, 2n, 1n),
      makeLevel(10n, 3n, 1n),
      makeLevel(11n, 0n, 0n, 0, 0)
    ]
  })

  assert.equal(profile.upsert(repeated), true)
  assert.equal(profile.snapshot().levels.length, 1)
  assert.equal(profile.snapshot().levels[0].totalQty, 7n)
  assert.equal(profile.upsert(makeBar({ key: 'empty', minute: 1, levels: [] })), true)
  assert.equal(profile.snapshot().levels.length, 1)
})

test('diagnoses inconsistent levels and rejects incompatible symbol and scales', () => {
  const diagnostics = []
  const profile = new RollingDeltaProfile({}, message => diagnostics.push(message))
  const inconsistent = makeBar({
    key: 'bad-level',
    levels: [makeLevel(10n, 4n, 2n)]
  })
  inconsistent.footprint.levels[0].delta = '99'
  assert.equal(profile.upsert(inconsistent), false)
  assert.ok(diagnostics.length > 0)

  profile.upsert(makeBar({ key: 'valid' }))
  assert.equal(profile.upsert(makeBar({ key: 'other-symbol', symbol: 'ETHUSDT' })), false)
  assert.equal(profile.upsert(makeBar({ key: 'other-source', source: 'dydx' })), false)
  const incompatible = makeBar({ key: 'other-scale', minute: 1 })
  incompatible.header.quantity_scale = 6
  assert.equal(profile.upsert(incompatible), false)
})

test('incremental state matches a rebuild after late revisions and expiry', () => {
  const input = [
    makeBar({ minute: 0, key: 'b0', levels: [makeLevel(10n, 2n, 0n)] }),
    makeBar({ minute: 1, key: 'b1', levels: [makeLevel(10n, 3n, 1n)] }),
    makeBar({ minute: 2, key: 'b2', levels: [makeLevel(11n, 1n, 4n)] }),
    makeBar({ minute: 3, key: 'b3', levels: [makeLevel(12n, 7n, 2n)] }),
    makeBar({ minute: 4, key: 'b4', levels: [makeLevel(11n, 6n, 0n)] }),
    makeBar({ minute: 2, key: 'b2', revision: 2, levels: [makeLevel(11n, 2n, 1n)] })
  ]
  const incremental = new RollingDeltaProfile({ rollingWindowMinutes: 3 })
  for (const bar of input) incremental.upsert(bar)

  const rebuilt = new RollingDeltaProfile({ rollingWindowMinutes: 3 })
  for (const bar of [input[3], input[4], input[5]]) rebuilt.upsert(bar)

  const current = incremental.snapshot()
  const reference = rebuilt.snapshot()
  assert.deepEqual(
    current.levels.map(row => [String(row.priceRaw), String(row.buyQty), String(row.sellQty)]),
    reference.levels.map(row => [String(row.priceRaw), String(row.buyQty), String(row.sellQty)])
  )
  assert.equal(current.totalVolume, reference.totalVolume)
  assert.equal(current.netDelta, reference.netDelta)
  assert.equal(current.pocPrice, reference.pocPrice)
  assert.equal(current.valueAreaHigh, reference.valueAreaHigh)
  assert.equal(current.valueAreaLow, reference.valueAreaLow)
})

test('groups prices by optional tick size without changing source quantities', () => {
  const profile = new RollingDeltaProfile({
    rollingWindowMinutes: 1,
    tickSize: '0.01'
  })
  profile.upsert(makeBar({
    levels: [
      makeLevel(6_000_000_000_001n, 2n, 0n),
      makeLevel(6_000_000_000_999n, 3n, 0n)
    ]
  }))
  assert.equal(profile.snapshot().levelCount, 1)
  assert.equal(profile.snapshot().totalBuyVolume, 5n)
})

test('uses deterministic lower-price POC ties and expands Value Area from the POC', () => {
  const profile = new RollingDeltaProfile({ rollingWindowMinutes: 1, valueAreaPercentage: 70 })
  profile.upsert(makeBar({
    levels: [
      makeLevel(10n, 10n, 0n),
      makeLevel(20n, 10n, 0n)
    ]
  }))

  const result = profile.snapshot()
  assert.equal(result.pocPrice, 10 / 10 ** SCALE)
  assert.equal(result.valueAreaLow, 10 / 10 ** SCALE)
  assert.equal(result.valueAreaHigh, 20 / 10 ** SCALE)
  assert.equal(result.buySellRatio, null)
  assert.equal(result.levels[0].buySellRatio, null)
})
