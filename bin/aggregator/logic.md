# Current `Bar1m` Structure

This document describes the structure currently serialized by `Bar1m` in
`src/model.rs` (schema version 2). Bars are stored as JSON in QuestDB and
included in events published to Pulsar.

## Input Tick

The `ticks` table provides these fields:

```text
"time","source","symbol","id","price","qty","is_buyer_maker"
"2026-11-23T00:05:30.104000Z","binance","BTCUSDT","8322178460",6117280000000,8100000,1
"2026-11-23T00:05:30.161000Z","binance","BTCUSDT","8322178461",6117290000000,100000,0
```

Ticks are sorted by time and, when timestamps match, by numeric ID if both IDs
are numeric; otherwise, IDs are compared as text. Each bar corresponds to a UTC
minute using the half-open interval `[start, end)`. A bar is created only when
the interval contains at least one tick.

## JSON Schema

The listed types are the Rust types used by the model. `String` is serialized
as a JSON string and vectors as arrays. High-order moments listed as `String`
are emitted as exact decimal strings.

```text
Bar1m
├── schema_version              u16
├── timestamp                   u64
├── source                      String
├── symbol                      String
├── revision                    u64
├── bar_key                     String
├── header
│   ├── schema_version          u16
│   ├── timestamp               u64
│   ├── symbol_id               u16
│   ├── source_id               u8
│   ├── flags                   u32
│   ├── aggregation_level       u8
│   ├── price_scale             u8
│   ├── quantity_scale          u8
│   └── time_unit               u8
├── identity
│   ├── first_trade_id           u64
│   ├── last_trade_id            u64
│   ├── first_trade_id_raw       String
│   ├── last_trade_id_raw        String
│   ├── trade_ids_numeric        u8
│   ├── n_trades                 u64
│   ├── first_event_ts           u64
│   ├── last_event_ts            u64
│   ├── first_receive_ts         u64
│   ├── last_receive_ts          u64
│   ├── coverage_flags           u32
│   └── sequence_contiguous      u8
├── price
│   ├── open                     i64
│   ├── high                     i64
│   ├── low                      i64
│   ├── close                    i64
│   ├── upticks                  u64
│   ├── downticks                u64
│   ├── zero_ticks               u64
│   ├── direction_changes        u64
│   ├── first_price_change       i64
│   ├── last_price_change        i64
│   ├── max_price_change         i64
│   └── min_price_change         i64
├── volume_flow
│   ├── total_qty                u128
│   ├── total_notional           u128
│   ├── buy_qty                  u128
│   ├── sell_qty                 u128
│   ├── buy_notional             u128
│   ├── sell_notional            u128
│   ├── signed_qty               i128
│   ├── signed_notional          i128
│   ├── buy_count                u64
│   ├── sell_count               u64
│   ├── max_trade_qty            u128
│   ├── max_trade_notional       u128
│   └── vwap                     u128
├── footprint
│   ├── level_count              u32
│   ├── poc_price                i64
│   ├── poc_volume               u128
│   ├── poc_delta                i128
│   ├── value_area_high          i64
│   ├── value_area_low           i64
│   ├── max_positive_delta       i128
│   ├── max_negative_delta       i128
│   ├── imbalance_count          u32
│   ├── stacked_buy_imbalance    u32
│   ├── stacked_sell_imbalance   u32
│   ├── unfinished_auction_high  u8
│   ├── unfinished_auction_low   u8
│   └── levels[]                 FootprintLevel
│       ├── price                i64
│       ├── buy_qty              u128
│       ├── sell_qty             u128
│       ├── buy_notional         u128
│       ├── sell_notional        u128
│       ├── buy_count            u32
│       ├── sell_count           u32
│       ├── delta                i128
│       ├── total_qty            u128
│       ├── total_count          u32
│       └── flags                u16
├── returns
│   ├── return_count             u64
│   ├── return_first             i64
│   ├── return_last              i64
│   ├── return_sum               i128
│   ├── return_sum_sq            String
│   ├── return_sum_cu            String
│   ├── return_sum_qu            String
│   ├── realized_variance        String
│   ├── bipower_variation        String
│   ├── positive_variation       u128
│   ├── negative_variation       u128
│   ├── max_return               i64
│   └── min_return               i64
├── trade_size_statistics
│   ├── size_sum                 u128
│   ├── size_sum_sq              String
│   ├── size_sum_cu              String
│   ├── size_sum_qu              String
│   ├── max_size                 u128
│   ├── min_size                 u128
│   └── mean_size                u128
├── timing
│   ├── first_interarrival       u64
│   ├── last_interarrival        u64
│   ├── min_interarrival         u64
│   ├── max_interarrival         u64
│   ├── sum_interarrival         u128
│   ├── sum_interarrival_sq      u128
│   ├── interarrival_count       u64
│   ├── active_seconds           u8
│   └── activity_profile         u32[60]
├── distributions
│   ├── trade_size_hist          u64[16]
│   ├── trade_size_hist_min      i128
│   ├── trade_size_hist_max      i128
│   ├── return_hist              u64[16]
│   ├── return_hist_min          i128
│   ├── return_hist_max          i128
│   ├── interarrival_hist        u64[16]
│   ├── interarrival_hist_min    i128
│   ├── interarrival_hist_max    i128
│   ├── price_volume_hist        u64[16]
│   ├── price_volume_hist_min    i128
│   └── price_volume_hist_max    i128
├── microstructure
│   ├── trade_imbalance          i128
│   ├── volume_imbalance         i128
│   ├── notional_imbalance       i128
│   ├── buy_trade_ratio          u64
│   ├── sell_trade_ratio         u64
│   ├── trade_intensity          u64
│   ├── average_trade_size       u128
│   ├── price_efficiency         i64
│   ├── absorption_buy           u128
│   ├── absorption_sell          u128
│   ├── exhaustion_buy           u128
│   └── exhaustion_sell          u128
├── integrity
│   ├── sequence_gaps            u64
│   ├── duplicate_count          u64
│   ├── out_of_order_count       u64
│   ├── invalid_trade_count      u64
│   ├── dropped_trade_count      u64
│   ├── expected_trade_count     u64
│   ├── received_trade_count     u64
│   ├── quality_flags            u32
│   └── checksum                 u64
└── raw_ticks[]                  RawTick
    ├── time                     u64
    ├── source                   String
    ├── symbol                   String
    ├── id                       String
    ├── price                    u64
    ├── qty                      u64
    └── is_buyer_maker           u8
```

