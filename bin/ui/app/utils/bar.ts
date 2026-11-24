export type BarValue = number | string

export interface BarPrice {
  open: BarValue
  high: BarValue
  low: BarValue
  close: BarValue
  upticks: BarValue
  downticks: BarValue
  zero_ticks: BarValue
  direction_changes: BarValue
  first_price_change: BarValue
  last_price_change: BarValue
  max_price_change: BarValue
  min_price_change: BarValue
}

export interface BarVolumeFlow {
  total_qty: BarValue
  total_notional: BarValue
  buy_qty: BarValue
  sell_qty: BarValue
  buy_notional: BarValue
  sell_notional: BarValue
  signed_qty: BarValue
  signed_notional: BarValue
  buy_count: BarValue
  sell_count: BarValue
  max_trade_qty: BarValue
  max_trade_notional: BarValue
  vwap: BarValue
}

export interface BarFootprintLevel {
  price: BarValue
  buy_qty: BarValue
  sell_qty: BarValue
  buy_notional: BarValue
  sell_notional: BarValue
  buy_count: number
  sell_count: number
  delta: BarValue
  total_qty: BarValue
  total_count: number
  flags: number
}

export interface BarFootprint {
  level_count: number
  poc_price: BarValue
  poc_volume: BarValue
  poc_delta: BarValue
  value_area_high: BarValue
  value_area_low: BarValue
  max_positive_delta: BarValue
  max_negative_delta: BarValue
  imbalance_count: number
  stacked_buy_imbalance: number
  stacked_sell_imbalance: number
  unfinished_auction_high: number
  unfinished_auction_low: number
  levels: BarFootprintLevel[]
}

export interface BarMicrostructure {
  trade_imbalance: BarValue
  volume_imbalance: BarValue
  notional_imbalance: BarValue
  buy_trade_ratio: BarValue
  sell_trade_ratio: BarValue
  trade_intensity: BarValue
  average_trade_size: BarValue
  price_efficiency: BarValue
  absorption_buy: BarValue
  absorption_sell: BarValue
  exhaustion_buy: BarValue
  exhaustion_sell: BarValue
}

/**
 * A market Bar passed to chart series. `time` is in Unix seconds for
 * open-charts; the original timestamp and all enriched market fields remain
 * available on the same object.
 */
export interface Bar {
  time: number
  open: number
  high: number
  low: number
  close: number
  volume: number
  revision: number
  timestamp?: BarValue
  schema_version?: number
  source?: string
  symbol?: string
  timeframe?: string
  bar_key?: string
  header?: Record<string, BarValue>
  identity?: Record<string, BarValue>
  price?: BarPrice
  volume_flow?: BarVolumeFlow
  footprint?: BarFootprint
  price_changes?: Record<string, BarValue>
  trade_size_statistics?: Record<string, BarValue>
  timing?: Record<string, unknown>
  distributions?: Record<string, unknown>
  microstructure?: BarMicrostructure
  integrity?: Record<string, BarValue>
  raw_ticks?: Record<string, BarValue>[]
  [field: string]: unknown
}
