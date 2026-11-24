import type { ChartEngine } from '../packages/open-charts/src/core/ChartEngine'
import type { MainPane, SeriesDefinition } from '../packages/open-charts/src/core/types'
import type { Bar, BarFootprintLevel } from './bar'

const MAX_RETAINED_BARS = 500
const MAX_DIAGNOSTICS = 1_000
const MINUTE_MS = 60_000
const ZERO = 0n
const COVERAGE_UNKNOWN_SIDE_VALUE = 1 << 4

export type DeltaProfileScaleMode = 'linear' | 'logarithmic'

export interface DeltaProfileOptions {
  rollingWindowMinutes: number
  valueAreaPercentage: number
  showBuyVolume: boolean
  showSellVolume: boolean
  showDelta: boolean
  showDeltaPercentage: boolean
  showPoc: boolean
  showValueArea: boolean
  showMaxPositiveDelta: boolean
  showMaxNegativeDelta: boolean
  showLabels: boolean
  scaleMode: DeltaProfileScaleMode
  tickSize: string
  includeFormingBar: boolean
  autoUpdate: boolean
}

export interface DeltaProfileLevel {
  /** Raw, integer price key at the source price scale. */
  priceRaw: bigint
  price: number
  buyQty: bigint
  sellQty: bigint
  totalQty: bigint
  delta: bigint
  deltaPercent: number
  buySellRatio: number | null
  buyCount: bigint
  sellCount: bigint
  totalCount: bigint
  buyNotional: bigint
  sellNotional: bigint
  signedNotional: bigint
}

export interface DeltaProfileSnapshot {
  symbol: string | null
  rollingWindowMinutes: number
  valueAreaPercentage: number
  priceScale: number
  quantityScale: number
  barsInWindow: number
  levels: DeltaProfileLevel[]
  totalBuyVolume: bigint
  totalSellVolume: bigint
  totalVolume: bigint
  netDelta: bigint
  deltaPercentage: number
  buySellRatio: number | null
  totalBuyCount: bigint
  totalSellCount: bigint
  totalCount: bigint
  totalBuyNotional: bigint
  totalSellNotional: bigint
  signedNotional: bigint
  pocPrice: number | null
  pocVolume: bigint
  valueAreaHigh: number | null
  valueAreaLow: number | null
  maxPositiveDeltaPrice: number | null
  maxPositiveDelta: bigint
  maxNegativeDeltaPrice: number | null
  maxNegativeDelta: bigint
  maxVolumePrice: number | null
  levelCount: number
}

interface RawLevel {
  price: bigint
  buyQty: bigint
  sellQty: bigint
  buyCount: bigint
  sellCount: bigint
  buyNotional: bigint
  sellNotional: bigint
}

interface StoredBar {
  key: string
  timestamp: number
  revision: bigint
  source: string
  symbol: string
  isFinal: boolean
  priceScale: number
  quantityScale: number
  levels: RawLevel[]
}

interface MutableLevel {
  buyQty: bigint
  sellQty: bigint
  buyCount: bigint
  sellCount: bigint
  buyNotional: bigint
  sellNotional: bigint
}

type DiagnosticHandler = (message: string) => void

const DEFAULT_OPTIONS: DeltaProfileOptions = {
  rollingWindowMinutes: 15,
  valueAreaPercentage: 70,
  showBuyVolume: true,
  showSellVolume: true,
  showDelta: true,
  showDeltaPercentage: false,
  showPoc: true,
  showValueArea: true,
  showMaxPositiveDelta: true,
  showMaxNegativeDelta: true,
  showLabels: true,
  scaleMode: 'linear',
  tickSize: '50',
  includeFormingBar: true,
  autoUpdate: true
}

function integer(value: unknown): bigint | null {
  if (typeof value === 'bigint') {
    return value
  }

  if (typeof value === 'number') {
    return Number.isSafeInteger(value) ? BigInt(value) : null
  }

  if (typeof value === 'string' && /^-?\d+$/.test(value.trim())) {
    try {
      return BigInt(value.trim())
    } catch {
      return null
    }
  }

  return null
}

function finiteNumber(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value
  }

  if (typeof value === 'string' && value.trim() !== '') {
    const parsed = Number(value)
    return Number.isFinite(parsed) ? parsed : null
  }

  return null
}

function decimalToScaled(value: string, scale: number): bigint | null {
  const normalized = value.trim()
  const match = /^(\d+)(?:\.(\d+))?$/.exec(normalized)

  if (!match) {
    return null
  }

  const whole = match[1] ?? '0'
  const fraction = match[2] ?? ''
  const significantFraction = fraction.slice(0, scale)
  const excessFraction = fraction.slice(scale)

  if (/[1-9]/.test(excessFraction)) {
    return null
  }

  const paddedFraction = significantFraction.padEnd(scale, '0')
  return BigInt(whole) * 10n ** BigInt(scale)
    + BigInt(paddedFraction || '0')
}

function floorDivide(value: bigint, divisor: bigint): bigint {
  const quotient = value / divisor
  const remainder = value % divisor
  return remainder < 0n ? quotient - 1n : quotient
}

function ratio(numerator: bigint, denominator: bigint): number {
  if (denominator === ZERO) {
    return 0
  }

  const precision = 1_000_000n
  return Number((numerator * precision) / denominator) / Number(precision)
}

function percent(numerator: bigint, denominator: bigint): number {
  return ratio(numerator, denominator) * 100
}

function displayPrice(raw: bigint, scale: number): number {
  return Number(raw) / 10 ** scale
}

function displayQuantity(raw: bigint, scale: number): number {
  return Number(raw) / 10 ** scale
}

