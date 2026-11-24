import type { ChartEngine } from '../packages/open-charts/src/core/ChartEngine'
import type { MainPane, SeriesDefinition } from '../packages/open-charts/src/core/types'
import type { Bar } from './bar'

const DEFAULT_TOP_PERCENTILE = 99
const ZERO = 0n

export interface InstitutionalTradeOptions {
  /** Keep trades at or above this notional percentile. */
  topPercentile: number
}

export interface InstitutionalTradeSeriesParams {
  topPercentile: {
    value: number
    affectsCompute: true
    min: number
    max: number
    step: number
  }
}

export interface InstitutionalTradeLevel {
  priceRaw: bigint
  price: number
  buyCount: number
  sellCount: number
  buyNotional: bigint
  sellNotional: bigint
}

export interface InstitutionalTradeProfile {
  topPercentile: number
  priceScale: number
  quantityScale: number
  totalTradeCount: number
  selectedTradeCount: number
  thresholdNotional: bigint | null
  levels: InstitutionalTradeLevel[]
}

interface TradeCandidate {
  price: bigint
  notional: bigint
  side: 'buy' | 'sell'
}

interface MutableLevel {
  buyCount: number
  sellCount: number
  buyNotional: bigint
  sellNotional: bigint
}

const DEFAULT_OPTIONS: InstitutionalTradeOptions = {
  topPercentile: DEFAULT_TOP_PERCENTILE
}

function integer(value: unknown): bigint | null {
  if (typeof value === 'bigint') {
    return value
  }

  if (typeof value === 'number') {
    return Number.isSafeInteger(value) ? BigInt(value) : null
  }

  if (typeof value === 'string' && /^\d+$/.test(value.trim())) {
    try {
      return BigInt(value.trim())
    } catch {
      return null
    }
  }

  return null
}

function scale(value: unknown): number | null {
  const parsed = integer(value)
  if (parsed === null || parsed < ZERO || parsed > 18n) {
    return null
  }

  return Number(parsed)
}

function normalizedPercentile(value: number): number {
  return Number.isFinite(value)
    ? Math.min(99.9, Math.max(50, value))
    : DEFAULT_TOP_PERCENTILE
}

function displayPrice(price: bigint, priceScale: number): number {
  return Number(price) / 10 ** priceScale
}

function ratio(numerator: bigint, denominator: bigint): number {
  if (denominator <= ZERO || numerator <= ZERO) {
    return 0
  }

  const precision = 1_000_000n
  return Number((numerator * precision) / denominator) / Number(precision)
}

function formatNotional(
  value: bigint | null,
  priceScale: number,
  quantityScale: number
): string {
  if (value === null) {
    return '—'
  }

  const displayValue = Number(value) / 10 ** (priceScale + quantityScale)
  if (!Number.isFinite(displayValue)) {
    return '—'
  }

  return new Intl.NumberFormat('en-US', {
    notation: 'compact',
    maximumFractionDigits: 1
  }).format(displayValue)
}

/**
 * Aggregates the highest-notional individual trades in the supplied Bars.
 * `raw_ticks` contains scaled integer prices/quantities and uses
 * is_buyer_maker=0 for aggressive BUYs and 1 for aggressive SELLs.
 */
