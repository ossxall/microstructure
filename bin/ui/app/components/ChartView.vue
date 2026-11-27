<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, computed, reactive, nextTick, watch } from 'vue'
import { createChart } from '~/packages/open-charts/src'
import type { AnyChartSeries, ChartEngine } from '~/packages/open-charts/src/core/types'
import {
  DEFAULT_CHART_ENABLED,
  DEFAULT_CHART_ORDER,
  normalizeChartOrder,
  useChartLayoutStore,
  type ChartId
} from '~/stores/chartLayout'
import type { Bar } from '~/utils/bar'
import { createOhlcvSeries } from '~/utils/ohlcvSeries'
import {
  countAttackBubbles,
  createAttackBubbleSeries,
  DEFAULT_ATTACK_BUBBLE_OPTIONS,
  type AttackBubbleOptions
} from '~/utils/attackBubbleSeries'
import {
  createDeltaProfileSeries,
  deltaProfileDefaults,
  RollingDeltaProfile
} from '~/utils/deltaProfileSeries'
import {
  buildDeltaCandle,
  createDeltaCandleSeries,
  type DeltaBar,
  type DeltaMode
} from '~/utils/deltaCandleSeries'
import {
  buildBidAskFlowSeries,
  createBidAskFlowSeries,
  type FlowCandleStyle
} from '~/utils/bidAskFlowSeries'
import {
  calculateMomentumSeries,
  createMomentumSeries,
  DEFAULT_MOMENTUM_OPTIONS,
  normalizeMomentumOptions,
  type MomentumOptions
} from '~/utils/momentumSeries'
import { parseLosslessJson } from '~/utils/losslessJson'

type MarketSource = 'binance' | 'dydx'
type Timeframe = '1m' | '5m' | '15m' | '30m' | '1h' | '4h' | '1d'

interface HistoryResponse {
  success?: boolean
  message?: string
  data?: {
    history?: unknown[]
  }
}

const HISTORY_LIMIT = 50
const DELTA_HISTORY_LIMIT = 15
const HISTORY_REQUEST_TIMEOUT_MS = 50_000
const timeframeMinutes: Record<Timeframe, number> = {
  '1m': 1,
  '5m': 5,
  '15m': 15,
  '30m': 30,
  '1h': 60,
  '4h': 240,
  '1d': 1_440
}
const sourceOptions: { label: string, value: MarketSource }[] = [
  { label: 'Binance', value: 'binance' },
  { label: 'dYdX', value: 'dydx' }
]

const timeframeOptions: { label: string, value: Timeframe }[] = [
  { label: '1m', value: '1m' },
  { label: '5m', value: '5m' },
  { label: '15m', value: '15m' },
  { label: '30m', value: '30m' },
  { label: '1h', value: '1h' },
  { label: '4h', value: '4h' },
  { label: '1d', value: '1d' }
]
const rollingWindowOptions = [
  { label: '1 minuto', value: '1' },
  { label: '3 minutos', value: '3' },
  { label: '5 minutos', value: '5' },
  { label: '10 minutos', value: '10' },
  { label: '15 minutos', value: '15' },
  { label: '30 minutos', value: '30' },
  { label: '60 minutos', value: '60' },
  { label: '120 minutos', value: '120' },
  { label: '240 minutos', value: '240' },
  { label: 'Personalizado', value: 'custom' }
]

const chartHost = ref<HTMLElement | null>(null)
const momentumChartHost = ref<HTMLElement | null>(null)
const deltaChartHost = ref<HTMLElement | null>(null)
const bidAskFlowChartHost = ref<HTMLElement | null>(null)
const chartLayoutStore = useChartLayoutStore()
const chartEnabledReady = ref(false)
const chartEnabled = computed(() => chartEnabledReady.value
  ? chartLayoutStore.chartEnabled
  : DEFAULT_CHART_ENABLED)
const chartOrder = ref<ChartId[]>([...DEFAULT_CHART_ORDER])
const chartHostRefCallbacks: Record<ChartId, (element: unknown) => void> = {
  ohlcv: (element) => { chartHost.value = element as HTMLElement | null },
  momentum: (element) => { momentumChartHost.value = element as HTMLElement | null },
  delta: (element) => { deltaChartHost.value = element as HTMLElement | null },
  bidAskFlow: (element) => { bidAskFlowChartHost.value = element as HTMLElement | null }
}
const chartLabels: Record<ChartId, string> = {
  ohlcv: 'OHLCV',
  momentum: 'MOMENTUM ALCISTA/BAJISTA',
  delta: 'VELAS DE DELTA',
  bidAskFlow: 'FLUJO BID/ASK ACUMULADO'
}
const activeTab = ref<'charts' | 'settings'>('charts')
const source = ref<MarketSource>('binance')
const symbol = ref('BTCUSDT')
const timeframe = ref<Timeframe>('5m')
const initialTimeframe = ref<Timeframe>('5m')
const hasSelectedInitialTimeframe = ref(false)
const draftSource = ref<MarketSource>(source.value)
const draftSymbol = ref(symbol.value)
const draftTimeframe = ref<Timeframe>(timeframe.value)
const isLoading = ref(false)
const errorMessage = ref('')
const socketStatus = ref<'connecting' | 'connected' | 'disconnected' | 'error'>('disconnected')
const barCount = ref(0)
const latestBarState = ref<'live' | 'final' | 'none'>('none')
const profileSettings = reactive(deltaProfileDefaults())
const profileWindowPreset = ref('15')
const customProfileWindow = ref(15)
const profileBarCount = ref(0)
const profileDiagnostic = ref('')
const deltaMode = ref<DeltaMode>('PER_BAR')
const deltaCandleCount = ref(0)
const flowCandleStyle = ref<FlowCandleStyle>('CANDLE')
const bidAskFlowCandleCount = ref(0)
const momentumPointCount = ref(0)
const momentumDoubtfulCount = ref(0)
const momentumDiagnostic = ref('')
const momentumSettings = reactive<MomentumOptions>({ ...DEFAULT_MOMENTUM_OPTIONS })
const showAttackBubbles = ref(true)
const attackBubbleCount = ref(0)
const attackMinDominancePercent = ref(25)
const attackMinRelativeVolume = ref(1.25)
const attackBaselineBars = ref(20)
const canApply = computed(() => {
  return !isLoading.value && (
    draftSource.value !== source.value
    || draftSymbol.value.trim().toUpperCase() !== symbol.value
    || draftTimeframe.value !== timeframe.value
  )
})

function moveChart(chartId: ChartId, direction: -1 | 1) {
  const currentIndex = chartOrder.value.indexOf(chartId)
  const targetIndex = currentIndex + direction

  if (targetIndex < 0 || targetIndex >= chartOrder.value.length) {
    return
  }

  const reorderedCharts = [...chartOrder.value]
  const [movedChart] = reorderedCharts.splice(currentIndex, 1)

  if (!movedChart) {
    return
  }

  reorderedCharts.splice(targetIndex, 0, movedChart)
  chartOrder.value = reorderedCharts
  chartLayoutStore.setChartOrder(reorderedCharts)
}

function chartEngineFor(chartId: ChartId): ChartEngine | undefined {
  switch (chartId) {
    case 'ohlcv':
      return chart
    case 'momentum':
      return momentumChart
    case 'delta':
      return deltaChart
    case 'bidAskFlow':
      return bidAskFlowChart
  }
}

function hasEnabledCharts(): boolean {
  return Object.values(chartEnabled.value).some(Boolean)
}

function hasEnabledDeltaCharts(): boolean {
  return chartEnabled.value.delta || chartEnabled.value.bidAskFlow
}

function hydrateChart(chartId: ChartId) {
  const currentBars = [...bars.values()].sort((left, right) => left.time - right.time)
  const currentDeltaBars = [...deltaBars.values()].sort((left, right) => left.time - right.time)

  switch (chartId) {
    case 'ohlcv':
      rebuildDeltaProfile()
      if (currentBars.length > 0) {
        series?.setData(currentBars)
        attackBubbleSeries?.setData(currentBars)
        deltaProfileSeries?.setData(currentBars)
      }
      deltaProfileSeries?.setParams({ ...profileSettings })
      attackBubbleSeries?.setVisible(showAttackBubbles.value)
      applyAttackBubbleSettings()
      refreshAttackBubbleCount()
      if (
        hasSelectedInitialTimeframe.value
        && profileHistoryLimit(timeframe.value) > HISTORY_LIMIT
      ) {
        void resyncDeltaProfileHistory(
          source.value,
          symbol.value,
          timeframe.value,
          feedGeneration,
          profileHistoryLimit(timeframe.value)
        )
      }
      break
    case 'momentum':
      if (currentBars.length > 0) {
        momentumSeries?.setData(currentBars)
      }
      momentumSeries?.setParams(momentumParamsPatch())
      refreshMomentumStatus(currentBars)
      break
    case 'delta':
      if (currentDeltaBars.length > 0) {
        deltaCandleSeries?.setData(currentDeltaBars)
      }
      deltaCandleSeries?.setParams({
        mode: deltaMode.value,
        baseClose: deltaCvdBase
      })
      refreshDeltaCandleCount()
      break
    case 'bidAskFlow':
      if (currentDeltaBars.length > 0) {
        bidAskFlowSeries?.setData(currentDeltaBars)
      }
      bidAskFlowSeries?.setParams({ style: flowCandleStyle.value })
      refreshBidAskFlowCandleCount()
      break
  }
}