function formatRaw(raw: bigint, scale: number, digits = 3): string {
  const value = displayQuantity(raw, scale)

  if (!Number.isFinite(value)) {
    return raw.toString()
  }

  return new Intl.NumberFormat('en-US', {
    maximumFractionDigits: digits,
    minimumFractionDigits: 0
  }).format(value)
}

function formatPrice(value: number, scale: number): string {
  const decimals = Math.min(Math.max(scale, 2), 8)
  return new Intl.NumberFormat('en-US', {
    maximumFractionDigits: decimals,
    minimumFractionDigits: Math.min(decimals, 2)
  }).format(value)
}

function formatSigned(value: bigint, scale: number): string {
  const sign = value > ZERO ? '+' : value < ZERO ? '−' : ''
  const magnitude = value < ZERO ? -value : value
  return `${sign}${formatRaw(magnitude, scale)}`
}

function validCount(value: unknown): bigint | null {
  const parsed = integer(value)
  return parsed !== null && parsed >= ZERO ? parsed : null
}

function sameLevels(left: readonly RawLevel[], right: readonly RawLevel[]): boolean {
  return left.length === right.length && left.every((level, index) => {
    const other = right[index]
    return Boolean(
      other
      && level.price === other.price
      && level.buyQty === other.buyQty
      && level.sellQty === other.sellQty
      && level.buyCount === other.buyCount
      && level.sellCount === other.sellCount
      && level.buyNotional === other.buyNotional
      && level.sellNotional === other.sellNotional
    )
  })
}

function sameStoredContribution(left: StoredBar, right: StoredBar): boolean {
  return left.revision === right.revision
    && left.timestamp === right.timestamp
    && left.source === right.source
    && left.symbol === right.symbol
    && left.priceScale === right.priceScale
    && left.quantityScale === right.quantityScale
    && left.isFinal === right.isFinal
    && sameLevels(left.levels, right.levels)
}

/**
 * Maintains the rolling aggregate independently of the chart timeframe.
 *
 * The engine stores only 1m footprint levels; it never reads trades/raw_ticks.
 * Bar contributions are added/removed as the active window changes, while
 * revisions replace the prior contribution before the new one is applied.
 */
export class RollingDeltaProfile {
  private readonly bars = new Map<string, StoredBar>()
  private readonly activeBars = new Map<string, StoredBar>()
  private readonly levels = new Map<bigint, MutableLevel>()
  private readonly reportedDiagnostics = new Set<string>()
  private symbol: string | null = null
  private source: string | null = null
  private priceScale: number | null = null
  private quantityScale: number | null = null
  private options: DeltaProfileOptions
  private readonly onDiagnostic?: DiagnosticHandler

  constructor(
    options: Partial<DeltaProfileOptions> = {},
    onDiagnostic?: DiagnosticHandler
  ) {
    this.onDiagnostic = onDiagnostic
    this.options = { ...DEFAULT_OPTIONS, ...options }
    this.options.rollingWindowMinutes = this.normalizeWindow(
      this.options.rollingWindowMinutes
    )
    this.options.valueAreaPercentage = this.normalizeValueArea(
      this.options.valueAreaPercentage
    )
  }

  get rollingWindowMinutes(): number {
    return this.options.rollingWindowMinutes
  }

  get retainedBarCount(): number {
    return this.bars.size
  }

  get activeBarCount(): number {
    return this.activeBars.size
  }

  /**
   * Clears data when the requested instrument changes.
   */
  reset(symbol: string | null = null, source: string | null = null): void {
    this.bars.clear()
    this.activeBars.clear()
    this.levels.clear()
    this.reportedDiagnostics.clear()
    this.symbol = symbol
    this.source = source
    this.priceScale = null
    this.quantityScale = null
  }

  configure(patch: Partial<DeltaProfileOptions>): void {
    const previousWindow = this.options.rollingWindowMinutes
    const previousIncludeForming = this.options.includeFormingBar
    const previousTickSize = this.options.tickSize

    this.options = {
      ...this.options,
      ...patch,
      rollingWindowMinutes: this.normalizeWindow(
        patch.rollingWindowMinutes ?? this.options.rollingWindowMinutes
      ),
      valueAreaPercentage: this.normalizeValueArea(
        patch.valueAreaPercentage ?? this.options.valueAreaPercentage
      )
    }

    if (
      previousWindow !== this.options.rollingWindowMinutes
      || previousIncludeForming !== this.options.includeFormingBar
      || previousTickSize !== this.options.tickSize
    ) {
      this.syncActiveBars(true)
    }
  }

