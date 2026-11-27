import { defineStore } from 'pinia'
import { ref } from 'vue'

export type ChartId = 'ohlcv' | 'momentum' | 'delta' | 'bidAskFlow'

export const DEFAULT_CHART_ORDER: ChartId[] = ['ohlcv', 'momentum', 'delta', 'bidAskFlow']
export const DEFAULT_CHART_ENABLED: Record<ChartId, boolean> = {
  ohlcv: true,
  momentum: true,
  delta: true,
  bidAskFlow: true
}

const chartIds = new Set<ChartId>(DEFAULT_CHART_ORDER)

export function normalizeChartEnabled(value: unknown): Record<ChartId, boolean> {
  const storedValues = typeof value === 'object' && value !== null
    ? value as Partial<Record<ChartId, unknown>>
    : {}

  return DEFAULT_CHART_ORDER.reduce((enabledCharts, chartId) => {
    enabledCharts[chartId] = typeof storedValues[chartId] === 'boolean'
      ? storedValues[chartId] as boolean
      : DEFAULT_CHART_ENABLED[chartId]
    return enabledCharts
  }, {} as Record<ChartId, boolean>)
}

export function normalizeChartOrder(value: unknown): ChartId[] {
  if (!Array.isArray(value)) {
    return [...DEFAULT_CHART_ORDER]
  }

  const validIds = value.filter((id): id is ChartId => chartIds.has(id))
  const uniqueIds = [...new Set(validIds)]
  return [
    ...uniqueIds,
    ...DEFAULT_CHART_ORDER.filter(id => !uniqueIds.includes(id))
  ]
}

export const useChartLayoutStore = defineStore('chart-layout', () => {
  const chartOrder = ref<ChartId[]>([...DEFAULT_CHART_ORDER])
  const chartEnabled = ref<Record<ChartId, boolean>>({ ...DEFAULT_CHART_ENABLED })

  function setChartOrder(order: ChartId[]) {
    chartOrder.value = normalizeChartOrder(order)
  }

  function setChartEnabled(chartId: ChartId, enabled: boolean) {
    chartEnabled.value = {
      ...normalizeChartEnabled(chartEnabled.value),
      [chartId]: enabled
    }
  }

  function setChartEnabledState(value: unknown) {
    chartEnabled.value = normalizeChartEnabled(value)
  }

  return {
    chartOrder,
    chartEnabled,
    setChartOrder,
    setChartEnabled,
    setChartEnabledState
  }
}, {
  persist: {
    key: 'chart-layout',
    storage: piniaPluginPersistedstate.localStorage(),
    pick: ['chartOrder', 'chartEnabled']
  }
})