async function toggleChart(chartId: ChartId) {
  const wasAnyEnabled = hasEnabledCharts()
  const enabled = !chartEnabled.value[chartId]
  chartLayoutStore.setChartEnabled(chartId, enabled)
  const engine = chartEngineFor(chartId)

  if (!enabled) {
    engine?.core.pauseLoop()
    if (!hasEnabledCharts()) {
      feedGeneration++
      isLoading.value = false
      stopWebSocket()
      feedAbortController?.abort()
      feedAbortController = undefined
    }
    return
  }

  if (!wasAnyEnabled && hasSelectedInitialTimeframe.value) {
    await loadMarketData(source.value, symbol.value, timeframe.value)
  } else {
    hydrateChart(chartId)
    if (
      wasAnyEnabled
      && hasSelectedInitialTimeframe.value
      && (chartId === 'delta' || chartId === 'bidAskFlow')
    ) {
      void resyncDeltaHistory(source.value, symbol.value, timeframe.value, feedGeneration)
    }
  }

  if (activeTab.value === 'charts' && engine) {
    await nextTick()
    engine.core.resize()
    engine.core.startLoop()
  }
}

watch(activeTab, async (tab) => {
  const chartEngines: [ChartId, ChartEngine | undefined][] = [
    ['ohlcv', chart],
    ['momentum', momentumChart],
    ['delta', deltaChart],
    ['bidAskFlow', bidAskFlowChart]
  ]

  if (tab === 'settings') {
    chartEngines.forEach(([, chartEngine]) => chartEngine?.core.pauseLoop())
    return
  }

  await nextTick()
  chartEngines.forEach(([chartId, chartEngine]) => {
    if (!chartEngine) {
      return
    }

    if (!chartEnabled.value[chartId]) {
      chartEngine.core.pauseLoop()
      return
    }

    chartEngine.core.resize()
    chartEngine.core.startLoop()
  })
})

const config = useRuntimeConfig()
const colorMode = useColorMode()
const bars = new Map<number, Bar>()
const deltaBars = new Map<number, DeltaBar>()
let deltaCvdBase = 0
const deltaProfile = new RollingDeltaProfile(profileSettings, (message) => {
  profileDiagnostic.value = message
  console.warn(message)
})
let chart: ChartEngine | undefined
let momentumChart: ChartEngine | undefined
let deltaChart: ChartEngine | undefined
let bidAskFlowChart: ChartEngine | undefined
let series: AnyChartSeries | undefined
let momentumSeries: AnyChartSeries | undefined
let attackBubbleSeries: AnyChartSeries | undefined
let unsubscribeChartEvents: (() => void) | undefined
let deltaCandleSeries: AnyChartSeries | undefined
let bidAskFlowSeries: AnyChartSeries | undefined
let deltaProfileSeries: AnyChartSeries | undefined
let socket: WebSocket | undefined
let reconnectTimer: ReturnType<typeof setTimeout> | undefined
let feedAbortController: AbortController | undefined
let feedGeneration = 0

const chartColorPalettes = {
  dark: {
    bg: '#0a0a0a',
    bg2: '#0a0a0a',
    bg3: 'rgba(255,255,255,0.1)',
    grid: 'rgba(64,64,64,0.5)',
    gridAlt: 'rgba(64,64,64,0.65)',
    text: '#d4d4d4',
    textDim: '#a3a3a3',
    line: '#00a155',
    area1: 'rgba(0,161,85,0.28)',
    area2: 'rgba(0,161,85,0)',
    bb: '#00a155',
    bbFill: 'rgba(0,161,85,0.05)',
    cross: 'rgba(163,163,163,0.25)',
    crossPt: '#00a155',
    vol: 'rgba(163,163,163,0.25)',
    accent: '#00a155'
  },
  light: {
    bg: '#ffffff',
    bg2: '#fafafa',
    bg3: 'rgba(0,0,0,0.08)',
    grid: 'rgba(163,163,163,0.35)',
    gridAlt: 'rgba(163,163,163,0.5)',
    text: '#171717',
    textDim: '#737373',
    line: '#008f5a',
    area1: 'rgba(0,143,90,0.22)',
    area2: 'rgba(0,143,90,0)',
    bb: '#008f5a',
    bbFill: 'rgba(0,143,90,0.05)',
    cross: 'rgba(115,115,115,0.3)',
    crossPt: '#008f5a',
    vol: 'rgba(115,115,115,0.25)',
    accent: '#008f5a'
  }
} as const

function updateChartTheme(engine: ChartEngine) {
  const palette = colorMode.value === 'dark' ? chartColorPalettes.dark : chartColorPalettes.light
  Object.assign(engine.options.colors, palette)
  engine.core.loadCssVariables()
  engine.dirty = true
  engine.timeAxisDirty = true
}

function updateChartThemes() {
  const engines = [chart, momentumChart, deltaChart, bidAskFlowChart]
  engines.forEach((engine) => {
    if (engine) {
      updateChartTheme(engine)
    }
  })
}

watch(() => colorMode.value, updateChartThemes)
let seriesHydrated = false

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function isCurrentTimeframe(nextTimeframe: Timeframe): boolean {
  return hasSelectedInitialTimeframe.value && timeframe.value === nextTimeframe
}

function profileHistoryLimit(nextTimeframe: Timeframe): number {
  return Math.ceil(profileSettings.rollingWindowMinutes / timeframeMinutes[nextTimeframe])
}

function matchesTimeframe(value: unknown, expectedTimeframe: Timeframe): boolean {
  if (!isRecord(value)) {
    return false
  }

  const records = [
    value,
    ...(isRecord(value.data) ? [value.data] : [])
  ]

  return records.every((record) => {
    const declaredTimeframe = typeof record.timeframe === 'string'
      ? record.timeframe
      : ''
    const eventType = typeof record.event_type === 'string'
      ? record.event_type
      : ''

    return (!declaredTimeframe || declaredTimeframe === expectedTimeframe)
      && (!eventType.startsWith('bar.') || eventType === `bar.${expectedTimeframe}`)
  })
}

function numberValue(value: unknown): number {
  if (typeof value === 'number') {
    return value
  }

  if (typeof value === 'string' && value.trim() !== '') {
    return Number(value)
  }

  return Number.NaN
}

function timestampToSeconds(value: unknown): number {
  const timestamp = numberValue(value)
  const unitsPerSecond: Record<MomentumOptions['timestampUnit'], number> = {
    s: 1,
    ms: 1_000,
    us: 1_000_000,
    ns: 1_000_000_000
  }
  return timestamp / unitsPerSecond[momentumSettings.timestampUnit]
}

function parseMarketBar(value: unknown): Bar | null {
  if (!isRecord(value)) {
    return null
  }

  const bar = isRecord(value.data) ? value.data : value
  const price = isRecord(bar.price) ? bar.price : bar
  const header = isRecord(bar.header) ? bar.header : {}
  const volumeFlow = isRecord(bar.volume_flow) ? bar.volume_flow : {}
  const hasScaledPrices = isRecord(bar.price)
  const priceScale = hasScaledPrices ? numberValue(header.price_scale ?? 8) : 0
  const quantityScale = numberValue(header.quantity_scale ?? 8)
  const priceDivisor = 10 ** (Number.isFinite(priceScale) ? priceScale : 8)
  const quantityDivisor = 10 ** (Number.isFinite(quantityScale) ? quantityScale : 8)
  const timestamp = numberValue(bar.timestamp ?? bar.time)
  const open = numberValue(price.open) / priceDivisor
  const high = numberValue(price.high) / priceDivisor
  const low = numberValue(price.low) / priceDivisor
  const close = numberValue(price.close) / priceDivisor
  const rawVolume = volumeFlow.total_qty ?? bar.volume ?? 0
  const volume = numberValue(rawVolume) / (isRecord(bar.volume_flow) ? quantityDivisor : 1)
  const revision = numberValue(bar.revision ?? value.revision ?? 0)

  if (
    ![timestamp, open, high, low, close, volume].every(Number.isFinite)
    || high < low
  ) {
    return null
  }

  return {
    ...bar,
    is_final: typeof value.is_final === 'boolean' ? value.is_final : true,
    ...(typeof value.event_id === 'string' ? { event_id: value.event_id } : {}),
    ...(typeof value.event_type === 'string' ? { event_type: value.event_type } : {}),
    // open-charts uses Unix timestamps in seconds for its time axis.
    time: timestampToSeconds(bar.timestamp ?? bar.time),
    open,
    high,
    low,
    close,
    volume,
    revision: Number.isFinite(revision) ? revision : 0,
    ...(bar.revision !== undefined || value.revision !== undefined
      ? { revision_raw: (bar.revision ?? value.revision) as string | number }
      : {})
  } as Bar
}