  /**
   * Upserts a 1m bar by key/revision while allowing live updates and finality
   * transitions that can legitimately share the same revision.
   */
  upsert(bar: Bar): boolean {
    const parsed = this.parseBar(bar)

    if (!parsed) {
      return false
    }

    if (this.symbol && parsed.symbol !== this.symbol) {
      this.diagnose(
        `symbol:${parsed.symbol}`,
        `Delta Profile ignoró la Bar de ${parsed.symbol}; el instrumento activo es ${this.symbol}.`
      )
      return false
    }

    if (this.source && parsed.source && parsed.source !== this.source) {
      this.diagnose(
        `source:${parsed.source}`,
        `Delta Profile ignoró la Bar de ${parsed.source}; la fuente activa es ${this.source}.`
      )
      return false
    }

    if (this.priceScale !== null && parsed.priceScale !== this.priceScale) {
      this.diagnose(
        `price-scale:${parsed.priceScale}`,
        `Delta Profile ignoró una Bar con price_scale=${parsed.priceScale}; se esperaba ${this.priceScale}.`
      )
      return false
    }

    if (this.quantityScale !== null && parsed.quantityScale !== this.quantityScale) {
      this.diagnose(
        `quantity-scale:${parsed.quantityScale}`,
        `Delta Profile ignoró una Bar con quantity_scale=${parsed.quantityScale}; se esperaba ${this.quantityScale}.`
      )
      return false
    }

    const current = this.bars.get(parsed.key)

    if (current) {
      // A finalized bar is terminal for this timestamp; a delayed live event
      // must never roll the profile back, even if it carries a larger revision.
      if (current.isFinal && !parsed.isFinal) {
        return false
      }

      if (parsed.revision < current.revision) {
        return false
      }

      if (
        parsed.revision === current.revision
        && (
          current.isFinal
          || sameStoredContribution(current, parsed)
        )
      ) {
        return false
      }
    }

    if (!this.symbol) {
      this.symbol = parsed.symbol
    }
    if (!this.source && parsed.source) {
      this.source = parsed.source
    }
    if (this.priceScale === null) {
      this.priceScale = parsed.priceScale
    }
    if (this.quantityScale === null) {
      this.quantityScale = parsed.quantityScale
    }

    this.bars.set(parsed.key, parsed)
    this.pruneRetainedBars()
    this.syncActiveBars()
    return true
  }

  snapshot(valueAreaPercentage = this.options.valueAreaPercentage): DeltaProfileSnapshot {
    const sortedLevels = [...this.levels.entries()]
      .map(([priceRaw, level]): DeltaProfileLevel => {
        const buyQty = level.buyQty
        const sellQty = level.sellQty
        const totalQty = buyQty + sellQty
        const delta = buyQty - sellQty
        const buyCount = level.buyCount
        const sellCount = level.sellCount

        return {
          priceRaw,
          price: displayPrice(priceRaw, this.priceScale ?? 0),
          buyQty,
          sellQty,
          totalQty,
          delta,
          deltaPercent: totalQty === ZERO ? 0 : percent(delta, totalQty),
          buySellRatio: sellQty === ZERO ? null : ratio(buyQty, sellQty),
          buyCount,
          sellCount,
          totalCount: buyCount + sellCount,
          buyNotional: level.buyNotional,
          sellNotional: level.sellNotional,
          signedNotional: level.buyNotional - level.sellNotional
        }
      })
      .sort((left, right) => left.priceRaw < right.priceRaw ? -1 : left.priceRaw > right.priceRaw ? 1 : 0)

    let totalBuyVolume = ZERO
    let totalSellVolume = ZERO
    let totalBuyCount = ZERO
    let totalSellCount = ZERO
    let totalBuyNotional = ZERO
    let totalSellNotional = ZERO

    for (const level of sortedLevels) {
      totalBuyVolume += level.buyQty
      totalSellVolume += level.sellQty
      totalBuyCount += level.buyCount
      totalSellCount += level.sellCount
      totalBuyNotional += level.buyNotional
      totalSellNotional += level.sellNotional
    }

    const totalVolume = totalBuyVolume + totalSellVolume
    const netDelta = totalBuyVolume - totalSellVolume
    const pocIndex = this.findPocIndex(sortedLevels)
    const valueArea = this.findValueArea(
      sortedLevels,
      pocIndex,
      this.normalizeValueArea(valueAreaPercentage)
    )
    const positiveDelta = sortedLevels.filter(level => level.delta > ZERO)
    const negativeDelta = sortedLevels.filter(level => level.delta < ZERO)
    const maxPositive = positiveDelta.reduce<DeltaProfileLevel | null>(
      (best, level) => !best || level.delta > best.delta ? level : best,
      null
    )
    const maxNegative = negativeDelta.reduce<DeltaProfileLevel | null>(
      (best, level) => !best || level.delta < best.delta ? level : best,
      null
    )
    const poc = pocIndex >= 0 ? sortedLevels[pocIndex] ?? null : null

    return {
      symbol: this.symbol,
      rollingWindowMinutes: this.options.rollingWindowMinutes,
      valueAreaPercentage: this.normalizeValueArea(valueAreaPercentage),
      priceScale: this.priceScale ?? 0,
      quantityScale: this.quantityScale ?? 0,
      barsInWindow: this.activeBars.size,
      levels: sortedLevels,
      totalBuyVolume,
      totalSellVolume,
      totalVolume,
      netDelta,
      deltaPercentage: totalVolume === ZERO ? 0 : percent(netDelta, totalVolume),
      buySellRatio: totalSellVolume === ZERO
        ? null
        : ratio(totalBuyVolume, totalSellVolume),
      totalBuyCount,
      totalSellCount,
      totalCount: totalBuyCount + totalSellCount,
      totalBuyNotional,
      totalSellNotional,
      signedNotional: totalBuyNotional - totalSellNotional,
      pocPrice: poc?.price ?? null,
      pocVolume: poc?.totalQty ?? ZERO,
      valueAreaHigh: valueArea?.high ?? null,
      valueAreaLow: valueArea?.low ?? null,
      maxPositiveDeltaPrice: maxPositive?.price ?? null,
      maxPositiveDelta: maxPositive?.delta ?? ZERO,
      maxNegativeDeltaPrice: maxNegative?.price ?? null,
      maxNegativeDelta: maxNegative?.delta ?? ZERO,
      maxVolumePrice: poc?.price ?? null,
      levelCount: sortedLevels.length
    }
  }

  private normalizeWindow(value: number): number {
    return Number.isFinite(value) ? Math.min(240, Math.max(1, Math.round(value))) : 15
  }

