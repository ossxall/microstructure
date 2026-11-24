<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, computed, reactive } from 'vue'
import { createChart } from '~/packages/open-charts/src'
import type { AnyChartSeries, ChartEngine } from '~/packages/open-charts/src/core/types'
import type { Bar } from '~/utils/bar'
import { createOhlcvSeries } from '~/utils/ohlcvSeries'
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
const DELTA_PROFILE_TIMEFRAME = '1m'
const HISTORY_REQUEST_TIMEOUT_MS = 25_000
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
const deltaChartHost = ref<HTMLElement | null>(null)
const bidAskFlowChartHost = ref<HTMLElement | null>(null)
const source = ref<MarketSource>('binance')
const symbol = ref('BTCUSDT')
const timeframe = ref<Timeframe>('1m')
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
const canApply = computed(() => {
  return !isLoading.value && (
    draftSource.value !== source.value
    || draftSymbol.value.trim().toUpperCase() !== symbol.value
    || draftTimeframe.value !== timeframe.value
  )
})

const config = useRuntimeConfig()
const bars = new Map<number, Bar>()
const deltaBars = new Map<number, DeltaBar>()
const deltaProfile = new RollingDeltaProfile(profileSettings, (message) => {
  profileDiagnostic.value = message
  console.warn(message)
})
let chart: ChartEngine | undefined
let deltaChart: ChartEngine | undefined
let bidAskFlowChart: ChartEngine | undefined
let series: AnyChartSeries | undefined
let deltaCandleSeries: AnyChartSeries | undefined
let bidAskFlowSeries: AnyChartSeries | undefined
let deltaProfileSeries: AnyChartSeries | undefined
let socket: WebSocket | undefined
let profileSocket: WebSocket | undefined
let reconnectTimer: ReturnType<typeof setTimeout> | undefined
let profileReconnectTimer: ReturnType<typeof setTimeout> | undefined
let feedGeneration = 0
let seriesHydrated = false

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
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
    time: timestamp / 1000,
    open,
    high,
    low,
    close,
    volume,
    revision: Number.isFinite(revision) ? revision : 0
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
    time: timestamp / 1000,
    revision: Number.isFinite(revision) ? revision : 0,
    ...(header ? { header } : {}),
    ...(rawTicks ? { raw_ticks: rawTicks } : {})
  }
}

function upsertBar(bar: Bar): boolean {
  const current = bars.get(bar.time)

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

  bars.set(bar.time, bar)

  if (bars.size > HISTORY_LIMIT) {
    const oldestTimes = [...bars.keys()]
      .sort((left, right) => left - right)
      .slice(0, bars.size - HISTORY_LIMIT)

    oldestTimes.forEach(time => bars.delete(time))
  }

  barCount.value = bars.size
  updateLatestBarState()
  return bars.has(bar.time)
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
  series?.destroy({ silent: true })
  series = chart.api.addSeries(createOhlcvSeries())
  deltaProfileSeries = chart.api.addSeries(createDeltaProfileSeries(deltaProfile))
}

function replaceDeltaCandleSeries() {
  if (!deltaChart) {
    return
  }

  deltaCandleSeries?.destroy({ silent: true })
  deltaCandleSeries = deltaChart.api.addSeries(createDeltaCandleSeries())
  deltaCandleSeries.setParams({ mode: deltaMode.value })
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

  deltaBars.set(bar.time, bar)
  const oldestTimes = [...deltaBars.keys()]
    .sort((left, right) => left - right)
    .slice(0, Math.max(0, deltaBars.size - DELTA_HISTORY_LIMIT))
  oldestTimes.forEach(time => deltaBars.delete(time))

  deltaCandleSeries?.upsert(bar, { maxDataPoints: DELTA_HISTORY_LIMIT })
  bidAskFlowSeries?.upsert(bar, { maxDataPoints: DELTA_HISTORY_LIMIT })
  refreshDeltaCandleCount()
  refreshBidAskFlowCandleCount()
  return true
}

function applyDeltaMode() {
  deltaCandleSeries?.setParams({ mode: deltaMode.value })
  refreshDeltaCandleCount()
}

function applyFlowCandleStyle() {
  bidAskFlowSeries?.setParams({ style: flowCandleStyle.value })
}

function renderCurrentBars() {
  const currentBars = [...bars.values()].sort((left, right) => left.time - right.time)

  if (!series) {
    return
  }

  if (currentBars.length > 0) {
    // Full replacement is reserved for initial history hydration.
    series.setData(currentBars)
    deltaProfileSeries?.setData(currentBars)
  }

  seriesHydrated = true
  refreshDeltaProfile(true)
}