function parseDeltaBar(value: unknown): DeltaBar | null {
  if (!isRecord(value)) {
    return null
  }

  const bar = isRecord(value.data) ? value.data : value
  const timestamp = numberValue(bar.timestamp ?? bar.time)
  const revision = numberValue(bar.revision ?? value.revision ?? 0)
  if (!Number.isFinite(timestamp)) {
    return null
  }

  const header = isRecord(bar.header) ? bar.header : undefined
  const rawTicks = Array.isArray(bar.raw_ticks)
    ? bar.raw_ticks.filter(isRecord)
    : undefined

  return {
    time: timestampToSeconds(bar.timestamp ?? bar.time),
    revision: Number.isFinite(revision) ? revision : 0,
    ...(header ? { header } : {}),
    ...(rawTicks ? { raw_ticks: rawTicks } : {})
  }
}

function exactBarRevision(bar: Bar): bigint {
  const revision = bar.revision_raw ?? bar.revision
  if (typeof revision === 'string' && /^\d+$/.test(revision.trim())) {
    return BigInt(revision.trim())
  }
  if (typeof revision === 'number' && Number.isSafeInteger(revision) && revision >= 0) {
    return BigInt(revision)
  }
  return BigInt(Math.max(0, Math.trunc(bar.revision)))
}

function upsertBar(bar: Bar): boolean {
  const current = bars.get(bar.time)
  const revisionComparison = current
    ? exactBarRevision(bar) - exactBarRevision(current)
    : 1n

  if (
    current
    && (
      revisionComparison < 0n
      || (
        revisionComparison === 0n
        && JSON.stringify(bar) === JSON.stringify(current)
      )
    )
  ) {
    return false
  }

  bars.set(bar.time, bar)

  if (bars.size > HISTORY_LIMIT) {
    const oldestTimes = [...bars.keys()]
      .sort((left, right) => left - right)
      .slice(0, bars.size - HISTORY_LIMIT)

    oldestTimes.forEach(time => bars.delete(time))
  }

  barCount.value = bars.size
  refreshAttackBubbleCount()
  updateLatestBarState()
  return bars.has(bar.time)
}

function refreshAttackBubbleCount() {
  if (!chartEnabled.value.ohlcv) {
    return
  }

  const orderedBars = [...bars.values()].sort((left, right) => left.time - right.time)
  const params = attackBubbleSeries?.getParams()
  const optionValue = (key: keyof AttackBubbleOptions) => {
    const value = numberValue(params?.[key])
    return Number.isFinite(value)
      ? value
      : DEFAULT_ATTACK_BUBBLE_OPTIONS[key]
  }
  const options: AttackBubbleOptions = {
    minDominance: optionValue('minDominance'),
    minRelativeVolume: optionValue('minRelativeVolume'),
    baselineBars: optionValue('baselineBars')
  }

  attackBubbleCount.value = countAttackBubbles(orderedBars, options)
}

function toggleAttackBubbles() {
  if (chartEnabled.value.ohlcv) {
    attackBubbleSeries?.setVisible(showAttackBubbles.value)
  }
}

function applyAttackBubbleSettings() {
  const dominance = numberValue(attackMinDominancePercent.value)
  const relativeVolume = numberValue(attackMinRelativeVolume.value)
  const baselineBars = numberValue(attackBaselineBars.value)

  attackMinDominancePercent.value = Math.min(
    80,
    Math.max(10, Number.isFinite(dominance) ? dominance : 25)
  )
  attackMinRelativeVolume.value = Math.min(
    3,
    Math.max(1, Number.isFinite(relativeVolume) ? relativeVolume : 1.25)
  )
  attackBaselineBars.value = Math.min(
    50,
    Math.max(5, Math.round(Number.isFinite(baselineBars) ? baselineBars : 20))
  )

  if (chartEnabled.value.ohlcv) {
    attackBubbleSeries?.setParams({
      minDominance: attackMinDominancePercent.value / 100,
      minRelativeVolume: attackMinRelativeVolume.value,
      baselineBars: attackBaselineBars.value
    })
  }
  refreshAttackBubbleCount()
}

function updateLatestBarState() {
  let latest: Bar | undefined
  for (const bar of bars.values()) {
    if (!latest || bar.time > latest.time) {
      latest = bar
    }
  }

  latestBarState.value = !latest
    ? 'none'
    : latest.is_final === false
      ? 'live'
      : 'final'
}

function replaceSeries() {
  if (!chart) {
    return
  }

  deltaProfileSeries?.destroy({ silent: true })
  attackBubbleSeries?.destroy({ silent: true })
  series?.destroy({ silent: true })
  series = chart.api.addSeries(createOhlcvSeries())
  deltaProfileSeries = chart.api.addSeries(createDeltaProfileSeries(deltaProfile))
  attackBubbleSeries = chart.api.addSeries(createAttackBubbleSeries())
  attackBubbleSeries.setVisible(showAttackBubbles.value)
  applyAttackBubbleSettings()
}

function momentumParamsPatch(options = momentumSettings) {
  return {
    signalPeriod: options.signalPeriod,
    basePeriod: options.basePeriod,
    alpha: options.alpha,
    maxIntensity: options.maxIntensity,
    warmup: options.warmup,
    timestampUnit: options.timestampUnit
  }
}

function replaceMomentumSeries() {
  if (!momentumChart) {
    return
  }

  momentumSeries?.destroy({ silent: true })
  momentumSeries = momentumChart.api.addSeries(createMomentumSeries())
  momentumSeries.setParams(momentumParamsPatch())
  momentumPointCount.value = 0
  momentumDoubtfulCount.value = 0
}

function replaceDeltaCandleSeries() {
  if (!deltaChart) {
    return
  }

  deltaCandleSeries?.destroy({ silent: true })
  deltaCandleSeries = deltaChart.api.addSeries(createDeltaCandleSeries())
  deltaCandleSeries.setParams({
    mode: deltaMode.value,
    baseClose: deltaCvdBase
  })
  deltaCandleCount.value = 0
}

function replaceBidAskFlowSeries() {
  if (!bidAskFlowChart) {
    return
  }

  bidAskFlowSeries?.destroy({ silent: true })
  bidAskFlowSeries = bidAskFlowChart.api.addSeries(createBidAskFlowSeries())
  bidAskFlowSeries.setParams({ style: flowCandleStyle.value })
  bidAskFlowCandleCount.value = 0
}

function refreshDeltaCandleCount() {
  deltaCandleCount.value = [...deltaBars.values()]
    .reduce((count, bar) => {
      const candle = buildDeltaCandle(bar, 0, deltaMode.value)
      return candle ? count + 1 : count
    }, 0)
}

function refreshBidAskFlowCandleCount() {
  const orderedBars = [...deltaBars.values()].sort((left, right) => left.time - right.time)
  bidAskFlowCandleCount.value = buildBidAskFlowSeries(orderedBars).length
}

function upsertDeltaBar(bar: DeltaBar): boolean {
  if (!Array.isArray(bar.raw_ticks)) {
    return false
  }

  const current = deltaBars.get(bar.time)
  const oldestTime = Math.min(...deltaBars.keys())
  if (
    !current
    && deltaBars.size >= DELTA_HISTORY_LIMIT
    && bar.time < oldestTime
  ) {
    return false
  }

  if (
    current
    && (
      bar.revision < current.revision
      || (
        bar.revision === current.revision
        && JSON.stringify(bar) === JSON.stringify(current)
      )
    )
  ) {
    return false
  }

  const existingTimes = new Set(deltaBars.keys())
  deltaBars.set(bar.time, bar)
  const oldestTimes = [...deltaBars.keys()]
    .sort((left, right) => left - right)
    .slice(0, Math.max(0, deltaBars.size - DELTA_HISTORY_LIMIT))
  let baseChanged = false
  oldestTimes.forEach((time) => {
    const removedBar = deltaBars.get(time)
    if (removedBar && existingTimes.has(time)) {
      const removedCandle = buildDeltaCandle(removedBar)
      if (removedCandle) {
        deltaCvdBase += removedCandle.close - removedCandle.open
        baseChanged = true
      }
    }
    deltaBars.delete(time)
  })

  if (chartEnabled.value.delta) {
    deltaCandleSeries?.upsert(bar, { maxDataPoints: DELTA_HISTORY_LIMIT })
    if (baseChanged) {
      deltaCandleSeries?.setParams({ baseClose: deltaCvdBase })
    }
    refreshDeltaCandleCount()
  }
  if (chartEnabled.value.bidAskFlow) {
    bidAskFlowSeries?.upsert(bar, { maxDataPoints: DELTA_HISTORY_LIMIT })
    refreshBidAskFlowCandleCount()
  }
  return true
}

