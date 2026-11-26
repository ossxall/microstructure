import type { ChartEngine } from '../packages/open-charts/src/core/ChartEngine'
import type { MainPane, SeriesDefinition } from '../packages/open-charts/src/core/types'
import type { Bar } from './bar'

const BULL_COLOR = '#22c55e'
const BEAR_COLOR = '#ef4444'
const DOUBTFUL_COLOR = '#f59e0b'
const U64_MAX = (1n << 64n) - 1n
const U128_MAX = (1n << 128n) - 1n
const I64_MIN = -(1n << 63n)
const I64_MAX = (1n << 63n) - 1n
const RATIO_PRECISION = 1_000_000_000_000_000n

export type TimestampUnit = 's' | 'ms' | 'us' | 'ns'

export interface MomentumOptions {
  signalPeriod: number
  basePeriod: number
  alpha: number
  maxIntensity: number
  warmup: number
  timestampUnit: TimestampUnit
}

export const DEFAULT_MOMENTUM_OPTIONS: Readonly<MomentumOptions> = {
  signalPeriod: 14,
  basePeriod: 100,
  alpha: 0.5,
  maxIntensity: 3,
  warmup: 30,
  timestampUnit: 'ms'
}

export interface MomentumPoint {
  time: number
  close: bigint
  bullish: number | null
  bearish: number | null
  doubtful: boolean
}

interface PreparedMomentumBar {
  bar: Bar
  line: number
  seriesKey: string
  identityKey: string
  revision: bigint
  timestamp: bigint
  time: number
  close: bigint
  buyNotional: bigint
  sellNotional: bigint
  positiveChange: bigint
  negativeChange: bigint
  doubtful: boolean
}

interface NumericParameter {
  value: number
  affectsCompute: true
  min: number
  max: number
  step: number
}

interface MomentumSeriesParams {
  signalPeriod: NumericParameter
  basePeriod: NumericParameter
  alpha: NumericParameter
  maxIntensity: NumericParameter
  warmup: NumericParameter
  timestampUnit: {
    value: TimestampUnit
    affectsCompute: true
    options: readonly { label: string, value: TimestampUnit }[]
  }
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null
    ? value as Record<string, unknown>
    : null
}

function requiredString(
  value: unknown,
  field: string,
  line: number
): string {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`Campo obligatorio ausente o inválido: ${field}, línea ${line}`)
  }
  return value
}

function integer(
  value: unknown,
  field: string,
  line: number,
  min: bigint,
  max: bigint
): bigint {
  let result: bigint
  try {
    if (typeof value === 'bigint') {
      result = value
    } else if (typeof value === 'number' && Number.isSafeInteger(value)) {
      result = BigInt(value)
    } else if (typeof value === 'string' && /^-?\d+$/.test(value.trim())) {
      result = BigInt(value.trim())
    } else {
      throw new Error()
    }
  } catch {
    throw new Error(`Campo obligatorio ausente o entero inválido: ${field}, línea ${line}`)
  }

  if (result < min || result > max) {
    throw new Error(`Entero fuera de rango en ${field}, línea ${line}`)
  }
  return result
}

function optionalU64(value: unknown, field: string, line: number): bigint {
  return value === undefined || value === null
    ? 0n
    : integer(value, field, line, 0n, U64_MAX)
}

function unitMilliseconds(unit: TimestampUnit): number {
  switch (unit) {
    case 's': return 1_000
    case 'ms': return 1
    case 'us': return 0.001
    case 'ns': return 0.000_001
  }
}

function seriesKey(source: string, symbol: string, timeframe: string): string {
  return JSON.stringify([source, symbol, timeframe])
}

function identityKey(
  source: string,
  symbol: string,
  timeframe: string,
  barKey: string
): string {
  return JSON.stringify([source, symbol, timeframe, barKey])
}

function normalizeOptions(options: Partial<MomentumOptions> = {}): MomentumOptions {
  const validUnit = options.timestampUnit === 's'
    || options.timestampUnit === 'ms'
    || options.timestampUnit === 'us'
    || options.timestampUnit === 'ns'
  const finiteOr = (value: number | undefined, fallback: number) => (
    typeof value === 'number' && Number.isFinite(value) ? value : fallback
  )

  return {
    signalPeriod: Math.max(1, Math.min(500, Math.round(
      finiteOr(options.signalPeriod, DEFAULT_MOMENTUM_OPTIONS.signalPeriod)
    ))),
    basePeriod: Math.max(1, Math.min(2_000, Math.round(
      finiteOr(options.basePeriod, DEFAULT_MOMENTUM_OPTIONS.basePeriod)
    ))),
    alpha: Math.max(0, Math.min(1, finiteOr(
      options.alpha,
      DEFAULT_MOMENTUM_OPTIONS.alpha
    ))),
    maxIntensity: Math.max(0, Math.min(100, finiteOr(
      options.maxIntensity,
      DEFAULT_MOMENTUM_OPTIONS.maxIntensity
    ))),
    warmup: Math.max(0, Math.min(2_000, Math.round(
      finiteOr(options.warmup, DEFAULT_MOMENTUM_OPTIONS.warmup)
    ))),
    timestampUnit: validUnit
      ? options.timestampUnit!
      : DEFAULT_MOMENTUM_OPTIONS.timestampUnit
  }
}

