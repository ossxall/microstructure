interface HistoryRequest {
  source: string
  symbol: string
  timeframe?: string
  limit: number
  include_raw_ticks?: boolean
}

export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const request = await readBody<HistoryRequest>(event)

  return $fetch(`${config.marketServerUrl}/get-history`, {
    method: 'POST',
    timeout: 20_000,
    body: {
      ...request,
      timeframe: request.timeframe ?? '1m',
      include_raw_ticks: request.include_raw_ticks ?? true
    }
  })
})