function applyDeltaMode() {
  if (chartEnabled.value.delta) {
    deltaCandleSeries?.setParams({ mode: deltaMode.value })
    refreshDeltaCandleCount()
  }
}

function applyFlowCandleStyle() {
  if (chartEnabled.value.bidAskFlow) {
    bidAskFlowSeries?.setParams({ style: flowCandleStyle.value })
  }
}

function renderCurrentBars() {
  const currentBars = [...bars.values()].sort((left, right) => left.time - right.time)

  if (!series) {
    return
  }

  if (currentBars.length > 0 && chartEnabled.value.ohlcv) {
    // Full replacement is reserved for initial history hydration.
    series.setData(currentBars)
    attackBubbleSeries?.setData(currentBars)
    deltaProfileSeries?.setData(currentBars)
  }
  if (currentBars.length > 0 && chartEnabled.value.momentum) {
    momentumSeries?.setData(currentBars)
  }

  seriesHydrated = true
  if (chartEnabled.value.ohlcv) {
    refreshDeltaProfile(true)
  }
  refreshMomentumStatus(currentBars)
}

function refreshMomentumStatus(currentBars = [...bars.values()].sort((left, right) => left.time - right.time)) {
  if (!chartEnabled.value.momentum) {
    return
  }

  try {
    const points = calculateMomentumSeries(currentBars, momentumSettings)
    momentumPointCount.value = points.filter(point => point.bullish !== null).length
    momentumDoubtfulCount.value = points.filter(point => point.doubtful).length

    if (currentBars.length === 0) {
      momentumDiagnostic.value = 'Entrada vacía: no hay barras para calcular el indicador.'
    } else if (currentBars.length <= momentumSettings.warmup) {
      momentumDiagnostic.value = `Aviso: la serie tiene ${currentBars.length} barras; se necesitan más de ${momentumSettings.warmup} para mostrar momentum.`
    } else {
      momentumDiagnostic.value = ''
    }
  } catch (error) {
    momentumPointCount.value = 0
    momentumDoubtfulCount.value = 0
    momentumDiagnostic.value = error instanceof Error
      ? error.message
      : 'No se pudo calcular el indicador de momentum.'
  }
}

async function fetchMarketHistory(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: Timeframe,
  options: { limit?: number, includeRawTicks?: boolean, signal?: AbortSignal } = {}
): Promise<Bar[]> {
  if (!isCurrentTimeframe(nextTimeframe)) {
    return []
  }

  const response = await $fetch<HistoryResponse>('/api/market/get-history', {
    method: 'POST',
    timeout: HISTORY_REQUEST_TIMEOUT_MS,
    signal: options.signal,
    body: {
      source: nextSource,
      symbol: nextSymbol,
      timeframe: nextTimeframe,
      limit: options.limit ?? HISTORY_LIMIT,
      include_raw_ticks: options.includeRawTicks ?? true
    }
  })

  if (response.success === false) {
    throw new Error(response.message || 'No se pudo cargar el histórico')
  }

  return (response.data?.history ?? [])
    .filter(bar => matchesTimeframe(bar, nextTimeframe))
    .map(parseMarketBar)
    .filter((bar): bar is Bar => bar !== null)
}

async function fetchDeltaHistory(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: Timeframe,
  signal?: AbortSignal
): Promise<DeltaBar[]> {
  if (!isCurrentTimeframe(nextTimeframe)) {
    return []
  }

  const response = await $fetch<HistoryResponse>('/api/market/get-history', {
    method: 'POST',
    timeout: HISTORY_REQUEST_TIMEOUT_MS,
    signal,
    body: {
      source: nextSource,
      symbol: nextSymbol,
      timeframe: nextTimeframe,
      limit: DELTA_HISTORY_LIMIT,
      include_raw_ticks: true
    }
  })

  if (response.success === false) {
    throw new Error(response.message || 'No se pudo cargar el histórico de delta')
  }

  return (response.data?.history ?? [])
    .filter(bar => matchesTimeframe(bar, nextTimeframe))
    .map(parseDeltaBar)
    .filter((bar): bar is DeltaBar => bar !== null)
}

function refreshDeltaProfile(force = false) {
  profileBarCount.value = deltaProfile.activeBarCount

  if (!chartEnabled.value.ohlcv || (!force && !profileSettings.autoUpdate)) {
    return
  }

  if (chart) {
    chart.dirty = true
    chart.overlayDirty = true
  }
}

function rebuildDeltaProfile() {
  deltaProfile.reset(symbol.value, source.value, timeframe.value)
  const currentBars = [...bars.values()].sort((left, right) => left.time - right.time)
  currentBars.forEach(bar => deltaProfile.upsert(bar))
  refreshDeltaProfile(true)
}

function upsertDeltaProfileBar(bar: Bar): boolean {
  if (!chartEnabled.value.ohlcv) {
    return false
  }

  const changed = deltaProfile.upsert(bar)
  if (changed) {
    refreshDeltaProfile()
  }
  return changed
}

async function resyncDeltaProfileHistory(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: Timeframe,
  generation: number,
  limit = profileHistoryLimit(nextTimeframe)
) {
  if (
    generation !== feedGeneration
    || !chartEnabled.value.ohlcv
    || !isCurrentTimeframe(nextTimeframe)
  ) {
    return
  }

  try {
    const history = await fetchMarketHistory(
      nextSource,
      nextSymbol,
      nextTimeframe,
      {
        limit,
        includeRawTicks: false,
        signal: feedAbortController?.signal
      }
    )

    if (
      generation !== feedGeneration
      || !chartEnabled.value.ohlcv
      || !isCurrentTimeframe(nextTimeframe)
    ) {
      return
    }

    history.sort((left, right) => left.time - right.time)
    for (const bar of history) {
      deltaProfile.upsert(bar)
    }
    refreshDeltaProfile()
  } catch (error) {
    if (generation === feedGeneration && chartEnabled.value.ohlcv) {
      profileDiagnostic.value = error instanceof Error
        ? `No se pudo resincronizar el histórico ${nextTimeframe} del Delta Profile: ${error.message}`
        : `No se pudo resincronizar el histórico ${nextTimeframe} del Delta Profile`
    }
  }
}

async function resyncMarketHistory(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: Timeframe,
  generation: number
) {
  if (
    generation !== feedGeneration
    || (!chartEnabled.value.ohlcv && !chartEnabled.value.momentum)
    || !isCurrentTimeframe(nextTimeframe)
  ) {
    return
  }

  try {
    const history = await fetchMarketHistory(nextSource, nextSymbol, nextTimeframe, {
      includeRawTicks: false,
      signal: feedAbortController?.signal
    })

    if (
      generation !== feedGeneration
      || (!chartEnabled.value.ohlcv && !chartEnabled.value.momentum)
    ) {
      return
    }

    let profileChanged = false
    if (chartEnabled.value.ohlcv) {
      for (const bar of history) {
        profileChanged = deltaProfile.upsert(bar) || profileChanged
      }
    }
    if (profileChanged) {
      refreshDeltaProfile()
    }

    const changedBars = history.filter(upsertBar)
    // Apply bars oldest-first: missed bars then take the cheap append path,
    // while any late revision is recomputed before newer bars are appended.
    changedBars.sort((left, right) => left.time - right.time)
    if (seriesHydrated) {
      for (const bar of changedBars) {
        if (chartEnabled.value.ohlcv && series) {
          series.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
          attackBubbleSeries?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
          deltaProfileSeries?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
        }
        if (chartEnabled.value.momentum) {
          momentumSeries?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
        }
      }
    }
    refreshMomentumStatus([...bars.values()].sort((left, right) => left.time - right.time))

    if (errorMessage.value.startsWith('Resincronización:')) {
      errorMessage.value = ''
    }
  } catch (error) {
    if (
      generation === feedGeneration
      && (chartEnabled.value.ohlcv || chartEnabled.value.momentum)
    ) {
      errorMessage.value = error instanceof Error
        ? `Resincronización: ${error.message}`
        : 'No se pudo resincronizar el histórico'
    }
  }
}

async function resyncDeltaHistory(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: Timeframe,
  generation: number
) {
  if (
    generation !== feedGeneration
    || !hasEnabledDeltaCharts()
    || !isCurrentTimeframe(nextTimeframe)
  ) {
    return
  }

  try {
    const history = await fetchDeltaHistory(
      nextSource,
      nextSymbol,
      nextTimeframe,
      feedAbortController?.signal
    )

    if (generation !== feedGeneration || !hasEnabledDeltaCharts()) {
      return
    }

    history.sort((left, right) => left.time - right.time)
    for (const bar of history) {
      upsertDeltaBar(bar)
    }
  } catch (error) {
    if (generation === feedGeneration && hasEnabledDeltaCharts()) {
      profileDiagnostic.value = error instanceof Error
        ? `No se pudo cargar el histórico de velas de delta: ${error.message}`
        : 'No se pudo cargar el histórico de velas de delta'
    }
  }
}