function prepareBars(
  bars: readonly Bar[],
  options: MomentumOptions
): PreparedMomentumBar[] {
  if (bars.length === 0) {
    throw new Error('La entrada está vacía: no hay barras para calcular momentum.')
  }

  const latestByIdentity = new Map<string, PreparedMomentumBar>()
  const millisecondsPerUnit = unitMilliseconds(options.timestampUnit)

  bars.forEach((bar, index) => {
    const line = index + 1
    const source = requiredString(bar.source, 'source', line)
    const symbol = requiredString(bar.symbol, 'symbol', line)
    const timeframe = requiredString(bar.timeframe, 'timeframe', line)
    const barKey = requiredString(bar.bar_key, 'bar_key', line)
    const revision = integer(
      bar.revision_raw ?? bar.revision,
      'revision',
      line,
      0n,
      U64_MAX
    )
    const timestamp = integer(bar.timestamp, 'timestamp', line, 0n, U64_MAX)
    const price = record(bar.price)
    const volumeFlow = record(bar.volume_flow)
    const priceChanges = record(bar.price_changes)
    const integrity = record(bar.integrity)

    if (!price || price.close === undefined) {
      throw new Error(`Campo obligatorio ausente: price.close, línea ${line}`)
    }
    if (!volumeFlow) {
      throw new Error(`Campo obligatorio ausente: volume_flow, línea ${line}`)
    }
    if (!priceChanges) {
      throw new Error(`Campo obligatorio ausente: price_changes, línea ${line}`)
    }

    const close = integer(price.close, 'price.close', line, I64_MIN, I64_MAX)
    const buyNotional = integer(
      volumeFlow.buy_notional,
      'volume_flow.buy_notional',
      line,
      0n,
      U128_MAX
    )
    const sellNotional = integer(
      volumeFlow.sell_notional,
      'volume_flow.sell_notional',
      line,
      0n,
      U128_MAX
    )
    const positiveChange = integer(
      priceChanges.positive_change_magnitude,
      'price_changes.positive_change_magnitude',
      line,
      0n,
      U128_MAX
    )
    const negativeChange = integer(
      priceChanges.negative_change_magnitude,
      'price_changes.negative_change_magnitude',
      line,
      -U128_MAX,
      U128_MAX
    )
    const timestampMilliseconds = Number(timestamp) * millisecondsPerUnit
    const timestampDate = new Date(timestampMilliseconds)
    const year = timestampDate.getUTCFullYear()
    if (
      !Number.isFinite(timestampMilliseconds)
      || !Number.isFinite(timestampDate.getTime())
      || year < 2000
      || year > 2100
    ) {
      throw new Error(
        `Fecha fuera del rango 2000–2100 en timestamp, línea ${line}; comprueba la unidad (${options.timestampUnit}).`
      )
    }

    const sequenceGaps = optionalU64(
      integrity?.sequence_gaps,
      'integrity.sequence_gaps',
      line
    )
    const droppedTrades = optionalU64(
      integrity?.dropped_trade_count,
      'integrity.dropped_trade_count',
      line
    )
    const invalidTrades = optionalU64(
      integrity?.invalid_trade_count,
      'integrity.invalid_trade_count',
      line
    )
    const prepared: PreparedMomentumBar = {
      bar,
      line: index,
      seriesKey: seriesKey(source, symbol, timeframe),
      identityKey: identityKey(source, symbol, timeframe, barKey),
      revision,
      timestamp,
      time: timestampMilliseconds / 1_000,
      close,
      buyNotional,
      sellNotional,
      positiveChange,
      negativeChange: negativeChange < 0n ? -negativeChange : negativeChange,
      doubtful: sequenceGaps > 0n || droppedTrades > 0n || invalidTrades > 0n
    }
    const current = latestByIdentity.get(prepared.identityKey)
    if (!current || prepared.revision >= current.revision) {
      // Iteration is in arrival order, so an equal revision keeps the last bar.
      latestByIdentity.set(prepared.identityKey, prepared)
    }
  })

  return [...latestByIdentity.values()].sort((left, right) => (
    left.seriesKey.localeCompare(right.seriesKey)
    || (left.timestamp < right.timestamp ? -1 : left.timestamp > right.timestamp ? 1 : 0)
    || left.line - right.line
  ))
}