  private normalizeValueArea(value: number): number {
    return Number.isFinite(value) ? Math.min(100, Math.max(1, value)) : 70
  }

  private diagnose(key: string, message: string): void {
    if (this.reportedDiagnostics.has(key)) {
      return
    }

    if (this.reportedDiagnostics.size >= MAX_DIAGNOSTICS) {
      const oldest = this.reportedDiagnostics.values().next().value
      if (oldest !== undefined) {
        this.reportedDiagnostics.delete(oldest)
      }
    }

    this.reportedDiagnostics.add(key)
    this.onDiagnostic?.(message)
  }

  private parseBar(bar: Bar): StoredBar | null {
    if (!bar || typeof bar !== 'object') {
      return null
    }

    const timeframe = typeof bar.timeframe === 'string' ? bar.timeframe : ''
    const eventType = typeof bar.event_type === 'string' ? bar.event_type : ''

    if (!timeframe && !eventType) {
      this.diagnose(
        `missing-timeframe:${bar.bar_key ?? bar.time}`,
        'Delta Profile ignoró una Bar sin timeframe/event_type verificable.'
      )
      return null
    }

    if ((timeframe && timeframe !== '1m') || (eventType && eventType !== 'bar.1m')) {
      this.diagnose(
        `timeframe:${timeframe || eventType}`,
        `Delta Profile requiere Bars 1m; se recibió ${timeframe || eventType}.`
      )
      return null
    }

    const source = typeof bar.source === 'string' ? bar.source.trim() : ''
    const symbol = typeof bar.symbol === 'string' ? bar.symbol.trim() : ''
    const timestamp = finiteNumber(bar.timestamp)
    const revision = integer(bar.revision)
    const priceScale = integer(bar.header?.price_scale)
    const quantityScale = integer(bar.header?.quantity_scale)

    if (
      !symbol
      || timestamp === null
      || revision === null
      || revision < ZERO
      || priceScale === null
      || quantityScale === null
      || priceScale < ZERO
      || priceScale > 18n
      || quantityScale < ZERO
      || quantityScale > 18n
    ) {
      this.diagnose(
        `metadata:${symbol}:${bar.time}`,
        'Delta Profile ignoró una Bar con identidad, timestamp o escalas ausentes/no válidas.'
      )
      return null
    }

    const priceScaleNumber = Number(priceScale)
    const quantityScaleNumber = Number(quantityScale)
    const footprint = bar.footprint
    const sourceLevels = Array.isArray(footprint?.levels) ? footprint.levels : []
    const levels = this.parseLevels(sourceLevels, priceScaleNumber)

    if (!levels) {
      return null
    }

    const key = typeof bar.bar_key === 'string' && bar.bar_key.trim()
      ? bar.bar_key
      : `${symbol}:1m:${Math.trunc(timestamp)}`

    if (!bar.bar_key) {
      this.diagnose(
        `missing-key:${symbol}`,
        'Delta Profile recibió una Bar sin bar_key; usará símbolo y timestamp para deduplicarla.'
      )
    }

    if (
      Number.isFinite(footprint?.level_count)
      && footprint?.level_count !== sourceLevels.length
    ) {
      this.diagnose(
        `level-count:${key}:${revision}`,
        `La Bar ${key} declara level_count=${footprint?.level_count}, pero contiene ${sourceLevels.length} niveles.`
      )
    }

    const sequenceContiguous = finiteNumber(bar.identity?.sequence_contiguous)
    if (sequenceContiguous === 0) {
      this.diagnose(
        `sequence:${key}:${revision}`,
        `La identidad de la Bar ${key} indica que la secuencia de trades no es contigua.`
      )
    }

    const coverageFlags = finiteNumber(bar.identity?.coverage_flags)
    if (coverageFlags !== null && coverageFlags !== 0) {
      this.diagnose(
        `coverage:${key}:${revision}`,
        `La Bar ${key} declara coverage_flags=${coverageFlags}; el perfil usa los niveles disponibles.`
      )
    }

    this.validateVolumeFlow(key, revision, levels, bar)

    if (sourceLevels.length === 0) {
      const total = integer(bar.volume_flow?.total_qty)
      if (total !== null && total !== ZERO) {
        this.diagnose(
          `empty-footprint:${key}:${revision}`,
          `La Bar ${key} tiene volumen en volume_flow pero no contiene niveles footprint; no aporta al perfil.`
        )
      }
    }

    return {
      key,
      timestamp: Math.trunc(timestamp),
      revision,
      source,
      symbol,
      isFinal: bar.is_final !== false,
      priceScale: priceScaleNumber,
      quantityScale: quantityScaleNumber,
      levels
    }
  }

  private parseLevels(levels: readonly BarFootprintLevel[], priceScale: number): RawLevel[] | null {
    const parsed: RawLevel[] = []

    for (const level of levels) {
      const price = integer(level.price)
      const buyQty = integer(level.buy_qty)
      const sellQty = integer(level.sell_qty)
      const buyNotional = integer(level.buy_notional)
      const sellNotional = integer(level.sell_notional)
      const buyCount = validCount(level.buy_count)
      const sellCount = validCount(level.sell_count)
      const totalCount = validCount(level.total_count)
      const totalQty = integer(level.total_qty)
      const delta = integer(level.delta)

      if (
        price === null
        || buyQty === null
        || sellQty === null
        || buyNotional === null
        || sellNotional === null
        || buyCount === null
        || sellCount === null
        || totalCount === null
        || totalQty === null
        || delta === null
        || buyQty < ZERO
        || sellQty < ZERO
        || buyNotional < ZERO
        || sellNotional < ZERO
        || buyCount + sellCount !== totalCount
        || buyQty + sellQty !== totalQty
        || buyQty - sellQty !== delta
      ) {
        this.diagnose(
          `invalid-level:${priceScale}:${String(level.price)}`,
          `Delta Profile descartó una Bar con un nivel footprint incompleto o inconsistente (precio ${String(level.price)}).`
        )
        return null
      }

      parsed.push({
        price,
        buyQty,
        sellQty,
        buyCount,
        sellCount,
        buyNotional,
        sellNotional
      })
    }

    return parsed
  }

