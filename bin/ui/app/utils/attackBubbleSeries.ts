import type { ChartEngine } from '../packages/open-charts/src/core/ChartEngine'
import type { MainPane, SeriesDefinition } from '../packages/open-charts/src/core/types'
import type { Bar } from './bar'

const BULL_COLOR = '#22c55e'
const BEAR_COLOR = '#ef4444'
const BASELINE_BARS = 20
const MIN_BASELINE_SAMPLES = 5
const MIN_BUBBLE_RADIUS = 5
const MAX_BUBBLE_RADIUS = 14

interface AttackParams {
  minDominance: {
    value: number
    affectsCompute: true
    min: number
    max: number
    step: number
  }
  minRelativeVolume: {
    value: number
    affectsCompute: true
    min: number
    max: number
    step: number
  }
  baselineBars: {
    value: number
    affectsCompute: true
    min: number
    max: number
    step: number
  }
}

export interface AttackBubbleOptions {
  minDominance: number
  minRelativeVolume: number
  baselineBars: number
}

export const DEFAULT_ATTACK_BUBBLE_OPTIONS: Readonly<AttackBubbleOptions> = {
  minDominance: 0.25,
  minRelativeVolume: 1.25,
  baselineBars: BASELINE_BARS
}

export interface AttackBubble {
  time: number
  side: 'bullish' | 'bearish'
  buyQty: number
  sellQty: number
  dominance: number
  relativeVolume: number
  closeLocation: number
  radius: number
}

function numeric(value: unknown): number {
  if (typeof value === 'number') {
    return value
  }

  if (typeof value === 'string' && value.trim() !== '') {
    return Number(value)
  }

  return Number.NaN
}

function quantityScale(bar: Bar): number {
  const scale = numeric(bar.header?.quantity_scale)
  return Number.isInteger(scale) && scale >= 0 && scale <= 18 ? scale : 0
}

function getAggressorVolume(bar: Bar): { buyQty: number, sellQty: number } | null {
  const buyQty = numeric(bar.volume_flow?.buy_qty)
  const sellQty = numeric(bar.volume_flow?.sell_qty)
  const divisor = 10 ** quantityScale(bar)

  if (
    Number.isFinite(buyQty)
    && buyQty >= 0
    && Number.isFinite(sellQty)
    && sellQty >= 0
  ) {
    return { buyQty: buyQty / divisor, sellQty: sellQty / divisor }
  }

  // Fallback for live feeds that include raw trades but not aggregated flow.
  if (!Array.isArray(bar.raw_ticks)) {
    return null
  }

  let rawBuyQty = 0
  let rawSellQty = 0
  let hasValidTick = false

  for (const tick of bar.raw_ticks) {
    const qty = numeric(tick.qty)
    const buyerMaker = numeric(tick.is_buyer_maker)
    if (
      !Number.isFinite(qty)
      || qty < 0
      || (buyerMaker !== 0 && buyerMaker !== 1)
    ) {
      continue
    }

    hasValidTick = true
    if (buyerMaker === 0) {
      rawBuyQty += qty
    } else {
      rawSellQty += qty
    }
  }

  return hasValidTick
    ? { buyQty: rawBuyQty / divisor, sellQty: rawSellQty / divisor }
    : null
}

function median(values: number[]): number {
  const sorted = [...values].sort((left, right) => left - right)
  const middle = Math.floor(sorted.length / 2)
  return sorted.length % 2 === 0
    ? ((sorted[middle - 1] ?? 0) + (sorted[middle] ?? 0)) / 2
    : sorted[middle] ?? 0
}

function bubbleRadius(
  dominance: number,
  relativeVolume: number,
  minDominance: number,
  minRelativeVolume: number
): number {
  const dominanceStrength = (dominance - minDominance)
    / Math.max(0.01, 1 - minDominance)
  const volumeStrength = (relativeVolume - minRelativeVolume)
    / Math.max(0.01, 4 - minRelativeVolume)
  const strength = Math.max(0, Math.min(1, dominanceStrength * 0.55 + volumeStrength * 0.45))
  return MIN_BUBBLE_RADIUS + strength * (MAX_BUBBLE_RADIUS - MIN_BUBBLE_RADIUS)
}