function stopWebSocket() {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer)
    reconnectTimer = undefined
  }

  const currentSocket = socket
  socket = undefined
  currentSocket?.close()
  socketStatus.value = 'disconnected'
}

function connectWebSocket(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: Timeframe,
  generation: number,
  resyncAfterConnect = false
) {
  if (
    generation !== feedGeneration
    || !hasEnabledCharts()
    || !isCurrentTimeframe(nextTimeframe)
  ) {
    return
  }

  socketStatus.value = 'connecting'

  try {
    const nextSocket = new WebSocket(String(config.public.marketWebSocketUrl))
    socket = nextSocket

    nextSocket.addEventListener('open', () => {
      if (socket !== nextSocket || generation !== feedGeneration) {
        return
      }

      socketStatus.value = 'connected'
      nextSocket.send(JSON.stringify({
        action: 'subscribe',
        source: nextSource,
        symbol: nextSymbol,
        timeframe: nextTimeframe
      }))
    })

    nextSocket.addEventListener('message', (event: MessageEvent<string>) => {
      if (generation !== feedGeneration) {
        return
      }

      try {
        const message: unknown = parseLosslessJson(event.data)

        if (
          isRecord(message)
          && message.type === 'subscribed'
          && message.source === nextSource
          && message.symbol === nextSymbol
          && message.timeframe === nextTimeframe
        ) {
          if (resyncAfterConnect) {
            void resyncMarketHistory(
              nextSource,
              nextSymbol,
              nextTimeframe,
              generation
            )
            if (profileHistoryLimit(nextTimeframe) > HISTORY_LIMIT) {
              void resyncDeltaProfileHistory(
                nextSource,
                nextSymbol,
                nextTimeframe,
                generation
              )
            }
            if (hasEnabledDeltaCharts()) {
              void resyncDeltaHistory(
                nextSource,
                nextSymbol,
                nextTimeframe,
                generation
              )
            }
          }
          return
        }

        if (isRecord(message) && message.type === 'error') {
          errorMessage.value = typeof message.message === 'string'
            ? message.message
            : 'El servidor rechazó la suscripción'
          return
        }

        if (!matchesTimeframe(message, nextTimeframe)) {
          return
        }

        const deltaBar = parseDeltaBar(message)
        if (deltaBar) {
          upsertDeltaBar(deltaBar)
        }

        const bar = parseMarketBar(message)

        if (bar && matchesTimeframe(bar, nextTimeframe)) {
          upsertDeltaProfileBar(bar)
          if (upsertBar(bar) && seriesHydrated) {
            if (chartEnabled.value.ohlcv) {
              series?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
              attackBubbleSeries?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
              deltaProfileSeries?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
            }
            if (chartEnabled.value.momentum) {
              momentumSeries?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
              refreshMomentumStatus()
            }
          }
        }
      } catch {
        errorMessage.value = 'El servidor envió un mensaje WebSocket inválido'
      }
    })

    nextSocket.addEventListener('error', () => {
      if (socket === nextSocket) {
        socketStatus.value = 'error'
      }
    })

    nextSocket.addEventListener('close', () => {
      if (socket !== nextSocket || generation !== feedGeneration) {
        return
      }

      socket = undefined
      socketStatus.value = 'disconnected'
      reconnectTimer = setTimeout(() => {
        connectWebSocket(nextSource, nextSymbol, nextTimeframe, generation, true)
      }, 1500)
    })
  } catch {
    socketStatus.value = 'error'
  }
}

async function loadMarketData(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: Timeframe
) {
  if (!hasEnabledCharts() || !isCurrentTimeframe(nextTimeframe)) {
    return
  }

  feedAbortController?.abort()
  const generation = ++feedGeneration
  feedAbortController = new AbortController()
  isLoading.value = true
  errorMessage.value = ''
  seriesHydrated = false
  bars.clear()
  deltaBars.clear()
  deltaCvdBase = 0
  deltaCandleCount.value = 0
  bidAskFlowCandleCount.value = 0
  barCount.value = 0
  attackBubbleCount.value = 0
  momentumPointCount.value = 0
  momentumDoubtfulCount.value = 0
  momentumDiagnostic.value = ''
  latestBarState.value = 'none'
  profileDiagnostic.value = ''
  profileBarCount.value = 0
  deltaProfile.reset(nextSymbol, nextSource, nextTimeframe)
  stopWebSocket()
  replaceSeries()
  replaceMomentumSeries()
  replaceDeltaCandleSeries()
  replaceBidAskFlowSeries()
  connectWebSocket(nextSource, nextSymbol, nextTimeframe, generation)
  if (hasEnabledDeltaCharts()) {
    void resyncDeltaHistory(nextSource, nextSymbol, nextTimeframe, generation)
  }

  try {
    const history = await fetchMarketHistory(nextSource, nextSymbol, nextTimeframe, {
      includeRawTicks: false,
      signal: feedAbortController.signal
    })

    if (generation !== feedGeneration) {
      return
    }

    for (const bar of history) {
      upsertBar(bar)
      upsertDeltaProfileBar(bar)
    }

    renderCurrentBars()
    if (profileHistoryLimit(nextTimeframe) > HISTORY_LIMIT) {
      void resyncDeltaProfileHistory(nextSource, nextSymbol, nextTimeframe, generation)
    }
  } catch (error) {
    if (generation === feedGeneration) {
      errorMessage.value = error instanceof Error
        ? `Histórico: ${error.message}`
        : 'No se pudo cargar el histórico'
      renderCurrentBars()
      if (profileHistoryLimit(nextTimeframe) > HISTORY_LIMIT) {
        void resyncDeltaProfileHistory(
          nextSource,
          nextSymbol,
          nextTimeframe,
          generation,
          profileHistoryLimit(nextTimeframe)
        )
      }
    }
  } finally {
    if (generation === feedGeneration) {
      isLoading.value = false
    }
  }
}

function startChartWithSelectedTimeframe() {
  timeframe.value = initialTimeframe.value
  draftTimeframe.value = initialTimeframe.value
  hasSelectedInitialTimeframe.value = true
  void loadMarketData(source.value, symbol.value, timeframe.value)
}

async function applySettings() {
  const nextSymbol = draftSymbol.value.trim().toUpperCase()

  if (!nextSymbol) {
    errorMessage.value = 'Escribe un símbolo válido'
    return
  }

  const settingsChanged = draftSource.value !== source.value
    || nextSymbol !== symbol.value
    || draftTimeframe.value !== timeframe.value

  source.value = draftSource.value
  symbol.value = nextSymbol
  draftSymbol.value = nextSymbol
  timeframe.value = draftTimeframe.value

  if (settingsChanged) {
    await loadMarketData(source.value, symbol.value, timeframe.value)
  }
}

function applyDeltaProfileSettings() {
  if (profileWindowPreset.value === 'custom') {
    profileSettings.rollingWindowMinutes = Math.min(
      240,
      Math.max(1, Math.round(Number(customProfileWindow.value) || 1))
    )
  } else {
    profileSettings.rollingWindowMinutes = Number(profileWindowPreset.value)
  }

  customProfileWindow.value = profileSettings.rollingWindowMinutes
  profileSettings.valueAreaPercentage = Math.min(
    100,
    Math.max(1, Number(profileSettings.valueAreaPercentage) || 70)
  )
  deltaProfile.configure({ ...profileSettings })
  if (chartEnabled.value.ohlcv) {
    deltaProfileSeries?.setParams({ ...profileSettings })
  }
  refreshDeltaProfile(true)
  if (
    isCurrentTimeframe(timeframe.value)
    && profileHistoryLimit(timeframe.value) > HISTORY_LIMIT
  ) {
    void resyncDeltaProfileHistory(
      source.value,
      symbol.value,
      timeframe.value,
      feedGeneration,
      profileHistoryLimit(timeframe.value)
    )
  }
}

async function applyMomentumSettings() {
  const previousTimestampUnit = momentumSettings.timestampUnit
  const options = normalizeMomentumOptions(momentumSettings)
  Object.assign(momentumSettings, options)
  if (chartEnabled.value.momentum) {
    momentumSeries?.setParams(momentumParamsPatch(options))
  }
  if (
    previousTimestampUnit !== options.timestampUnit
    && chartEnabled.value.momentum
    && hasSelectedInitialTimeframe.value
  ) {
    await loadMarketData(source.value, symbol.value, timeframe.value)
    return
  }
  refreshMomentumStatus()
}

function selectDeltaProfileWindow() {
  if (profileWindowPreset.value !== 'custom') {
    profileSettings.rollingWindowMinutes = Number(profileWindowPreset.value)
    customProfileWindow.value = profileSettings.rollingWindowMinutes
  } else {
    customProfileWindow.value = profileSettings.rollingWindowMinutes
  }
  applyDeltaProfileSettings()
}

