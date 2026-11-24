interface HistoryRequest {
  source: string
  symbol: string
  timeframe?: string
  limit: number
}

export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const request = await readBody<HistoryRequest>(event)

  return $fetch(`${config.marketServerUrl}/get-history`, {
    method: 'POST',
    body: {
      ...request,
      timeframe: request.timeframe ?? '1m'
    }
  })
})