function buildAttackAt(
  data: readonly Bar[],
  index: number,
  options: AttackBubbleOptions
): AttackBubble | null {
  const bar = data[index]
  if (
    !bar
    || ![bar.time, bar.open, bar.high, bar.low, bar.close].every(Number.isFinite)
    || bar.high <= bar.low
  ) {
    return null
  }

  const currentFlow = getAggressorVolume(bar)
  if (!currentFlow) {
    return null
  }

  const totalQty = currentFlow.buyQty + currentFlow.sellQty
  if (!Number.isFinite(totalQty) || totalQty <= 0) {
    return null
  }

  const delta = currentFlow.buyQty - currentFlow.sellQty
  const dominance = Math.abs(delta) / totalQty
  if (dominance < options.minDominance) {
    return null
  }

  const baselineBars = Math.max(1, Math.floor(options.baselineBars))
  const priorVolumes: number[] = []
  for (let priorIndex = Math.max(0, index - baselineBars); priorIndex < index; priorIndex++) {
    const priorBar = data[priorIndex]
    if (!priorBar) {
      continue
    }
    const priorFlow = getAggressorVolume(priorBar)
    const priorTotal = priorFlow ? priorFlow.buyQty + priorFlow.sellQty : 0
    if (Number.isFinite(priorTotal) && priorTotal > 0) {
      priorVolumes.push(priorTotal)
    }
  }

  if (priorVolumes.length < MIN_BASELINE_SAMPLES) {
    return null
  }

  const typicalVolume = median(priorVolumes)
  if (typicalVolume <= 0) {
    return null
  }

  const relativeVolume = totalQty / typicalVolume
  if (relativeVolume < options.minRelativeVolume) {
    return null
  }

  const closeLocation = (bar.close - bar.low) / (bar.high - bar.low)
  const isBullish = delta > 0
    && bar.close > bar.open
    && closeLocation >= 0.65
  const isBearish = delta < 0
    && bar.close < bar.open
    && closeLocation <= 0.35

  // Aggressive flow without a matching price response is treated as
  // absorption/exhaustion, not as a successful directional attack.
  if (!isBullish && !isBearish) {
    return null
  }

  return {
    time: bar.time,
    side: isBullish ? 'bullish' : 'bearish',
    buyQty: currentFlow.buyQty,
    sellQty: currentFlow.sellQty,
    dominance,
    relativeVolume,
    closeLocation,
    radius: bubbleRadius(
      dominance,
      relativeVolume,
      options.minDominance,
      options.minRelativeVolume
    )
  }
}

function formatQuantity(value: number): string {
  return new Intl.NumberFormat('es-ES', { maximumFractionDigits: 8 }).format(value)
}

function drawBubble(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  radius: number,
  side: AttackBubble['side']
) {
  const isBullish = side === 'bullish'
  const dark = isBullish ? '#064e3b' : '#7f1d1d'
  const mid = isBullish ? BULL_COLOR : BEAR_COLOR
  const light = isBullish ? '#bbf7d0' : '#fecaca'
  const highlightX = x - radius * 0.32
  const highlightY = y - radius * 0.38
  const gradient = ctx.createRadialGradient(
    highlightX,
    highlightY,
    radius * 0.04,
    x,
    y,
    radius
  )
  gradient.addColorStop(0, light)
  gradient.addColorStop(0.48, mid)
  gradient.addColorStop(1, dark)

  ctx.beginPath()
  ctx.arc(x, y, radius, 0, Math.PI * 2)
  ctx.fillStyle = gradient
  ctx.fill()
  ctx.lineWidth = 1
  ctx.strokeStyle = dark
  ctx.stroke()

  ctx.beginPath()
  ctx.arc(highlightX, highlightY, radius * 0.19, 0, Math.PI * 2)
  ctx.fillStyle = 'rgba(255, 255, 255, 0.58)'
  ctx.fill()
}

export function buildAttackBubbleValues(
  data: readonly Bar[],
  options: AttackBubbleOptions = DEFAULT_ATTACK_BUBBLE_OPTIONS
): Array<AttackBubble | null> {
  return data.map((_bar, index) => buildAttackAt(data, index, options))
}