onMounted(async () => {
  chartLayoutStore.setChartEnabledState(chartLayoutStore.chartEnabled)
  chartEnabledReady.value = true
  const savedChartOrder = normalizeChartOrder(chartLayoutStore.chartOrder)
  chartOrder.value = savedChartOrder
  chartLayoutStore.setChartOrder(savedChartOrder)
  await nextTick()

  if (!chartHost.value) {
    return
  }

  chart = createChart(chartHost.value)
  if (momentumChartHost.value) {
    momentumChart = createChart(momentumChartHost.value)
    replaceMomentumSeries()
  }
  unsubscribeChartEvents = chart.subscribe((event) => {
    if (
      event.type === 'series:params'
      && event.seriesId === 'aggressive-attack-bubbles'
    ) {
      refreshAttackBubbleCount()
    }
  })
  replaceSeries()
  if (deltaChartHost.value) {
    deltaChart = createChart(deltaChartHost.value)
    replaceDeltaCandleSeries()
  }
  if (bidAskFlowChartHost.value) {
    bidAskFlowChart = createChart(bidAskFlowChartHost.value)
    replaceBidAskFlowSeries()
  }
  updateChartThemes()

  await nextTick()
  const chartEngines = [chart, momentumChart, deltaChart, bidAskFlowChart]
  chartEngines.forEach((engine, index) => {
    const chartId = DEFAULT_CHART_ORDER[index]
    if (chartId && chartEnabled.value[chartId]) {
      engine?.core.resize()
    } else {
      engine?.core.pauseLoop()
    }
  })
})

onBeforeUnmount(() => {
  feedGeneration++
  feedAbortController?.abort()
  feedAbortController = undefined
  unsubscribeChartEvents?.()
  unsubscribeChartEvents = undefined
  stopWebSocket()
  chart?.api.destroy()
  momentumChart?.api.destroy()
  deltaChart?.api.destroy()
  bidAskFlowChart?.api.destroy()
  chart = undefined
  momentumChart = undefined
  deltaChart = undefined
  bidAskFlowChart = undefined
  series = undefined
  momentumSeries = undefined
  attackBubbleSeries = undefined
  deltaCandleSeries = undefined
  bidAskFlowSeries = undefined
  deltaProfileSeries = undefined
})
</script>

