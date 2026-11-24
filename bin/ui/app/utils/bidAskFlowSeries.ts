import type { SeriesDefinition } from '../packages/open-charts/src/core/types'
import type { DeltaBar } from './deltaCandleSeries'

const UP_COLOR = '#22c55e'
const DOWN_COLOR = '#ef4444'
const ENGINE_PRICE_PADDING = 0.06
const Y_PADDING = 0.05

function numeric(value: unknown): number {
  if (typeof value === 'number') {
    return value
  }

  if (typeof value === 'string' && value.trim() !== '') {
    return Number(value)
  }

  return Number.NaN
}

function orderedTicks(bar: DeltaBar) {
  return (bar.raw_ticks ?? [])
    .map((tick, index) => ({
      tick,
      index,
      time: numeric(tick.time),
      timeUs: numeric(tick.time_us)
    }))
    .sort((left, right) => {
      const leftTime = Number.isFinite(left.time) ? left.time : Infinity
      const rightTime = Number.isFinite(right.time) ? right.time : Infinity
      if (leftTime !== rightTime) {
        return leftTime < rightTime ? -1 : 1
      }

      const leftTimeUs = Number.isFinite(left.timeUs) ? left.timeUs : Infinity
      const rightTimeUs = Number.isFinite(right.timeUs) ? right.timeUs : Infinity
      if (leftTimeUs !== rightTimeUs) {
        return leftTimeUs < rightTimeUs ? -1 : 1
      }
      return left.index - right.index
    })
}

function formatQuantity(value: number): string {
  return new Intl.NumberFormat('en-US', {
    maximumFractionDigits: 12
  }).format(value)
}

export type FlowCandleStyle = 'CANDLE' | 'OHLC_BAR'

export interface BidAskFlowCandle {
  time: number
  open: number
  high: number
  low: number
  close: number
  askQty: number
  bidQty: number
}

interface FlowParams {
  style: {
    value: FlowCandleStyle
    affectsCompute: false
    options: readonly { label: string, value: FlowCandleStyle }[]
  }
}

const FLOW_BAR_WIDTH_RATIO = 0.62

/**
 * Builds an always-cumulative bid/ask quantity candle. Price fields are never
 * read; the only inputs are quantity scale and raw tick time/quantity/side.
 */
export function buildBidAskFlowCandle(
  bar: DeltaBar,
  prevClose = 0
): BidAskFlowCandle | null {
  const quantityScale = numeric(bar.header?.quantity_scale)
  if (
    !Number.isInteger(quantityScale)
    || quantityScale < 0
    || quantityScale > 18
    || !Number.isFinite(bar.time)
    || !Array.isArray(bar.raw_ticks)
    || bar.raw_ticks.length === 0
  ) {
    return null
  }

  const divisor = 10 ** quantityScale
  let running = Number.isFinite(prevClose) ? prevClose : 0
  const open = running
  let high = open
  let low = open
  let askQty = 0
  let bidQty = 0
  let hasValidTick = false

  for (const { tick } of orderedTicks(bar)) {
    const qty = numeric(tick.qty)
    const buyerMaker = numeric(tick.is_buyer_maker)
    if (
      !Number.isFinite(qty)
      || qty < 0
      || !Number.isFinite(buyerMaker)
    ) {
      continue
    }

    hasValidTick = true
    const signedQty = (buyerMaker === 0 ? qty : -qty) / divisor
    running += signedQty

    if (signedQty > 0) {
      askQty += signedQty
    } else {
      bidQty += -signedQty
    }

    high = Math.max(high, running)
    low = Math.min(low, running)
  }

  if (!hasValidTick) {
    return null
  }

  return {
    time: bar.time,
    open,
    high,
    low,
    close: running,
    askQty,
    bidQty
  }
}

export function buildBidAskFlowSeries(
  bars: readonly DeltaBar[]
): BidAskFlowCandle[] {
  const candles: BidAskFlowCandle[] = []
  let prevClose = 0

  for (const bar of bars) {
    const candle = buildBidAskFlowCandle(bar, prevClose)
    if (!candle) {
      continue
    }

    candles.push(candle)
    prevClose = candle.close
  }

  return candles
}

function previousFlowClose(
  values: readonly (BidAskFlowCandle | null)[]
): number {
  for (let index = values.length - 1; index >= 0; index--) {
    const candle = values[index]
    if (candle) {
      return candle.close
    }
  }
  return 0
}

function rangeForFlowCandles(
  candles: readonly (BidAskFlowCandle | null)[]
) {
  let low = Infinity
  let high = -Infinity

  for (const candle of candles) {
    if (!candle) {
      continue
    }
    low = Math.min(low, candle.low)
    high = Math.max(high, candle.high)
  }

  if (!Number.isFinite(low) || !Number.isFinite(high)) {
    return { lo: -1, hi: 1 }
  }

  const candleRange = high - low
  const padding = candleRange > 0 ? candleRange * Y_PADDING : 1
  const targetRange = candleRange + padding * 2
  // The chart engine adds 6% of its input range on each side. Compensate here
  // so the final visible range has exactly the specified 5% padding.
  const engineRange = targetRange / (1 + ENGINE_PRICE_PADDING * 2)
  const center = (low + high) / 2
  return {
    lo: center - engineRange / 2,
    hi: center + engineRange / 2
  }
}