  private validateVolumeFlow(
    key: string,
    revision: bigint,
    levels: readonly RawLevel[],
    bar: Bar
  ): void {
    const flow = bar.volume_flow
    if (levels.length === 0) {
      return
    }

    const sums = levels.reduce(
      (result, level) => {
        result.buy += level.buyQty
        result.sell += level.sellQty
        result.buyNotional += level.buyNotional
        result.sellNotional += level.sellNotional
        result.buyCount += level.buyCount
        result.sellCount += level.sellCount
        return result
      },
      {
        buy: ZERO,
        sell: ZERO,
        buyNotional: ZERO,
        sellNotional: ZERO,
        buyCount: ZERO,
        sellCount: ZERO
      }
    )
    const coverageFlags = integer(bar.identity?.coverage_flags)
    const hasUnknownSide = coverageFlags !== null
      && (coverageFlags & BigInt(COVERAGE_UNKNOWN_SIDE_VALUE)) !== ZERO

    if (flow) {
      const expected: Array<[string, unknown, bigint, boolean?]> = [
        ['buy_qty', flow.buy_qty, sums.buy],
        ['sell_qty', flow.sell_qty, sums.sell],
        ['total_qty', flow.total_qty, sums.buy + sums.sell, hasUnknownSide],
        ['signed_qty', flow.signed_qty, sums.buy - sums.sell],
        ['buy_notional', flow.buy_notional, sums.buyNotional],
        ['sell_notional', flow.sell_notional, sums.sellNotional],
        [
          'total_notional',
          flow.total_notional,
          sums.buyNotional + sums.sellNotional,
          hasUnknownSide
        ],
        ['signed_notional', flow.signed_notional, sums.buyNotional - sums.sellNotional],
        ['buy_count', flow.buy_count, sums.buyCount],
        ['sell_count', flow.sell_count, sums.sellCount]
      ]

      for (const [name, value, actual, mayIncludeUnknownSide] of expected) {
        const supplied = integer(value)
        const inconsistent = supplied !== null && (
          mayIncludeUnknownSide ? supplied < actual : supplied !== actual
        )
        if (inconsistent) {
          this.diagnose(
            `flow:${key}:${revision}:${name}`,
            mayIncludeUnknownSide
              ? `La Bar ${key} declara volume_flow.${name} menor que el volumen de lado conocido del footprint; se conserva footprint como fuente del perfil.`
              : `La Bar ${key} no cuadra entre footprint y volume_flow.${name}; se conserva footprint como fuente del perfil.`
          )
        }
      }
    }

    const tradeCount = integer(bar.identity?.n_trades)
    const knownSideCount = sums.buyCount + sums.sellCount
    const inconsistentTradeCount = tradeCount !== null && (
      hasUnknownSide ? tradeCount < knownSideCount : tradeCount !== knownSideCount
    )
    if (inconsistentTradeCount) {
      this.diagnose(
        `trade-count:${key}:${revision}`,
        `La Bar ${key} no cuadra entre footprint y identity.n_trades; se conserva footprint como fuente del perfil.`
      )
    }
  }

  private pruneRetainedBars(): void {
    if (this.bars.size <= MAX_RETAINED_BARS) {
      return
    }

    const oldest = [...this.bars.values()]
      .sort((left, right) => left.timestamp - right.timestamp)
      .slice(0, this.bars.size - MAX_RETAINED_BARS)

    for (const bar of oldest) {
      this.bars.delete(bar.key)
    }
  }

  private syncActiveBars(forceRebuild = false): void {
    const eligible = [...this.bars.values()]
      .filter(bar => this.options.includeFormingBar || bar.isFinal)
      .sort((left, right) => left.timestamp - right.timestamp || left.key.localeCompare(right.key))

    // At most one Bar per minute can contribute. If duplicate bar_keys exist
    // for a timestamp, choose the highest revision; ties are resolved by key.
    const byTimestamp = new Map<number, StoredBar>()
    for (const bar of eligible) {
      const current = byTimestamp.get(bar.timestamp)
      if (current && current.key !== bar.key) {
        this.diagnose(
          `duplicate-time:${bar.timestamp}`,
          `Hay varias bar_key para el mismo timestamp ${bar.timestamp}; se usa la revisión más alta.`
        )
      }
      if (
        !current
        || bar.revision > current.revision
        || (bar.revision === current.revision && bar.key < current.key)
      ) {
        byTimestamp.set(bar.timestamp, bar)
      }
    }

    const uniqueBars = [...byTimestamp.values()].sort(
      (left, right) => left.timestamp - right.timestamp
    )
    const latestTimestamp = uniqueBars.at(-1)?.timestamp
    let nextBars: StoredBar[] = []

    if (latestTimestamp !== undefined) {
      const earliestTimestamp = latestTimestamp - (this.options.rollingWindowMinutes - 1) * MINUTE_MS
      nextBars = uniqueBars.filter(bar => bar.timestamp >= earliestTimestamp)
    }

    const nextActive = new Map(nextBars.map(bar => [bar.key, bar]))

    if (forceRebuild) {
      this.levels.clear()
      this.activeBars.clear()
      for (const bar of nextBars) {
        this.applyBar(bar, 1)
        this.activeBars.set(bar.key, bar)
      }
      return
    }

    for (const [key, current] of this.activeBars) {
      const next = nextActive.get(key)
      if (!next || !sameStoredContribution(current, next)) {
        this.applyBar(current, -1)
        this.activeBars.delete(key)
      }
    }

    for (const [key, next] of nextActive) {
      const current = this.activeBars.get(key)
      if (!current || !sameStoredContribution(current, next)) {
        this.applyBar(next, 1)
        this.activeBars.set(key, next)
      }
    }
  }