async function fetchMarketHistory(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: string,
  options: { limit?: number, includeRawTicks?: boolean } = {}
): Promise<Bar[]> {
  const response = await $fetch<HistoryResponse>('/api/market/get-history', {
    method: 'POST',
    timeout: HISTORY_REQUEST_TIMEOUT_MS,
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
    .map(parseMarketBar)
    .filter((bar): bar is Bar => bar !== null)
}

async function fetchDeltaHistory(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: string
): Promise<DeltaBar[]> {
  const response = await $fetch<HistoryResponse>('/api/market/get-history', {
    method: 'POST',
    timeout: HISTORY_REQUEST_TIMEOUT_MS,
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
    .map(parseDeltaBar)
    .filter((bar): bar is DeltaBar => bar !== null)
}

function refreshDeltaProfile(force = false) {
  profileBarCount.value = deltaProfile.activeBarCount

  if (!force && !profileSettings.autoUpdate) {
    return
  }

  if (chart) {
    chart.dirty = true
    chart.overlayDirty = true
  }
}

function upsertDeltaProfileBar(bar: Bar): boolean {
  const changed = deltaProfile.upsert(bar)
  if (changed) {
    refreshDeltaProfile()
  }
  return changed
}

async function resyncDeltaProfileHistory(
  nextSource: MarketSource,
  nextSymbol: string,
  generation: number,
  limit = profileSettings.rollingWindowMinutes
) {
  try {
    const history = await fetchMarketHistory(
      nextSource,
      nextSymbol,
      DELTA_PROFILE_TIMEFRAME,
      { limit, includeRawTicks: false }
    )

    if (generation !== feedGeneration) {
      return
    }

    history.sort((left, right) => left.time - right.time)
    for (const bar of history) {
      deltaProfile.upsert(bar)
    }
    refreshDeltaProfile()
  } catch (error) {
    if (generation === feedGeneration) {
      profileDiagnostic.value = error instanceof Error
        ? `No se pudo resincronizar el histórico 1m del Delta Profile: ${error.message}`
        : 'No se pudo resincronizar el histórico 1m del Delta Profile'
    }
  }
}

function stopDeltaProfileWebSocket() {
  if (profileReconnectTimer) {
    clearTimeout(profileReconnectTimer)
    profileReconnectTimer = undefined
  }

  const currentSocket = profileSocket
  profileSocket = undefined
  currentSocket?.close()
}

function connectDeltaProfileWebSocket(
  nextSource: MarketSource,
  nextSymbol: string,
  generation: number,
  resyncAfterConnect = false
) {
  if (generation !== feedGeneration) {
    return
  }

  try {
    const nextSocket = new WebSocket(String(config.public.marketWebSocketUrl))
    profileSocket = nextSocket

    nextSocket.addEventListener('open', () => {
      if (profileSocket !== nextSocket || generation !== feedGeneration) {
        return
      }

      nextSocket.send(JSON.stringify({
        action: 'subscribe',
        source: nextSource,
        symbol: nextSymbol,
        timeframe: DELTA_PROFILE_TIMEFRAME
      }))
    })

    nextSocket.addEventListener('message', (event: MessageEvent<string>) => {
      if (generation !== feedGeneration) {
        return
      }

      try {
        const message: unknown = JSON.parse(event.data)

        if (
          isRecord(message)
          && message.type === 'subscribed'
          && message.source === nextSource
          && message.symbol === nextSymbol
          && message.timeframe === DELTA_PROFILE_TIMEFRAME
        ) {
          if (resyncAfterConnect) {
            void resyncDeltaProfileHistory(
              nextSource,
              nextSymbol,
              generation,
              profileSettings.rollingWindowMinutes
            )
          }
          return
        }

        if (isRecord(message) && message.type === 'error') {
          profileDiagnostic.value = typeof message.message === 'string'
            ? `Feed 1m del Delta Profile: ${message.message}`
            : 'El feed 1m del Delta Profile rechazó la suscripción'
          return
        }

        const deltaBar = parseDeltaBar(message)
        if (deltaBar && timeframe.value === DELTA_PROFILE_TIMEFRAME) {
          upsertDeltaBar(deltaBar)
        }

        const bar = parseMarketBar(message)
        if (bar) {
          upsertDeltaProfileBar(bar)
        }
      } catch {
        profileDiagnostic.value = 'El feed 1m envió un mensaje inválido al Delta Profile'
      }
    })

    nextSocket.addEventListener('close', () => {
      if (profileSocket !== nextSocket || generation !== feedGeneration) {
        return
      }

      profileSocket = undefined
      profileReconnectTimer = setTimeout(() => {
        connectDeltaProfileWebSocket(nextSource, nextSymbol, generation, true)
      }, 1500)
    })
  } catch {
    profileDiagnostic.value = 'No se pudo conectar el feed 1m del Delta Profile'
  }
}

async function resyncMarketHistory(
  nextSource: MarketSource,
  nextSymbol: string,
  nextTimeframe: Timeframe,
  generation: number
) {
  try {
    const history = await fetchMarketHistory(nextSource, nextSymbol, nextTimeframe, {
      includeRawTicks: false
    })

    if (generation !== feedGeneration) {
      return
    }

    const changedBars = history.filter(upsertBar)
    if (!seriesHydrated || !series) {
      return
    }

    // Apply bars oldest-first: missed bars then take the cheap append path,
    // while any late revision is recomputed before newer bars are appended.
    changedBars.sort((left, right) => left.time - right.time)
    for (const bar of changedBars) {
      series.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
      deltaProfileSeries?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
    }

    if (errorMessage.value.startsWith('Resincronización:')) {
      errorMessage.value = ''
    }
  } catch (error) {
    if (generation === feedGeneration) {
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
  try {
    const history = await fetchDeltaHistory(nextSource, nextSymbol, nextTimeframe)

    if (generation !== feedGeneration) {
      return
    }

    history.sort((left, right) => left.time - right.time)
    for (const bar of history) {
      upsertDeltaBar(bar)
    }
  } catch (error) {
    if (generation === feedGeneration) {
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
  if (generation !== feedGeneration) {
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
        const message: unknown = JSON.parse(event.data)

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
            void resyncDeltaHistory(
              nextSource,
              nextSymbol,
              nextTimeframe,
              generation
            )
          }
          return
        }

        if (isRecord(message) && message.type === 'error') {
          errorMessage.value = typeof message.message === 'string'
            ? message.message
            : 'El servidor rechazó la suscripción'
          return
        }

        const deltaBar = parseDeltaBar(message)
        if (deltaBar) {
          upsertDeltaBar(deltaBar)
        }

        const bar = parseMarketBar(message)

        if (bar) {
          if (upsertBar(bar) && seriesHydrated) {
            series?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
            deltaProfileSeries?.upsert(bar, { maxDataPoints: HISTORY_LIMIT })
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
  const generation = ++feedGeneration
  isLoading.value = true
  errorMessage.value = ''
  seriesHydrated = false
  bars.clear()
  deltaBars.clear()
  deltaCandleCount.value = 0
  bidAskFlowCandleCount.value = 0
  barCount.value = 0
  latestBarState.value = 'none'
  profileDiagnostic.value = ''
  profileBarCount.value = 0
  deltaProfile.reset(nextSymbol, nextSource)
  stopWebSocket()
  stopDeltaProfileWebSocket()
  replaceSeries()
  replaceDeltaCandleSeries()
  replaceBidAskFlowSeries()
  connectWebSocket(nextSource, nextSymbol, nextTimeframe, generation)
  connectDeltaProfileWebSocket(nextSource, nextSymbol, generation)
  void resyncDeltaHistory(nextSource, nextSymbol, nextTimeframe, generation)

  try {
    const history = await fetchMarketHistory(nextSource, nextSymbol, nextTimeframe, {
      includeRawTicks: false
    })

    if (generation !== feedGeneration) {
      return
    }

    for (const bar of history) {
      upsertBar(bar)
      if (nextTimeframe === DELTA_PROFILE_TIMEFRAME) {
        deltaProfile.upsert(bar)
      }
    }

    renderCurrentBars()
    if (nextTimeframe === DELTA_PROFILE_TIMEFRAME) {
      refreshDeltaProfile()
    }
    if (
      nextTimeframe !== DELTA_PROFILE_TIMEFRAME
      || profileSettings.rollingWindowMinutes > HISTORY_LIMIT
    ) {
      void resyncDeltaProfileHistory(nextSource, nextSymbol, generation)
    }
  } catch (error) {
    if (generation === feedGeneration) {
      errorMessage.value = error instanceof Error
        ? `Histórico: ${error.message}`
        : 'No se pudo cargar el histórico'
      renderCurrentBars()
      void resyncDeltaProfileHistory(
        nextSource,
        nextSymbol,
        generation,
        profileSettings.rollingWindowMinutes
      )
    }
  } finally {
    if (generation === feedGeneration) {
      isLoading.value = false
    }
  }
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
  deltaProfileSeries?.setParams({ ...profileSettings })
  refreshDeltaProfile(true)
  if (
    (
      timeframe.value === DELTA_PROFILE_TIMEFRAME
      && profileSettings.rollingWindowMinutes > HISTORY_LIMIT
    )
    || timeframe.value !== DELTA_PROFILE_TIMEFRAME
  ) {
    void resyncDeltaProfileHistory(
      source.value,
      symbol.value,
      feedGeneration,
      profileSettings.rollingWindowMinutes
    )
  }
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

onMounted(() => {
  if (!chartHost.value) {
    return
  }

  chart = createChart(chartHost.value)
  replaceSeries()
  if (deltaChartHost.value) {
    deltaChart = createChart(deltaChartHost.value)
    replaceDeltaCandleSeries()
  }
  if (bidAskFlowChartHost.value) {
    bidAskFlowChart = createChart(bidAskFlowChartHost.value)
    replaceBidAskFlowSeries()
  }
  void loadMarketData(source.value, symbol.value, timeframe.value)
})

onBeforeUnmount(() => {
  feedGeneration++
  stopWebSocket()
  stopDeltaProfileWebSocket()
  chart?.api.destroy()
  deltaChart?.api.destroy()
  bidAskFlowChart?.api.destroy()
  chart = undefined
  deltaChart = undefined
  bidAskFlowChart = undefined
  series = undefined
  deltaCandleSeries = undefined
  bidAskFlowSeries = undefined
  deltaProfileSeries = undefined
})
</script>

<template>
  <section
    class="flex min-h-screen flex-col gap-4 p-4 sm:p-6"
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
            Fuente 1m · {{ profileSettings.rollingWindowMinutes }} min · {{ profileBarCount }} Bars
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
              Solo footprint de Bars 1m; incluye la Bar live mientras está en formación.
            </span>
          </div>
        </form>
      </details>
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

    <div class="flex items-center justify-between text-xs text-muted">
      <span>{{ source }} · {{ symbol }} · {{ timeframe }}</span>
      <div class="flex items-center gap-3">
        <span class="rounded-full border border-default bg-elevated/50 px-2 py-1">
          Delta Profile 1m · {{ profileSettings.rollingWindowMinutes }}m · {{ profileBarCount }} Bars
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

    <div class="relative box-border h-[clamp(420px,65vh,720px)] w-full flex-none overflow-hidden rounded-xl border border-default bg-elevated/30">
      <div
        id="ohlcv-chart"
        ref="chartHost"
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

    <section
      class="flex flex-col gap-3"
      aria-labelledby="delta-chart-title"
    >
      <header class="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2
            id="delta-chart-title"
            class="text-base font-semibold text-highlighted"
          >
            VELAS DE DELTA
          </h2>
          <p class="mt-1 text-xs text-muted">
            Unidad qty · solo cantidades de raw_ticks · {{ deltaCandleCount }} / {{ DELTA_HISTORY_LIMIT }} Bars
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
      </header>

      <div class="relative box-border h-[clamp(260px,35vh,380px)] w-full flex-none overflow-hidden rounded-xl border border-default bg-elevated/30">
        <div
          id="delta-chart"
          ref="deltaChartHost"
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
    </section>

    <section
      class="flex flex-col gap-3"
      aria-labelledby="bid-ask-flow-title"
    >
      <header class="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2
            id="bid-ask-flow-title"
            class="text-base font-semibold text-highlighted"
          >
            FLUJO BID/ASK ACUMULADO
          </h2>
          <p class="mt-1 text-xs text-muted">
            Unidad qty · OHLC acumulado desde la primera barra cargada (base 0) · solo raw_ticks · {{ bidAskFlowCandleCount }} / {{ DELTA_HISTORY_LIMIT }} Bars
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
      </header>

      <div class="relative box-border h-[clamp(260px,35vh,380px)] w-full flex-none overflow-hidden rounded-xl border border-default bg-elevated/30">
        <div
          id="bid-ask-flow-chart"
          ref="bidAskFlowChartHost"
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
    </section>
  </section>
</template>