/** Convert a signed integer ratio only after exact BigInt subtraction/addition. */
function integerRatio(numerator: bigint, denominator: bigint): number {
  if (denominator === 0n) {
    return 0
  }
  const scaled = (numerator * RATIO_PRECISION) / denominator
  return Number(scaled) / Number(RATIO_PRECISION)
}

function calculatePrepared(
  prepared: readonly PreparedMomentumBar[],
  options: MomentumOptions
): Map<number, MomentumPoint> {
  const bySeries = new Map<string, PreparedMomentumBar[]>()
  for (const bar of prepared) {
    const group = bySeries.get(bar.seriesKey) ?? []
    group.push(bar)
    bySeries.set(bar.seriesKey, group)
  }

  const result = new Map<number, MomentumPoint>()
  const signalK = 2 / (options.signalPeriod + 1)
  const baseK = 2 / (options.basePeriod + 1)

  for (const group of bySeries.values()) {
    let base: number | null = null
    let bullish: number | null = null
    let bearish: number | null = null

    group.forEach((bar, index) => {
      const totalBig = bar.buyNotional + bar.sellNotional
      const changesTotal = bar.positiveChange + bar.negativeChange
      const imbalance = totalBig > 0n
        ? integerRatio(bar.buyNotional - bar.sellNotional, totalBig)
        : 0
      const efficiency = changesTotal > 0n
        ? integerRatio(bar.positiveChange - bar.negativeChange, changesTotal)
        : 0
      const total = Number(totalBig)
      const reference = base !== null && base > 0 ? base : total
      const intensity = reference > 0
        ? Math.min(Math.sqrt(total / reference), options.maxIntensity)
        : 0
      base = base === null
        ? total
        : base + baseK * (total - base)

      const up = intensity * (
        options.alpha * Math.max(imbalance, 0)
        + (1 - options.alpha) * Math.max(efficiency, 0)
      )
      const down = intensity * (
        options.alpha * Math.max(-imbalance, 0)
        + (1 - options.alpha) * Math.max(-efficiency, 0)
      )
      bullish = bullish === null ? up : bullish + signalK * (up - bullish)
      bearish = bearish === null ? down : bearish + signalK * (down - bearish)

      result.set(bar.line, {
        time: bar.time,
        close: bar.close,
        bullish: index >= options.warmup ? bullish : null,
        bearish: index >= options.warmup ? bearish : null,
        doubtful: bar.doubtful
      })
    })
  }

  return result
}

/**
 * Calculates the independent momentum state for each source/symbol/timeframe.
 * Duplicate bar identities keep the highest revision; equal revisions use
 * the last bar in arrival order.
 */
export function calculateMomentumSeries(
  bars: readonly Bar[],
  inputOptions: Partial<MomentumOptions> = {}
): MomentumPoint[] {
  const options = normalizeOptions(inputOptions)
  const prepared = prepareBars(bars, options)
  const byLine = calculatePrepared(prepared, options)
  return prepared.flatMap((bar) => {
    const point = byLine.get(bar.line)
    return point ? [point] : []
  })
}

/** Computes values aligned with the provided source rows; superseded revisions are null. */
export function calculateMomentumValues(
  bars: readonly Bar[],
  inputOptions: Partial<MomentumOptions> = {}
): Array<MomentumPoint | null> {
  if (bars.length === 0) {
    return []
  }
  const options = normalizeOptions(inputOptions)
  const prepared = prepareBars(bars, options)
  const byLine = calculatePrepared(prepared, options)
  return bars.map((_bar, index) => byLine.get(index) ?? null)
}

function readOptions(params: MomentumSeriesParams): MomentumOptions {
  return normalizeOptions({
    signalPeriod: params.signalPeriod.value,
    basePeriod: params.basePeriod.value,
    alpha: params.alpha.value,
    maxIntensity: params.maxIntensity.value,
    warmup: params.warmup.value,
    timestampUnit: params.timestampUnit.value
  })
}

function numberParameter(value: number, min: number, max: number, step: number): NumericParameter {
  return { value, affectsCompute: true, min, max, step }
}

function createMomentumParams(): MomentumSeriesParams {
  return {
    signalPeriod: numberParameter(14, 1, 500, 1),
    basePeriod: numberParameter(100, 1, 2_000, 1),
    alpha: numberParameter(0.5, 0, 1, 0.05),
    maxIntensity: numberParameter(3, 0, 100, 0.1),
    warmup: numberParameter(30, 0, 2_000, 1),
    timestampUnit: {
      value: 'ms',
      affectsCompute: true,
      options: [
        { label: 's', value: 's' },
        { label: 'ms', value: 'ms' },
        { label: 'us', value: 'us' },
        { label: 'ns', value: 'ns' }
      ]
    }
  }
}

function format(value: number): string {
  return new Intl.NumberFormat('es-ES', {
    minimumFractionDigits: 0,
    maximumFractionDigits: 6
  }).format(value)
}