  private applyBar(bar: StoredBar, direction: 1 | -1): void {
    const tickSizeRaw = this.options.tickSize
      ? decimalToScaled(this.options.tickSize, bar.priceScale)
      : null

    if (this.options.tickSize && (!tickSizeRaw || tickSizeRaw <= ZERO)) {
      this.diagnose(
        `tick-size:${this.options.tickSize}:${bar.priceScale}`,
        `Tick Size "${this.options.tickSize}" no es representable con price_scale=${bar.priceScale}; se usa el precio original.`
      )
    }

    for (const contribution of bar.levels) {
      const price = tickSizeRaw && tickSizeRaw > ZERO
        ? floorDivide(contribution.price, tickSizeRaw) * tickSizeRaw
        : contribution.price
      const current = this.levels.get(price) ?? {
        buyQty: ZERO,
        sellQty: ZERO,
        buyCount: ZERO,
        sellCount: ZERO,
        buyNotional: ZERO,
        sellNotional: ZERO
      }
      const multiplier = BigInt(direction)

      current.buyQty += contribution.buyQty * multiplier
      current.sellQty += contribution.sellQty * multiplier
      current.buyCount += contribution.buyCount * multiplier
      current.sellCount += contribution.sellCount * multiplier
      current.buyNotional += contribution.buyNotional * multiplier
      current.sellNotional += contribution.sellNotional * multiplier

      if (
        current.buyQty === ZERO
        && current.sellQty === ZERO
        && current.buyCount === ZERO
        && current.sellCount === ZERO
        && current.buyNotional === ZERO
        && current.sellNotional === ZERO
      ) {
        this.levels.delete(price)
      } else {
        this.levels.set(price, current)
      }
    }
  }

  private findPocIndex(levels: readonly DeltaProfileLevel[]): number {
    let index = -1
    let maxVolume = ZERO

    for (let current = 0; current < levels.length; current++) {
      const level = levels[current]
      if (level && (index < 0 || level.totalQty > maxVolume)) {
        index = current
        maxVolume = level.totalQty
      }
      // Since rows are sorted low-to-high and ties do not replace the current
      // candidate, POC ties deterministically resolve to the lower price.
    }

    return index
  }

  private findValueArea(
    levels: readonly DeltaProfileLevel[],
    pocIndex: number,
    percentage: number
  ): { high: number, low: number } | null {
    const poc = levels[pocIndex]
    if (!poc || poc.totalQty === ZERO) {
      return null
    }

    const targetBasisPoints = BigInt(Math.round(percentage * 100))
    const total = levels.reduce((sum, level) => sum + level.totalQty, ZERO)
    const target = (total * targetBasisPoints + 9_999n) / 10_000n
    let included = poc.totalQty
    let lowIndex = pocIndex
    let highIndex = pocIndex

    while (included < target && (lowIndex > 0 || highIndex < levels.length - 1)) {
      const lower = lowIndex > 0 ? levels[lowIndex - 1] : undefined
      const upper = highIndex < levels.length - 1 ? levels[highIndex + 1] : undefined

      if (lower && (!upper || lower.totalQty > upper.totalQty)) {
        lowIndex--
        included += lower.totalQty
      } else if (upper) {
        highIndex++
        included += upper.totalQty
      }
    }

    const low = levels[lowIndex]
    const high = levels[highIndex]
    return low && high ? { high: high.price, low: low.price } : null
  }
}

function volumeScale(value: bigint, max: bigint, mode: DeltaProfileScaleMode): number {
  if (max <= ZERO || value <= ZERO) {
    return 0
  }

  if (mode === 'linear') {
    return ratio(value, max)
  }

  const current = Number(value)
  const maximum = Number(max)
  if (Number.isFinite(current) && Number.isFinite(maximum)) {
    return Math.log1p(current) / Math.log1p(maximum)
  }

  const currentText = value.toString()
  const maximumText = max.toString()
  return currentText.length / maximumText.length
}