## Conventions and Coverage

- `timestamp`, `header.timestamp`, `first_event_ts`, `last_event_ts`, and
  `raw_ticks[].time` are Unix UTC milliseconds. Inter-tick times
  (`interarrival`) are in microseconds.
- `price` and `qty` retain the input integers scaled by `10^8`; `notional` is
  their integer product.
- Price changes and their moments/histograms are integer price changes, not
  percentage returns.
- `is_buyer_maker = 0` contributes to buy flow and `= 1` to sell flow. Other
  values are retained in `raw_ticks` but excluded from side-specific totals.
- The footprint uses observed price levels, a 70% volume value area, and a 3:1
  buy/sell imbalance threshold. Histograms have 16 bins; each includes its
  minimum and maximum so the bins can be interpreted.
- `checksum` is FNV-1a 64-bit over the canonical ordered ticks.
- Moments that can exceed 128 bits are serialized as exact base-10 strings
  (schema version 2).
- The `ticks` table does not contain receive timestamps, original arrival
  order, pre-deduplication duplicate counts, expected tick counts, or rejected
  and dropped tick counts. Numeric fields required for those values are set to
  zero, and `coverage_flags`/`quality_flags` indicate missing coverage.
  `raw_ticks[].time` has millisecond resolution; the checksum is calculated
  using the input timestamp in microseconds.
- Numeric IDs are used to order ticks and check for sequence gaps. Non-numeric
  IDs are preserved as text in `identity` and `raw_ticks`.
- `revision` starts at 1 and increases when a bar changes after being
  recomputed for late ticks within `REPROCESS_MINUTES`. `event_id` is stable
  for each bar revision.