export function countAttackBubbles(
  data: readonly Bar[],
  options: AttackBubbleOptions = DEFAULT_ATTACK_BUBBLE_OPTIONS
): number {
  return buildAttackBubbleValues(data, options).filter(Boolean).length
}

export function createAttackBubbleParams(): AttackParams {
  return {
    minDominance: {
      value: 0.25,
      affectsCompute: true,
      min: 0.1,
      max: 0.8,
      step: 0.05
    },
    minRelativeVolume: {
      value: 1.25,
      affectsCompute: true,
      min: 1,
      max: 3,
      step: 0.1
    },
    baselineBars: {
      value: BASELINE_BARS,
      affectsCompute: true,
      min: 5,
      max: 50,
      step: 1
    }
  }
}

export function createAttackBubbleSeries(): SeriesDefinition<
  Bar,
  AttackBubble | null,
  AttackParams
> {
  const params = createAttackBubbleParams()

  return {
    id: 'aggressive-attack-bubbles',
    label: 'ATAQUES ALCISTA/BAJISTA',
    color: BULL_COLOR,
    layer: 'foreground',
    width: '100%',
    height: '100%',
    params,

    compute(data, seriesParams) {
      return buildAttackBubbleValues(data, {
        minDominance: seriesParams.minDominance.value,
        minRelativeVolume: seriesParams.minRelativeVolume.value,
        baselineBars: seriesParams.baselineBars.value
      })
    },

    updateIncremental(data, values, isNewBar, seriesParams) {
      const lastIndex = data.length - 1
      const bubble = buildAttackAt(data, lastIndex, {
        minDominance: seriesParams.minDominance.value,
        minRelativeVolume: seriesParams.minRelativeVolume.value,
        baselineBars: seriesParams.baselineBars.value
      })
      if (isNewBar) {
        values.push(bubble)
      } else if (values.length > 0) {
        values[values.length - 1] = bubble
      }
    },

    render(
      ctx: CanvasRenderingContext2D,
      pane: MainPane,
      engine: ChartEngine,
      _data: readonly Bar[],
      values: readonly (AttackBubble | null)[],
      _seriesParams: AttackParams,
      priceMin: number,
      priceMax: number
    ) {
      ctx.save()
      for (
        let index = engine.viewStart;
        index < engine.viewEnd && index < values.length;
        index++
      ) {
        const bubble = values[index]
        if (!bubble) {
          continue
        }

        const bar = _data[index]
        if (!bar) {
          continue
        }

        const maxRadius = Math.max(2.5, engine.barWidth * 0.46)
        const radius = Math.min(bubble.radius, maxRadius)
        const x = Math.round(engine.utils.xOf(index))
        const edgeY = engine.utils.yOf(
          bubble.side === 'bullish' ? bar.low : bar.high,
          pane,
          priceMin,
          priceMax
        )
        const y = edgeY + (bubble.side === 'bullish' ? radius + 3 : -radius - 3)
        drawBubble(ctx, x, y, radius, bubble.side)
      }
      ctx.restore()
    },

    tooltipRow(values, index) {
      const bubble = values[index]
      if (!bubble) {
        return null
      }

      return {
        label: bubble.side === 'bullish' ? 'ATAQUE ALCISTA' : 'ATAQUE BAJISTA',
        value: `Compras agresoras ${formatQuantity(bubble.buyQty)} · Ventas agresoras ${formatQuantity(bubble.sellQty)} · Dominancia ${(bubble.dominance * 100).toFixed(1)}% · Volumen ${bubble.relativeVolume.toFixed(2)}×`
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
      const bubble = values[index]
      if (!bubble) {
        return []
      }

      const color = bubble.side === 'bullish' ? BULL_COLOR : BEAR_COLOR
      return [
        {
          label: 'Ataque',
          value: bubble.side === 'bullish' ? 'Alcista' : 'Bajista',
          color
        },
        {
          label: 'Dominancia',
          value: `${(bubble.dominance * 100).toFixed(1)}%`,
          color
        },
        {
          label: 'Volumen',
          value: `${bubble.relativeVolume.toFixed(2)}×`,
          color
        }
      ]
    }
  }
}