export function computeInstitutionalTradeProfile(
  bars: readonly Bar[],
  options: Partial<InstitutionalTradeOptions> = {}
): InstitutionalTradeProfile {
  const topPercentile = normalizedPercentile(
    options.topPercentile ?? DEFAULT_TOP_PERCENTILE
  )
  const trades: TradeCandidate[] = []
  let priceScale = 0
  let quantityScale = 0
  let hasScale = false

  for (const bar of bars) {
    const currentPriceScale = scale(bar.header?.price_scale)
    const currentQuantityScale = scale(bar.header?.quantity_scale)
    if (currentPriceScale === null || currentQuantityScale === null) {
      continue
    }

    if (!hasScale) {
      priceScale = currentPriceScale
      quantityScale = currentQuantityScale
      hasScale = true
    } else if (
      currentPriceScale !== priceScale
      || currentQuantityScale !== quantityScale
    ) {
      // Raw notionals from different scales cannot be ranked together.
      continue
    }

    for (const tick of bar.raw_ticks ?? []) {
      const price = integer(tick.price)
      const quantity = integer(tick.qty)
      const buyerMaker = integer(tick.is_buyer_maker)

      if (
        price === null
        || quantity === null
        || price <= ZERO
        || quantity <= ZERO
        || (buyerMaker !== 0n && buyerMaker !== 1n)
      ) {
        continue
      }

      trades.push({
        price,
        notional: price * quantity,
        side: buyerMaker === 0n ? 'buy' : 'sell'
      })
    }
  }

  if (trades.length === 0) {
    return {
      topPercentile,
      priceScale,
      quantityScale,
      totalTradeCount: 0,
      selectedTradeCount: 0,
      thresholdNotional: null,
      levels: []
    }
  }

  const rankedTrades = trades.sort((left, right) => (
    left.notional < right.notional ? -1 : left.notional > right.notional ? 1 : 0
  ))
  const thresholdIndex = Math.min(
    rankedTrades.length - 1,
    Math.max(0, Math.ceil((topPercentile / 100) * rankedTrades.length) - 1)
  )
  const thresholdNotional = rankedTrades[thresholdIndex]?.notional ?? null
  const levels = new Map<bigint, MutableLevel>()
  let selectedTradeCount = 0

  for (const trade of trades) {
    if (thresholdNotional === null || trade.notional < thresholdNotional) {
      continue
    }

    selectedTradeCount++
    const level = levels.get(trade.price) ?? {
      buyCount: 0,
      sellCount: 0,
      buyNotional: ZERO,
      sellNotional: ZERO
    }

    if (trade.side === 'buy') {
      level.buyCount++
      level.buyNotional += trade.notional
    } else {
      level.sellCount++
      level.sellNotional += trade.notional
    }

    levels.set(trade.price, level)
  }

  return {
    topPercentile,
    priceScale,
    quantityScale,
    totalTradeCount: trades.length,
    selectedTradeCount,
    thresholdNotional,
    levels: [...levels.entries()]
      .map(([priceRaw, level]) => ({
        priceRaw,
        price: displayPrice(priceRaw, priceScale),
        ...level
      }))
      .sort((left, right) => (
        left.priceRaw < right.priceRaw ? -1 : left.priceRaw > right.priceRaw ? 1 : 0
      ))
  }
}

function drawInstitutionalProfile(
  ctx: CanvasRenderingContext2D,
  pane: MainPane,
  engine: ChartEngine,
  data: readonly Bar[],
  profile: InstitutionalTradeProfile,
  priceMin: number,
  priceMax: number
): void {
  if (profile.levels.length === 0 || pane.h < 40 || engine.chartW < 40) {
    return
  }

  const lastIndex = data.length - 1
  if (lastIndex < 0) {
    return
  }

  const lastBarX = engine.utils.xOf(lastIndex)
  if (lastBarX < 0 || lastBarX >= engine.chartW) {
    return
  }

  const markerX = Math.min(
    engine.chartW - 10,
    lastBarX + Math.max(12, engine.barWidth * 1.8)
  )
  if (markerX <= lastBarX + 3) {
    return
  }

  const maxNotional = profile.levels.reduce((max, level) => {
    const total = level.buyNotional + level.sellNotional
    return total > max ? total : max
  }, ZERO)

  ctx.save()
  ctx.beginPath()
  ctx.rect(0, 0, engine.chartW, pane.h)
  ctx.clip()

  for (const level of profile.levels) {
    if (level.price < priceMin || level.price > priceMax) {
      continue
    }

    const totalNotional = level.buyNotional + level.sellNotional
    const strength = ratio(totalNotional, maxNotional)
    const radius = 3.5 + Math.sqrt(strength) * 7
    const buyIsDominant = level.buyCount > level.sellCount
      || (
        level.buyCount === level.sellCount
        && level.buyNotional > level.sellNotional
      )
    const sellIsDominant = level.sellCount > level.buyCount
      || (
        level.buyCount === level.sellCount
        && level.sellNotional > level.buyNotional
      )
    const color = buyIsDominant
      ? '#22c55e'
      : sellIsDominant
        ? '#ef4444'
        : '#cbd5e1'
    const imbalance = totalNotional === ZERO
      ? 0
      : ratio(
          level.buyNotional > level.sellNotional
            ? level.buyNotional - level.sellNotional
            : level.sellNotional - level.buyNotional,
          totalNotional
        )
    const y = engine.utils.yOf(level.price, pane, priceMin, priceMax)

    ctx.globalAlpha = 0.16 + imbalance * 0.12
    ctx.fillStyle = color
    ctx.beginPath()
    ctx.arc(markerX, y, radius + 3, 0, Math.PI * 2)
    ctx.fill()

    ctx.globalAlpha = 0.72 + imbalance * 0.28
    ctx.beginPath()
    ctx.arc(markerX, y, radius, 0, Math.PI * 2)
    ctx.fill()
    ctx.strokeStyle = 'rgba(15, 23, 42, 0.9)'
    ctx.lineWidth = 1
    ctx.stroke()
  }

  ctx.restore()
}

