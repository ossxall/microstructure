<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, computed } from 'vue'
import { createChart } from '~/packages/open-charts/src'
import type { AnyChartSeries, ChartEngine } from '~/packages/open-charts/src/core/types'
import type { Bar } from '~/utils/bar'
import { createOhlcvSeries } from '~/utils/ohlcvSeries'

type MarketSource = 'binance' | 'dydx'
type Timeframe = '1m' | '5m' | '15m' | '30m' | '1h' | '4h' | '1d'

interface HistoryResponse {
  success?: boolean
  message?: string
  data?: {
    history?: unknown[]
  }
}

const HISTORY_LIMIT = 500
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

const chartHost = ref<HTMLElement | null>(null)
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
const canApply = computed(() => {
  return !isLoading.value && (
    draftSource.value !== source.value
    || draftSymbol.value.trim().toUpperCase() !== symbol.value
    || draftTimeframe.value !== timeframe.value
  )
})

const config = useRuntimeConfig()
const bars = new Map<number, Bar>()
let chart: ChartEngine | undefined
let series: AnyChartSeries | undefined
let socket: WebSocket | undefined
let reconnectTimer: ReturnType<typeof setTimeout> | undefined
let feedGeneration = 0

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
  return true
}

function replaceSeries() {
  if (!chart) {
    return
  }

  series?.destroy({ silent: true })
  series = chart.api.addSeries(createOhlcvSeries())
}

function renderCurrentBars() {
  const currentBars = [...bars.values()].sort((left, right) => left.time - right.time)

  if (!series || currentBars.length === 0) {
    return
  }

  series.setData(currentBars)
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
  generation: number
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

        if (isRecord(message) && message.type === 'error') {
          errorMessage.value = typeof message.message === 'string'
            ? message.message
            : 'El servidor rechazó la suscripción'
          return
        }

        const bar = parseMarketBar(message)

        if (bar && upsertBar(bar)) {
          renderCurrentBars()
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
        connectWebSocket(nextSource, nextSymbol, nextTimeframe, generation)
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
  bars.clear()
  barCount.value = 0
  stopWebSocket()
  replaceSeries()
  connectWebSocket(nextSource, nextSymbol, nextTimeframe, generation)

  try {
    const response = await $fetch<HistoryResponse>('/api/market/get-history', {
      method: 'POST',
      body: {
        source: nextSource,
        symbol: nextSymbol,
        timeframe: nextTimeframe,
        limit: HISTORY_LIMIT
      }
    })

    if (generation !== feedGeneration) {
      return
    }

    if (response.success === false) {
      throw new Error(response.message || 'No se pudo cargar el histórico')
    }

    for (const rawBar of response.data?.history ?? []) {
      const bar = parseMarketBar(rawBar)

      if (bar) {
        upsertBar(bar)
      }
    }

    renderCurrentBars()
  } catch (error) {
    if (generation === feedGeneration) {
      errorMessage.value = error instanceof Error
        ? `Histórico: ${error.message}`
        : 'No se pudo cargar el histórico'
      renderCurrentBars()
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

onMounted(() => {
  if (!chartHost.value) {
    return
  }

  chart = createChart(chartHost.value)
  replaceSeries()
  void loadMarketData(source.value, symbol.value, timeframe.value)
})

onBeforeUnmount(() => {
  feedGeneration++
  stopWebSocket()
  chart?.api.destroy()
  chart = undefined
  series = undefined
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
    </header>

    <p
      v-if="errorMessage"
      class="rounded-lg border border-error/30 bg-error/10 px-3 py-2 text-sm text-error"
      role="alert"
    >
      {{ errorMessage }}
    </p>

    <div class="flex items-center justify-between text-xs text-muted">
      <span>{{ source }} · {{ symbol }} · {{ timeframe }}</span>
      <span>{{ barCount }} / {{ HISTORY_LIMIT }} barras {{ timeframe }}</span>
    </div>

    <div class="chart-view__surface relative overflow-hidden rounded-xl border border-default bg-elevated/30 sm:min-h-[520px]">
      <div
        id="ohlcv-chart"
        ref="chartHost"
        class="chart-area h-full w-full"
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
  </section>
</template>