<template>
  <Teleport to="body">
    <div
      v-if="!hasSelectedInitialTimeframe"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
    >
      <section
        class="w-full max-w-xl rounded-2xl border border-default bg-default p-6 shadow-2xl"
        role="dialog"
        aria-modal="true"
        aria-labelledby="initial-timeframe-title"
        aria-describedby="initial-timeframe-description"
      >
        <p class="text-sm font-medium text-primary">
          Configuración del chart
        </p>
        <h2
          id="initial-timeframe-title"
          class="mt-2 text-2xl font-semibold text-highlighted"
        >
          ¿Qué timeframe quieres cargar?
        </h2>
        <p
          id="initial-timeframe-description"
          class="mt-2 text-sm text-muted"
        >
          El historial y los feeds en vivo se cargarán después de confirmar. 5m está preseleccionado; puedes elegir otro timeframe.
        </p>

        <div
          class="mt-5 grid grid-cols-2 gap-2 sm:grid-cols-4"
          role="group"
          aria-label="Timeframes disponibles"
        >
          <button
            v-for="option in timeframeOptions"
            :key="option.value"
            type="button"
            class="min-h-11 rounded-lg border px-4 text-sm font-semibold transition-colors"
            :class="initialTimeframe === option.value
              ? 'border-primary bg-primary/10 text-highlighted'
              : 'border-default bg-elevated/30 text-muted hover:bg-elevated/60'"
            :aria-pressed="initialTimeframe === option.value"
            @click="initialTimeframe = option.value"
          >
            {{ option.label }}
          </button>
        </div>

        <div class="mt-6 flex justify-end">
          <button
            type="button"
            class="min-h-10 rounded-lg bg-primary px-5 text-sm font-semibold text-inverted"
            @click="startChartWithSelectedTimeframe"
          >
            Cargar chart {{ initialTimeframe }}
          </button>
        </div>
      </section>
    </div>
  </Teleport>

  <section
    class="flex min-h-screen flex-col gap-4 p-4 sm:p-6"
    :inert="!hasSelectedInitialTimeframe"
    aria-labelledby="chart-title"
  >
    <header class="flex flex-col gap-4">
      <div>
        <p class="text-sm font-medium text-muted">
          Workspace
        </p>
        <h1
          id="chart-title"
          class="mt-1 text-2xl font-semibold text-highlighted"
        >
          Chart
        </h1>
      </div>

      <nav
        class="flex w-fit gap-1 rounded-lg border border-default bg-elevated/30 p-1"
        role="tablist"
        aria-label="Vista del chart"
      >
        <button
          id="charts-tab"
          type="button"
          role="tab"
          aria-controls="charts-panel"
          :aria-selected="activeTab === 'charts'"
          class="rounded-md px-4 py-2 text-sm font-medium transition-colors"
          :class="activeTab === 'charts' ? 'bg-primary/10 text-primary' : 'text-muted hover:text-highlighted'"
          @click="activeTab = 'charts'"
        >
          Charts
        </button>
        <button
          id="settings-tab"
          type="button"
          role="tab"
          aria-controls="settings-panel"
          :aria-selected="activeTab === 'settings'"
          class="rounded-md px-4 py-2 text-sm font-medium transition-colors"
          :class="activeTab === 'settings' ? 'bg-primary/10 text-primary' : 'text-muted hover:text-highlighted'"
          @click="activeTab = 'settings'"
        >
          Settings
        </button>
      </nav>

      <div
        v-show="activeTab === 'settings'"
        id="settings-panel"
        class="flex flex-col gap-3"
        role="tabpanel"
        aria-labelledby="settings-tab"
        tabindex="0"
      >
        <p class="text-sm text-muted">
          Configura el mercado y los indicadores que aparecen en Charts.
        </p>
        <form
          class="flex flex-wrap items-end gap-3 rounded-xl border border-default bg-elevated/30 p-3"
          @submit.prevent="applySettings"
        >
          <label class="flex min-w-32 flex-col gap-1 text-xs font-medium text-muted">
            Source
            <select
              v-model="draftSource"
              class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
            >
              <option
                v-for="option in sourceOptions"
                :key="option.value"
                :value="option.value"
              >
                {{ option.label }}
              </option>
            </select>
          </label>

          <label class="flex min-w-36 flex-col gap-1 text-xs font-medium text-muted">
            Symbol
            <input
              v-model="draftSymbol"
              class="min-h-9 rounded-md border border-default bg-default px-3 text-sm uppercase text-highlighted"
              autocomplete="off"
              spellcheck="false"
              placeholder="BTCUSDT"
            >
          </label>

          <label class="flex min-w-28 flex-col gap-1 text-xs font-medium text-muted">
            Timeframe
            <select
              v-model="draftTimeframe"
              class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
            >
              <option
                v-for="option in timeframeOptions"
                :key="option.value"
                :value="option.value"
              >
                {{ option.label }}
              </option>
            </select>
          </label>

          <button
            type="submit"
            class="min-h-9 rounded-md bg-primary px-4 text-sm font-semibold text-inverted transition-opacity disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="!canApply"
          >
            {{ isLoading ? 'Cargando…' : 'Aplicar' }}
          </button>

          <span class="ml-auto flex items-center gap-2 pb-2 text-xs text-muted">
            <span
              class="size-2 rounded-full"
              :class="{
                'bg-success': socketStatus === 'connected',
                'bg-warning': socketStatus === 'connecting',
                'bg-error': socketStatus === 'error',
                'bg-muted': socketStatus === 'disconnected'
              }"
            />
            {{ socketStatus === 'connected' ? 'En vivo' : socketStatus === 'connecting' ? 'Conectando' : socketStatus === 'error' ? 'WebSocket error' : 'Desconectado' }}
          </span>
        </form>

        <details class="rounded-xl border border-default bg-elevated/20 p-3">
          <summary class="cursor-pointer text-sm font-semibold text-highlighted">
            DELTA PROFILE — ROLLING WINDOW
            <span class="ml-2 text-xs font-normal text-muted">
              Fuente {{ timeframe }} · {{ profileSettings.rollingWindowMinutes }} min · {{ profileBarCount }} Bars
            </span>
          </summary>

          <form
            class="mt-4 flex flex-col gap-4"
            @submit.prevent="applyDeltaProfileSettings"
          >
            <div class="flex flex-wrap items-end gap-3">
              <label class="flex min-w-40 flex-col gap-1 text-xs font-medium text-muted">
                Rolling Window
                <select
                  v-model="profileWindowPreset"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  @change="selectDeltaProfileWindow"
                >
                  <option
                    v-for="option in rollingWindowOptions"
                    :key="option.value"
                    :value="option.value"
                  >
                    {{ option.label }}
                  </option>
                </select>
              </label>

              <label
                v-if="profileWindowPreset === 'custom'"
                class="flex w-32 flex-col gap-1 text-xs font-medium text-muted"
              >
                Minutos (1–240)
                <input
                  v-model.number="customProfileWindow"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="1"
                  max="240"
                >
              </label>

              <label class="flex w-36 flex-col gap-1 text-xs font-medium text-muted">
                Value Area (%)
                <input
                  v-model.number="profileSettings.valueAreaPercentage"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="1"
                  max="100"
                  step="1"
                >
              </label>

              <label class="flex w-36 flex-col gap-1 text-xs font-medium text-muted">
                Tick Size (opcional)
                <input
                  v-model="profileSettings.tickSize"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  inputmode="decimal"
                  placeholder="Precio original"
                >
              </label>

              <label class="flex min-w-36 flex-col gap-1 text-xs font-medium text-muted">
                Escala visual
                <select
                  v-model="profileSettings.scaleMode"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                >
                  <option value="linear">Lineal</option>
                  <option value="logarithmic">Logarítmica</option>
                </select>
              </label>
            </div>

            <div class="flex flex-wrap gap-x-5 gap-y-2 text-xs text-muted">
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.showBuyVolume"
                  type="checkbox"
                >
                Buy Volume
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.showSellVolume"
                  type="checkbox"
                >
                Sell Volume
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.showDelta"
                  type="checkbox"
                >
                Delta
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.showDeltaPercentage"
                  type="checkbox"
                >
                Delta %
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.showPoc"
                  type="checkbox"
                >
                POC
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.showValueArea"
                  type="checkbox"
                >
                Value Area
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.showMaxPositiveDelta"
                  type="checkbox"
                >
                Max Δ positivo
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.showMaxNegativeDelta"
                  type="checkbox"
                >
                Max Δ negativo
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.showLabels"
                  type="checkbox"
                >
                Etiquetas
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.includeFormingBar"
                  type="checkbox"
                >
                Incluir Bar en formación
              </label>
              <label class="flex items-center gap-2">
                <input
                  v-model="profileSettings.autoUpdate"
                  type="checkbox"
                >
                Actualizar automáticamente
              </label>
            </div>

            <div class="flex flex-wrap items-center gap-3">
              <button
                type="submit"
                class="min-h-9 rounded-md bg-primary px-4 text-sm font-semibold text-inverted"
              >
                Aplicar perfil
              </button>
              <button
                v-if="!profileSettings.autoUpdate"
                type="button"
                class="min-h-9 rounded-md border border-default px-4 text-sm font-medium text-highlighted"
                @click="refreshDeltaProfile(true)"
              >
                Actualizar ahora
              </button>
              <span class="text-xs text-muted">
                Usa el footprint de Bars {{ timeframe }} del feed principal; incluye la Bar live mientras está en formación.
              </span>
            </div>
          </form>
        </details>

        <details
          class="rounded-xl border border-default bg-elevated/20 p-3"
        >
          <summary class="cursor-pointer text-sm font-semibold text-highlighted">
            ATAQUES ALCISTA/BAJISTA
            <span class="ml-2 text-xs font-normal text-muted">
              {{ attackBubbleCount }} señales · flujo agresor + confirmación de precio
            </span>
          </summary>

          <form
            class="mt-4 flex flex-col gap-4"
            @submit.prevent="applyAttackBubbleSettings"
          >
            <div class="flex flex-wrap items-end gap-3">
              <label class="flex w-40 flex-col gap-1 text-xs font-medium text-muted">
                Dominancia mínima (%)
                <input
                  v-model.number="attackMinDominancePercent"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="10"
                  max="80"
                  step="5"
                >
              </label>

              <label class="flex w-40 flex-col gap-1 text-xs font-medium text-muted">
                Volumen relativo mínimo (×)
                <input
                  v-model.number="attackMinRelativeVolume"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="1"
                  max="3"
                  step="0.1"
                >
              </label>

              <label class="flex w-40 flex-col gap-1 text-xs font-medium text-muted">
                Ventana de referencia (barras)
                <input
                  v-model.number="attackBaselineBars"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="5"
                  max="50"
                  step="1"
                >
              </label>

              <label class="flex min-h-9 items-center gap-2 pb-2 text-xs font-medium text-muted">
                <input
                  v-model="showAttackBubbles"
                  type="checkbox"
                  @change="toggleAttackBubbles"
                >
                Mostrar burbujas
              </label>
            </div>

            <div class="flex flex-wrap items-center gap-3">
              <button
                type="submit"
                class="min-h-9 rounded-md bg-primary px-4 text-sm font-semibold text-inverted"
              >
                Aplicar indicador
              </button>
              <span class="text-xs text-muted">
                El precio debe cerrar en el sentido del flujo; mínimo 5 barras para establecer la referencia.
              </span>
            </div>
          </form>
        </details>

        <details class="rounded-xl border border-default bg-elevated/20 p-3">
          <summary class="cursor-pointer text-sm font-semibold text-highlighted">
            MOMENTUM ALCISTA/BAJISTA
            <span class="ml-2 text-xs font-normal text-muted">
              {{ momentumPointCount }} puntos · {{ momentumDoubtfulCount }} barras dudosas
            </span>
          </summary>

          <form
            class="mt-4 flex flex-col gap-4"
            @submit.prevent="applyMomentumSettings"
          >
            <div class="flex flex-wrap items-end gap-3">
              <label class="flex w-36 flex-col gap-1 text-xs font-medium text-muted">
                Suavizado de señal
                <input
                  v-model.number="momentumSettings.signalPeriod"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="1"
                  max="500"
                  step="1"
                >
              </label>

              <label class="flex w-36 flex-col gap-1 text-xs font-medium text-muted">
                Referencia de intensidad
                <input
                  v-model.number="momentumSettings.basePeriod"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="1"
                  max="2000"
                  step="1"
                >
              </label>

              <label class="flex w-32 flex-col gap-1 text-xs font-medium text-muted">
                Peso del flujo (α)
                <input
                  v-model.number="momentumSettings.alpha"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="0"
                  max="1"
                  step="0.05"
                >
              </label>

              <label class="flex w-32 flex-col gap-1 text-xs font-medium text-muted">
                Tope de intensidad
                <input
                  v-model.number="momentumSettings.maxIntensity"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="0"
                  max="100"
                  step="0.1"
                >
              </label>

              <label class="flex w-32 flex-col gap-1 text-xs font-medium text-muted">
                Calentamiento
                <input
                  v-model.number="momentumSettings.warmup"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                  type="number"
                  min="0"
                  max="2000"
                  step="1"
                >
              </label>

              <label class="flex w-32 flex-col gap-1 text-xs font-medium text-muted">
                Unidad timestamp
                <select
                  v-model="momentumSettings.timestampUnit"
                  class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
                >
                  <option value="s">s</option>
                  <option value="ms">ms</option>
                  <option value="us">us</option>
                  <option value="ns">ns</option>
                </select>
              </label>
            </div>

            <div class="flex flex-wrap items-center gap-3">
              <button
                type="submit"
                class="min-h-9 rounded-md bg-primary px-4 text-sm font-semibold text-inverted"
              >
                Aplicar indicador
              </button>
              <span class="text-xs text-muted">
                Buy notional representa compras agresoras; las magnitudes de precio son movimientos tick a tick. Alcista &gt; bajista favorece al alza; ambas altas pueden indicar conflicto o alternancia. No es predictivo.
              </span>
            </div>
          </form>
        </details>

        <section class="flex flex-wrap items-end gap-4 rounded-xl border border-default bg-elevated/20 p-3">
          <div class="flex min-w-48 flex-col gap-1">
            <h2 class="text-sm font-semibold text-highlighted">
              Velas de delta
            </h2>
            <p class="text-xs text-muted">
              {{ deltaCandleCount }} / {{ DELTA_HISTORY_LIMIT }} Bars
            </p>
          </div>
          <label class="flex flex-col gap-1 text-xs font-medium text-muted">
            Modo
            <select
              v-model="deltaMode"
              class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
              @change="applyDeltaMode"
            >
              <option value="PER_BAR">Per bar</option>
              <option value="CVD">CVD</option>
            </select>
          </label>
        </section>

        <section class="flex flex-wrap items-end gap-4 rounded-xl border border-default bg-elevated/20 p-3">
          <div class="flex min-w-48 flex-col gap-1">
            <h2 class="text-sm font-semibold text-highlighted">
              Flujo bid/ask acumulado
            </h2>
            <p class="text-xs text-muted">
              {{ bidAskFlowCandleCount }} / {{ DELTA_HISTORY_LIMIT }} Bars
            </p>
          </div>
          <label class="flex flex-col gap-1 text-xs font-medium text-muted">
            Estilo
            <select
              v-model="flowCandleStyle"
              class="min-h-9 rounded-md border border-default bg-default px-3 text-sm text-highlighted"
              @change="applyFlowCandleStyle"
            >
              <option value="CANDLE">Velas</option>
              <option value="OHLC_BAR">OHLC</option>
            </select>
          </label>
        </section>
      </div>
    </header>

    <p
      v-if="errorMessage"
      class="rounded-lg border border-error/30 bg-error/10 px-3 py-2 text-sm text-error"
      role="alert"
    >
      {{ errorMessage }}
    </p>

    <p
      v-if="profileDiagnostic"
      class="rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning"
      role="status"
    >
      {{ profileDiagnostic }}
    </p>

    <p
      v-if="momentumDiagnostic"
      class="rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning"
      role="status"
    >
      {{ momentumDiagnostic }}
    </p>

    <div
      v-show="activeTab === 'charts'"
      id="charts-panel"
      class="flex flex-col gap-4"
      role="tabpanel"
      aria-labelledby="charts-tab"
      tabindex="0"
    >
      <div class="flex flex-wrap items-center justify-between gap-2 text-xs text-muted">
        <span>{{ source }} · {{ symbol }} · {{ timeframe }}</span>
        <div class="flex flex-wrap items-center gap-3">
          <span class="flex items-center gap-2 rounded-full border border-default bg-elevated/50 px-2 py-1">
            <span
              class="size-2 rounded-full"
              :class="{
                'bg-success': socketStatus === 'connected',
                'bg-warning': socketStatus === 'connecting',
                'bg-error': socketStatus === 'error',
                'bg-muted': socketStatus === 'disconnected'
              }"
            />
            {{ socketStatus === 'connected' ? 'En vivo' : socketStatus === 'connecting' ? 'Conectando' : socketStatus === 'error' ? 'WebSocket error' : 'Desconectado' }}
          </span>
          <span class="rounded-full border border-default bg-elevated/50 px-2 py-1">
            Delta Profile {{ timeframe }} · {{ profileSettings.rollingWindowMinutes }}m · {{ profileBarCount }} Bars
          </span>
          <span class="rounded-full border border-default bg-elevated/50 px-2 py-1">
            Ataques · {{ attackBubbleCount }}
          </span>
          <span class="rounded-full border border-default bg-elevated/50 px-2 py-1">
            Momentum · {{ momentumPointCount }} puntos
          </span>
          <span
            v-if="latestBarState !== 'none'"
            class="rounded-full border px-2 py-1"
            :class="latestBarState === 'live'
              ? 'border-warning/40 bg-warning/10 text-warning'
              : 'border-default bg-elevated/50 text-muted'"
            role="status"
          >
            {{ latestBarState === 'live' ? 'Bar en formación' : 'Última Bar cerrada' }}
          </span>
          <span>{{ barCount }} / {{ HISTORY_LIMIT }} barras {{ timeframe }}</span>
        </div>
      </div>

      <div class="flex flex-col gap-4">
        <section
          v-for="(chartId, index) in chartOrder"
          :key="chartId"
          class="relative flex flex-col gap-3"
          :aria-labelledby="`${chartId}-chart-title`"
        >
          <header class="relative z-20 flex items-start justify-between gap-3">
            <div>
              <h2
                :id="`${chartId}-chart-title`"
                class="text-base font-semibold text-highlighted"
              >
                {{ chartLabels[chartId] }}
              </h2>
              <p
                v-if="chartId === 'momentum'"
                class="mt-1 text-xs text-muted"
              >
                Flujo y eficiencia del precio ponderados por intensidad · línea alcista continua · bajista discontinua · marcas naranjas = integridad dudosa
              </p>
              <p
                v-else-if="chartId === 'delta'"
                class="mt-1 text-xs text-muted"
              >
                Unidad qty · solo cantidades de raw_ticks · {{ deltaCandleCount }} / {{ DELTA_HISTORY_LIMIT }} Bars
              </p>
              <p
                v-else-if="chartId === 'bidAskFlow'"
                class="mt-1 text-xs text-muted"
              >
                Unidad qty · OHLC acumulado desde la primera barra cargada (base 0) · solo raw_ticks · {{ bidAskFlowCandleCount }} / {{ DELTA_HISTORY_LIMIT }} Bars
              </p>
            </div>

            <div
              class="flex shrink-0 items-center gap-1"
              role="group"
              :aria-label="`Controles de ${chartLabels[chartId]}`"
            >
              <button
                type="button"
                class="inline-flex min-h-9 items-center gap-2 rounded-md border border-default bg-elevated/30 px-3 text-sm text-highlighted transition-colors hover:bg-elevated/60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
                :class="chartEnabled[chartId] ? 'text-success' : 'text-muted'"
                :aria-label="`${chartEnabled[chartId] ? 'Apagar' : 'Encender'} ${chartLabels[chartId]}`"
                :aria-pressed="chartEnabled[chartId]"
                :title="`${chartEnabled[chartId] ? 'Apagar' : 'Encender'} ${chartLabels[chartId]}`"
                @click="toggleChart(chartId)"
              >
                <UIcon
                  name="i-lucide-power"
                  class="size-4"
                  aria-hidden="true"
                />
                {{ chartEnabled[chartId] ? 'Apagar' : 'Encender' }}
              </button>
              <button
                type="button"
                class="size-9 rounded-md border border-default bg-elevated/30 text-highlighted transition-colors hover:bg-elevated/60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:opacity-40"
                :aria-label="`Mover ${chartLabels[chartId]} hacia arriba`"
                :disabled="index === 0"
                @click="moveChart(chartId, -1)"
              >
                <span aria-hidden="true">↑</span>
              </button>
              <button
                type="button"
                class="size-9 rounded-md border border-default bg-elevated/30 text-highlighted transition-colors hover:bg-elevated/60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:opacity-40"
                :aria-label="`Mover ${chartLabels[chartId]} hacia abajo`"
                :disabled="index === chartOrder.length - 1"
                @click="moveChart(chartId, 1)"
              >
                <span aria-hidden="true">↓</span>
              </button>
            </div>
          </header>

          <div
            v-if="chartId === 'ohlcv'"
            class="relative box-border h-[clamp(420px,65vh,720px)] w-full flex-none overflow-hidden rounded-xl border border-default bg-elevated/30"
          >
            <div
              id="ohlcv-chart"
              :ref="chartHostRefCallbacks.ohlcv"
              class="h-full w-full"
              aria-label="Gráfico de velas OHLCV"
            />
            <div
              v-if="isLoading && barCount === 0"
              class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-default/70 text-sm text-muted"
              role="status"
            >
              Cargando histórico de {{ symbol }}…
            </div>
            <div
              v-else-if="!isLoading && barCount === 0"
              class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center text-sm text-muted"
            >
              No hay barras históricas para mostrar.
            </div>
          </div>

          <div
            v-else-if="chartId === 'momentum'"
            class="relative box-border h-[clamp(220px,30vh,320px)] w-full flex-none overflow-hidden rounded-xl border border-default bg-elevated/30"
          >
            <div
              id="momentum-chart"
              :ref="chartHostRefCallbacks.momentum"
              class="h-full w-full"
              aria-label="Indicador de momentum alcista y bajista"
            />
            <div
              v-if="momentumPointCount === 0"
              class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-default/50 px-4 text-center text-sm text-muted"
              role="status"
            >
              El indicador necesita datos microestructurales válidos y {{ momentumSettings.warmup + 1 }} barras para mostrar las líneas.
            </div>
          </div>

          <div
            v-else-if="chartId === 'delta'"
            class="relative box-border h-[clamp(260px,35vh,380px)] w-full flex-none overflow-hidden rounded-xl border border-default bg-elevated/30"
          >
            <div
              id="delta-chart"
              :ref="chartHostRefCallbacks.delta"
              class="h-full w-full"
              aria-label="Gráfico de velas de delta"
            />
            <div
              v-if="deltaCandleCount === 0"
              class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-default/50 px-4 text-center text-sm text-muted"
              role="status"
            >
              Esperando Bars con cantidades válidas en raw_ticks de {{ symbol }}…
            </div>
          </div>

          <div
            v-else
            class="relative box-border h-[clamp(260px,35vh,380px)] w-full flex-none overflow-hidden rounded-xl border border-default bg-elevated/30"
          >
            <div
              id="bid-ask-flow-chart"
              :ref="chartHostRefCallbacks.bidAskFlow"
              class="h-full w-full"
              aria-label="Gráfico OHLC de flujo bid/ask acumulado en cantidades"
            />
            <div
              v-if="bidAskFlowCandleCount === 0"
              class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-default/50 px-4 text-center text-sm text-muted"
              role="status"
            >
              Esperando Bars con cantidades válidas en raw_ticks de {{ symbol }}…
            </div>
          </div>
          <div
            v-if="!chartEnabled[chartId]"
            class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-xl bg-default/50 text-sm font-medium text-highlighted"
            role="status"
          >
            {{ chartLabels[chartId] }} apagado
          </div>
        </section>
      </div>
    </div>
  </section>
</template>
