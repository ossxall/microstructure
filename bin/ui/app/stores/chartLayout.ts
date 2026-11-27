import { defineStore } from 'pinia'
import { ref } from 'vue'

export type ChartId = 'ohlcv' | 'momentum' | 'delta' | 'bidAskFlow'

export const DEFAULT_CHART_ORDER: ChartId[] = ['ohlcv', 'momentum', 'delta', 'bidAskFlow']

const chartIds = new Set<ChartId>(DEFAULT_CHART_ORDER)

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

  function setChartOrder(order: ChartId[]) {
    chartOrder.value = normalizeChartOrder(order)
  }

  return {
    chartOrder,
    setChartOrder
  }
}, {
  persist: {
    key: 'chart-layout',
    storage: piniaPluginPersistedstate.localStorage(),
    pick: ['chartOrder']
  }
})