function drawProfile(
  ctx: CanvasRenderingContext2D,
  pane: MainPane,
  engine: ChartEngine,
  snapshot: DeltaProfileSnapshot,
  options: DeltaProfileOptions,
  priceMin: number,
  priceMax: number
): void {
  if (snapshot.levels.length === 0 || pane.w < 120 || pane.h < 80) {
    return
  }

  const width = Math.min(340, Math.max(190, pane.w * 0.34), pane.w - 12)
  const x = Math.max(6, pane.w - width - 8)
  const top = 8
  const bottom = pane.h - 8
  const titleHeight = 44
  const contentTop = top + titleHeight
  const contentBottom = bottom - 4
  const panelHeight = bottom - top
  const volumeWidth = width * 0.43
  const deltaWidth = options.showDelta ? width * 0.27 : 0
  const deltaCenter = x + width - deltaWidth / 2 - 10
  const volumeX = x + 10
  const deltaMax = snapshot.levels.reduce(
    (max, level) => level.delta < ZERO
      ? (max > -level.delta ? max : -level.delta)
      : (max > level.delta ? max : level.delta),
    ZERO
  )
  const maxVolume = snapshot.levels.reduce(
    (max, level) => max > level.totalQty ? max : level.totalQty,
    ZERO
  )
  const visible = snapshot.levels.filter(
    level => level.price >= priceMin && level.price <= priceMax
  )

  ctx.save()
  ctx.fillStyle = 'rgba(10, 15, 23, 0.88)'
  ctx.strokeStyle = 'rgba(148, 163, 184, 0.3)'
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.roundRect(x, top, width, panelHeight, 7)
  ctx.fill()
  ctx.stroke()

  ctx.font = '600 10px ui-sans-serif, system-ui, sans-serif'
  ctx.fillStyle = '#e2e8f0'
  ctx.fillText('DELTA PROFILE', x + 10, top + 15)
  ctx.font = '10px ui-sans-serif, system-ui, sans-serif'
  ctx.fillStyle = '#94a3b8'
  ctx.fillText(
    `${snapshot.rollingWindowMinutes}m · ${snapshot.barsInWindow} Bars · VA ${snapshot.valueAreaPercentage}%`,
    x + 10,
    top + 30
  )
  const ratioLabel = snapshot.buySellRatio === null
    ? 'B/S —'
    : `B/S ${snapshot.buySellRatio.toFixed(2)}`
  ctx.textAlign = 'right'
  ctx.fillText(ratioLabel, x + width - 10, top + 30)
  ctx.textAlign = 'left'

  const summary = width < 250
    ? `Δ ${formatSigned(snapshot.netDelta, snapshot.quantityScale)}`
    : `Δ ${formatSigned(snapshot.netDelta, snapshot.quantityScale)} · V ${formatRaw(snapshot.totalVolume, snapshot.quantityScale)}`
  const summaryWidth = ctx.measureText(summary).width
  ctx.fillStyle = snapshot.netDelta >= ZERO ? '#34d399' : '#fb7185'
  ctx.fillText(summary, Math.max(x + 10, x + width - summaryWidth - 10), top + 15)

  if (options.showValueArea && snapshot.valueAreaHigh !== null && snapshot.valueAreaLow !== null) {
    const yHigh = engine.utils.yOf(snapshot.valueAreaHigh, pane, priceMin, priceMax)
    const yLow = engine.utils.yOf(snapshot.valueAreaLow, pane, priceMin, priceMax)
    ctx.fillStyle = 'rgba(234, 179, 8, 0.055)'
    ctx.fillRect(x + 1, Math.min(yHigh, yLow), width - 2, Math.max(1, Math.abs(yLow - yHigh)))
  }

  if (options.showDelta) {
    ctx.strokeStyle = 'rgba(148, 163, 184, 0.35)'
    ctx.beginPath()
    ctx.moveTo(deltaCenter, contentTop)
    ctx.lineTo(deltaCenter, contentBottom)
    ctx.stroke()
  }

  const yPositions = visible.map(level => engine.utils.yOf(level.price, pane, priceMin, priceMax))
  for (let index = 0; index < visible.length; index++) {
    const level = visible[index]
    const y = yPositions[index]
    if (!level || y === undefined) {
      continue
    }

    const adjacentGaps = [
      index > 0 ? Math.abs(y - (yPositions[index - 1] ?? y)) : Infinity,
      index < yPositions.length - 1 ? Math.abs((yPositions[index + 1] ?? y) - y) : Infinity
    ].filter(Number.isFinite)
    const rowHeight = Math.max(
      2,
      Math.min(14, adjacentGaps.length ? Math.min(...adjacentGaps) : 8)
    )
    const rowTop = Math.max(contentTop, y - rowHeight / 2)
    const clippedHeight = Math.max(1, Math.min(contentBottom, y + rowHeight / 2) - rowTop)

    if (options.showValueArea && snapshot.valueAreaLow !== null && snapshot.valueAreaHigh !== null) {
      if (level.price >= snapshot.valueAreaLow && level.price <= snapshot.valueAreaHigh) {
        ctx.fillStyle = 'rgba(234, 179, 8, 0.045)'
        ctx.fillRect(x + 1, rowTop, width - 2, clippedHeight)
      }
    }

    const volumeFraction = volumeScale(level.totalQty, maxVolume, options.scaleMode)
    const scaledVolumeWidth = volumeWidth * volumeFraction
    let currentX = volumeX

    if (options.showBuyVolume && level.buyQty > ZERO) {
      const buyWidth = Math.max(1, scaledVolumeWidth * ratio(level.buyQty, level.totalQty))
      ctx.fillStyle = 'rgba(16, 185, 129, 0.82)'
      ctx.fillRect(currentX, rowTop, Math.min(buyWidth, volumeWidth), clippedHeight)
      currentX += Math.min(buyWidth, volumeWidth)
    }

    if (options.showSellVolume && level.sellQty > ZERO) {
      const sellWidth = Math.max(1, scaledVolumeWidth * ratio(level.sellQty, level.totalQty))
      ctx.fillStyle = 'rgba(244, 63, 94, 0.78)'
      ctx.fillRect(
        currentX,
        rowTop,
        Math.min(sellWidth, Math.max(0, volumeWidth - (currentX - volumeX))),
        clippedHeight
      )
    }

    if (options.showDelta && level.delta !== ZERO && deltaWidth > 0) {
      const deltaFraction = volumeScale(
        level.delta < ZERO ? -level.delta : level.delta,
        deltaMax,
        options.scaleMode
      )
      const barWidth = Math.max(1, (deltaWidth / 2 - 3) * deltaFraction)
      ctx.fillStyle = level.delta > ZERO ? 'rgba(16, 185, 129, 0.95)' : 'rgba(244, 63, 94, 0.92)'
      ctx.fillRect(
        level.delta > ZERO ? deltaCenter : deltaCenter - barWidth,
        rowTop,
        barWidth,
        clippedHeight
      )
    }

    if (
      options.showDeltaPercentage
      && options.showLabels
      && clippedHeight >= 11
      && options.showDelta
    ) {
      ctx.font = '9px ui-monospace, SFMono-Regular, monospace'
      ctx.fillStyle = level.delta >= ZERO ? '#6ee7b7' : '#fda4af'
      ctx.fillText(
        `${level.deltaPercent.toFixed(0)}%`,
        Math.min(x + width - 31, deltaCenter + 4),
        y + 3
      )
    } else if (options.showLabels && clippedHeight >= 12 && volumeFraction > 0.25) {
      ctx.font = '9px ui-monospace, SFMono-Regular, monospace'
      ctx.fillStyle = 'rgba(226, 232, 240, 0.9)'
      const labelX = Math.min(currentX + 3, x + width - 86)
      const label = options.showDelta
        ? `${formatRaw(level.totalQty, snapshot.quantityScale)} Δ${formatSigned(level.delta, snapshot.quantityScale)}`
        : formatRaw(level.totalQty, snapshot.quantityScale)
      ctx.fillText(label, labelX, y + 3, Math.max(20, x + width - 8 - labelX))
    }

    if (options.showPoc && snapshot.pocPrice === level.price) {
      ctx.strokeStyle = 'rgba(250, 204, 21, 0.95)'
      ctx.lineWidth = 1
      ctx.beginPath()
      ctx.moveTo(x, y + 0.5)
      ctx.lineTo(x + width, y + 0.5)
      ctx.stroke()
    }

    if (options.showMaxPositiveDelta && snapshot.maxPositiveDeltaPrice === level.price) {
      ctx.fillStyle = '#34d399'
      ctx.fillRect(x + width - 5, y - 2, 4, 4)
    }

    if (options.showMaxNegativeDelta && snapshot.maxNegativeDeltaPrice === level.price) {
      ctx.fillStyle = '#fb7185'
      ctx.fillRect(x + width - 5, y - 2, 4, 4)
    }
  }

  if (options.showValueArea && snapshot.valueAreaHigh !== null && snapshot.valueAreaLow !== null) {
    for (const price of [snapshot.valueAreaHigh, snapshot.valueAreaLow]) {
      const y = engine.utils.yOf(price, pane, priceMin, priceMax)
      ctx.strokeStyle = 'rgba(250, 204, 21, 0.7)'
      ctx.setLineDash([3, 3])
      ctx.beginPath()
      ctx.moveTo(x, y)
      ctx.lineTo(x + width, y)
      ctx.stroke()
      ctx.setLineDash([])
    }
  }

  ctx.restore()
}