export function createBidAskFlowSeries(): SeriesDefinition<
  DeltaBar,
  BidAskFlowCandle | null,
  FlowParams
> {
  const params: FlowParams = {
    style: {
      value: 'CANDLE',
      affectsCompute: false,
      options: [
        { label: 'Velas', value: 'CANDLE' },
        { label: 'OHLC', value: 'OHLC_BAR' }
      ]
    }
  }

  return {
    id: 'bid-ask-flow-ohlc',
    label: 'FLUJO BID/ASK',
    color: UP_COLOR,
    layer: 'foreground',
    width: '100%',
    height: '100%',
    params,

    compute(data) {
      const candlesByTime = new Map(
        buildBidAskFlowSeries(data).map(candle => [candle.time, candle])
      )
      return data.map(bar => candlesByTime.get(bar.time) ?? null)
    },

    updateIncremental(data, values, isNewBar) {
      const bar = data.at(-1)
      if (!bar) {
        return
      }

      const priorValues = isNewBar ? values : values.slice(0, -1)
      const candle = buildBidAskFlowCandle(bar, previousFlowClose(priorValues))
      if (isNewBar) {
        values.push(candle)
      } else {
        values[values.length - 1] = candle
      }
    },

    render(
      ctx,
      pane,
      engine,
      _data,
      values,
      seriesParams,
      priceMin,
      priceMax
    ) {
      if (priceMin < 0 && priceMax > 0) {
        const zeroY = engine.utils.yOf(0, pane, priceMin, priceMax)
        ctx.save()
        ctx.setLineDash([4, 4])
        ctx.strokeStyle = 'rgba(148, 163, 184, 0.65)'
        ctx.lineWidth = 1
        ctx.beginPath()
        ctx.moveTo(0, zeroY)
        ctx.lineTo(engine.chartW, zeroY)
        ctx.stroke()
        ctx.restore()
      }

      const candleWidth = Math.max(
        1,
        Math.floor(engine.barWidth * FLOW_BAR_WIDTH_RATIO)
      )
      const halfWidth = candleWidth / 2

      for (
        let index = engine.viewStart;
        index < engine.viewEnd && index < values.length;
        index++
      ) {
        const candle = values[index]
        if (!candle) {
          continue
        }

        const x = Math.round(engine.utils.xOf(index)) + 0.5
        const yHigh = engine.utils.yOf(candle.high, pane, priceMin, priceMax)
        const yLow = engine.utils.yOf(candle.low, pane, priceMin, priceMax)
        const yOpen = engine.utils.yOf(candle.open, pane, priceMin, priceMax)
        const yClose = engine.utils.yOf(candle.close, pane, priceMin, priceMax)
        const color = candle.close >= candle.open ? UP_COLOR : DOWN_COLOR

        ctx.strokeStyle = color
        ctx.lineWidth = 1.5
        ctx.beginPath()
        ctx.moveTo(x, yHigh)
        ctx.lineTo(x, yLow)
        ctx.stroke()

        if (seriesParams.style.value === 'OHLC_BAR') {
          ctx.beginPath()
          ctx.moveTo(x - halfWidth, yOpen)
          ctx.lineTo(x, yOpen)
          ctx.moveTo(x, yClose)
          ctx.lineTo(x + halfWidth, yClose)
          ctx.stroke()
          continue
        }

        const bodyTop = Math.min(yOpen, yClose)
        const bodyHeight = Math.max(1.5, Math.abs(yClose - yOpen))
        ctx.fillStyle = color
        ctx.fillRect(x - halfWidth, bodyTop, candleWidth, bodyHeight)
      }
    },

    tooltipRow(values, index) {
      const candle = values[index]
      if (!candle) {
        return null
      }

      return {
        label: 'FLUJO BID/ASK',
        value: `O ${formatQuantity(candle.open)} · H ${formatQuantity(candle.high)} · L ${formatQuantity(candle.low)} · C ${formatQuantity(candle.close)} · ask ${formatQuantity(candle.askQty)} · bid ${formatQuantity(candle.bidQty)} · delta ${formatQuantity(candle.askQty - candle.bidQty)}`
      }
    },

    priceTags(_data, values) {
      const candle = [...values].reverse().find(
        (value): value is BidAskFlowCandle => value !== null
      )
      return candle
        ? [{
            value: candle.close,
            color: candle.close >= candle.open ? UP_COLOR : DOWN_COLOR,
            label: 'qty'
          }]
        : []
    },

    valueRange(_data, values, start, end) {
      return rangeForFlowCandles(values.slice(start, end))
    },

    legend(_data, values, index) {
      const candle = values[index]
      if (!candle) {
        return []
      }

      const color = candle.close >= candle.open ? UP_COLOR : DOWN_COLOR
      return [
        { label: 'O', value: formatQuantity(candle.open), color },
        { label: 'H', value: formatQuantity(candle.high), color },
        { label: 'L', value: formatQuantity(candle.low), color },
        { label: 'C', value: formatQuantity(candle.close), color },
        { label: 'Ask qty', value: formatQuantity(candle.askQty), color: UP_COLOR },
        { label: 'Bid qty', value: formatQuantity(candle.bidQty), color: DOWN_COLOR },
        {
          label: 'Delta',
          value: formatQuantity(candle.askQty - candle.bidQty),
          color
        }
      ]
    }
  }
}