export function createInstitutionalTradeSeries(
  getProfileBars: () => readonly Bar[]
): SeriesDefinition<Bar, InstitutionalTradeProfile, InstitutionalTradeSeriesParams> {
  const params: InstitutionalTradeSeriesParams = {
    topPercentile: {
      value: DEFAULT_TOP_PERCENTILE,
      affectsCompute: true,
      min: 50,
      max: 99.9,
      step: 0.1
    }
  }

  return {
    id: 'institutional-trade-profile',
    label: 'INSTITUTIONAL TRADE PROFILE',
    color: '#22c55e',
    layer: 'foreground',
    width: '100%',
    height: '100%',
    params,

    compute(data, options) {
      const profile = computeInstitutionalTradeProfile(getProfileBars(), {
        topPercentile: options.topPercentile.value
      })
      return data.map(() => profile)
    },

    render(
      ctx: CanvasRenderingContext2D,
      pane: MainPane,
      engine: ChartEngine,
      data: readonly Bar[],
      values: readonly InstitutionalTradeProfile[],
      _options: InstitutionalTradeSeriesParams,
      priceMin: number,
      priceMax: number
    ) {
      const profile = values[0]
      if (profile) {
        drawInstitutionalProfile(ctx, pane, engine, data, profile, priceMin, priceMax)
      }
    },

    tooltipRow(values, index) {
      const profile = values[index]
      return !profile || profile.selectedTradeCount === 0
        ? null
        : {
            label: 'Institutional flow',
            value: `P${profile.topPercentile} · ${profile.selectedTradeCount} trades`
          }
    },

    priceTags() {
      return []
    },

    valueRange(data, _values, start, end) {
      let low = Infinity
      let high = -Infinity

      for (let index = start; index < end; index++) {
        const bar = data[index]
        if (bar) {
          low = Math.min(low, bar.low)
          high = Math.max(high, bar.high)
        }
      }

      return Number.isFinite(low) && Number.isFinite(high)
        ? { lo: low, hi: high }
        : { lo: 0, hi: 1 }
    },

    legend(_data, values, index) {
      const profile = values[index] ?? values[0]
      if (!profile || profile.selectedTradeCount === 0) {
        return []
      }

      const buyCount = profile.levels.reduce((sum, level) => sum + level.buyCount, 0)
      const sellCount = profile.levels.reduce((sum, level) => sum + level.sellCount, 0)
      const buyNotional = profile.levels.reduce((sum, level) => sum + level.buyNotional, ZERO)
      const sellNotional = profile.levels.reduce((sum, level) => sum + level.sellNotional, ZERO)
      const dominantColor = buyCount > sellCount
        ? '#22c55e'
        : sellCount > buyCount
          ? '#ef4444'
          : buyNotional > sellNotional
            ? '#22c55e'
            : sellNotional > buyNotional
              ? '#ef4444'
              : '#cbd5e1'

      return [
        {
          label: `INST P${profile.topPercentile}`,
          value: `${profile.selectedTradeCount} trades`,
          color: dominantColor
        },
        {
          label: 'Umbral',
          value: formatNotional(
            profile.thresholdNotional,
            profile.priceScale,
            profile.quantityScale
          ),
          color: '#94a3b8'
        }
      ]
    }
  }
}

export function institutionalTradeDefaults(): InstitutionalTradeOptions {
  return { ...DEFAULT_OPTIONS }
}