/**
 * Creates an open-charts overlay. Its source data follows the chart's primary
 * timeframe so the library's time scale remains aligned; its rendered profile
 * is supplied independently by the 1m RollingDeltaProfile engine.
 */
export function createDeltaProfileSeries(
  profile: RollingDeltaProfile
): SeriesDefinition<Bar, Bar, DeltaProfileOptions> {
  const params: DeltaProfileOptions = { ...DEFAULT_OPTIONS }

  return {
    id: 'delta-profile',
    label: 'DELTA PROFILE — ROLLING WINDOW',
    color: '#10b981',
    layer: 'foreground',
    width: '100%',
    height: '100%',
    params,

    compute(data) {
      return [...data]
    },

    render(
      ctx: CanvasRenderingContext2D,
      pane: MainPane,
      engine: ChartEngine,
      _data: readonly Bar[],
      _values: readonly Bar[],
      options: DeltaProfileOptions,
      priceMin: number,
      priceMax: number
    ) {
      drawProfile(
        ctx,
        pane,
        engine,
        profile.snapshot(options.valueAreaPercentage),
        options,
        priceMin,
        priceMax
      )
    },

    updateIncremental(data, values, isNewBar) {
      const lastBar = data.at(-1)
      if (!lastBar) {
        return
      }

      if (isNewBar) {
        values.push(lastBar)
      } else {
        values[values.length - 1] = lastBar
      }
    },

    tooltipRow(_values, _index) {
      const snapshot = profile.snapshot()
      return snapshot.levelCount === 0
        ? null
        : {
            label: 'Delta Profile',
            value: `${snapshot.rollingWindowMinutes}m · Δ ${formatSigned(snapshot.netDelta, snapshot.quantityScale)}`
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

    legend(_data, _values, _index, options) {
      const snapshot = profile.snapshot(options.valueAreaPercentage)
      if (snapshot.levelCount === 0) {
        return []
      }

      return [
        {
          label: 'Δ',
          value: formatSigned(snapshot.netDelta, snapshot.quantityScale),
          color: snapshot.netDelta >= ZERO ? '#10b981' : '#f43f5e'
        },
        {
          label: 'POC',
          value: snapshot.pocPrice === null
            ? '—'
            : formatPrice(snapshot.pocPrice, snapshot.priceScale),
          color: '#facc15'
        },
        {
          label: 'VA',
          value: snapshot.valueAreaLow === null || snapshot.valueAreaHigh === null
            ? '—'
            : `${formatPrice(snapshot.valueAreaLow, snapshot.priceScale)}–${formatPrice(snapshot.valueAreaHigh, snapshot.priceScale)}`,
          color: '#eab308'
        }
      ]
    }
  }
}

export function deltaProfileDefaults(): DeltaProfileOptions {
  return { ...DEFAULT_OPTIONS }
}