function lastPoint(values: readonly (MomentumPoint | null)[]): MomentumPoint | null {
  for (let index = values.length - 1; index >= 0; index--) {
    const point = values[index]
    if (point && point.bullish !== null && point.bearish !== null) {
      return point
    }
  }
  return null
}

function valueRange(values: readonly (MomentumPoint | null)[], start: number, end: number) {
  let high = 0
  for (let index = start; index < end; index++) {
    const point = values[index]
    if (point?.bullish !== null && point?.bullish !== undefined) {
      high = Math.max(high, point.bullish)
    }
    if (point?.bearish !== null && point?.bearish !== undefined) {
      high = Math.max(high, point.bearish)
    }
  }
  return { lo: 0, hi: Math.max(1, high * 1.08) }
}

function drawLine(
  ctx: CanvasRenderingContext2D,
  pane: MainPane,
  engine: ChartEngine,
  values: readonly (MomentumPoint | null)[],
  side: 'bullish' | 'bearish',
  color: string,
  dashed: boolean,
  priceMin: number,
  priceMax: number
) {
  ctx.save()
  ctx.beginPath()
  ctx.strokeStyle = color
  ctx.lineWidth = 2
  ctx.setLineDash(dashed ? [6, 4] : [])
  let connected = false
  for (
    let index = engine.viewStart;
    index < engine.viewEnd && index < values.length;
    index++
  ) {
    const point = values[index]
    const value = point?.[side]
    if (value === null || value === undefined) {
      connected = false
      continue
    }
    const x = engine.utils.xOf(index)
    const y = engine.utils.yOf(value, pane, priceMin, priceMax)
    if (connected) {
      ctx.lineTo(x, y)
    } else {
      ctx.moveTo(x, y)
      connected = true
    }
  }
  ctx.stroke()
  ctx.restore()
}

export function createMomentumSeries(): SeriesDefinition<
  Bar,
  MomentumPoint | null,
  MomentumSeriesParams
> {
  const params = createMomentumParams()
  return {
    id: 'bullish-bearish-momentum',
    label: 'MOMENTUM ALCISTA/BAJISTA',
    color: BULL_COLOR,
    layer: 'foreground',
    width: '100%',
    height: '100%',
    params,

    compute(data, seriesParams) {
      try {
        return calculateMomentumValues(data, readOptions(seriesParams))
      } catch {
        return data.map(() => null)
      }
    },

    render(ctx, pane, engine, _data, values, _params, priceMin, priceMax) {
      drawLine(ctx, pane, engine, values, 'bullish', BULL_COLOR, false, priceMin, priceMax)
      drawLine(ctx, pane, engine, values, 'bearish', BEAR_COLOR, true, priceMin, priceMax)

      ctx.save()
      ctx.fillStyle = DOUBTFUL_COLOR
      for (
        let index = engine.viewStart;
        index < engine.viewEnd && index < values.length;
        index++
      ) {
        if (!values[index]?.doubtful) {
          continue
        }
        const x = Math.round(engine.utils.xOf(index))
        ctx.fillRect(x - 2, pane.h - 7, 4, 4)
      }
      ctx.restore()
    },

    tooltipRow(values, index) {
      const point = values[index]
      if (!point) {
        return null
      }
      const bullish = point.bullish === null ? 'NULO' : format(point.bullish)
      const bearish = point.bearish === null ? 'NULO' : format(point.bearish)
      const integrity = point.doubtful ? ' · integridad dudosa' : ''
      return {
        label: 'MOMENTUM',
        value: `Alcista ${bullish} · Bajista ${bearish} · Cierre escalado ${point.close.toString()}${integrity}`
      }
    },

    priceTags(_data, values) {
      const point = lastPoint(values)
      if (!point || point.bullish === null || point.bearish === null) {
        return []
      }
      return [
        { value: point.bullish, color: BULL_COLOR, label: 'Alcista' },
        { value: point.bearish, color: BEAR_COLOR, label: 'Bajista' }
      ]
    },

    valueRange(_data, values, start, end) {
      return valueRange(values, start, end)
    },

    legend(_data, values, index) {
      const point = values[index]
      if (!point) {
        return []
      }
      return [
        {
          label: 'Momentum alcista',
          value: point.bullish === null ? 'NULO' : format(point.bullish),
          color: BULL_COLOR
        },
        {
          label: 'Momentum bajista',
          value: point.bearish === null ? 'NULO' : format(point.bearish),
          color: BEAR_COLOR
        },
        ...(point.doubtful
          ? [{ label: 'Integridad', value: 'dudosa', color: DOUBTFUL_COLOR }]
          : [])
      ]
    }
  }
}

export function normalizeMomentumOptions(
  options: Partial<MomentumOptions>
): MomentumOptions {
  return normalizeOptions(options)
}
