# Current `Bar1m` Structure

This document describes the `Bar1m` JSON contract serialized by `src/model.rs`
(schema version 3). Bars are stored as JSON in QuestDB and included as `data`
in events published to Pulsar.

Version 3 is intentionally not wire-compatible with version 2: wide integers
are decimal strings, the `returns` object/fields are renamed to `price_changes`,
and `raw_ticks[].time_us` is added.

## Input Tick

The `ticks` table provides these fields:

```text
"time","source","symbol","id","price","qty","is_buyer_maker"
"2026-11-23T00:05:30.104000Z","binance","BTCUSDT","8322178460",6117280000000,8100000,1
"2026-11-23T00:05:30.161000Z","binance","BTCUSDT","8322178461",6117290000000,100000,0
```

Ticks are sorted by `time_us`, then by a total trade-ID order: IDs that parse as
`u64` come before text IDs; numeric IDs sort by numeric value, then by their
original text (so `"02"` precedes `"2"`); text IDs sort lexicographically.
Each bar corresponds to a UTC minute using the half-open interval
`[start, end)`. A bar is created only when the interval contains at least one
tick.

## JSON Schema

The listed types are Rust types. In JSON, every `i64`, `u64`, `i128`, and
`u128` is an exact base-10 string, including elements of arrays of `u64`.
`u8`, `u16`, and `u32` remain JSON numbers. Rust `String` values are JSON
strings; vectors are arrays. This is schema version 3 and intentionally avoids
precision loss in JavaScript and other binary64-based consumers.

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
├── price_changes
│   ├── change_count             u64
│   ├── first_change             i64
│   ├── last_change              i64
│   ├── change_sum               i128
│   ├── change_sum_sq            String
│   ├── change_sum_cu            String
│   ├── change_sum_qu            String
│   ├── realized_variance        String
│   ├── bipower_variation        String
│   ├── positive_change_magnitude u128
│   ├── negative_change_magnitude u128
│   ├── max_change               i64
│   └── min_change               i64
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
│   ├── price_change_hist        u64[16]
│   ├── price_change_hist_min    i128
│   ├── price_change_hist_max    i128
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
    ├── time_us                  i64
    ├── source                   String
    ├── symbol                   String
    ├── id                       String
    ├── price                    u64
    ├── qty                      u64
    └── is_buyer_maker           u8
```

## JSON envelope, identity, and versioning

- `event_id` is not a `Bar1m` field. It is in the Pulsar envelope beside
  `event_type`, `timestamp`, `source`, `symbol`, `revision`, and `data`.
  Envelope `timestamp` and `revision` are JSON decimal strings, like the
  corresponding fields in `Bar1m`.
- `bar_key` is `"{source}/{symbol}/{timestamp_ms}"`. `event_id` is
  `"{bar_key}:r{revision}"`. Revision starts at 1 and increments when a
  recomputed bar changes.
- The root `schema_version` and `timestamp` are authoritative. The same values
  are copied into `header`; consumers can validate that the copies match. The
  envelope timestamp/revision likewise mirror `data.timestamp` and
  `data.revision`.
- `timestamp`, `header.timestamp`, `first_event_ts`, `last_event_ts`,
  `raw_ticks[].time`, and the bar/event timestamps are Unix UTC milliseconds.
  `raw_ticks[].time` is the input microsecond timestamp truncated to
  milliseconds. `raw_ticks[].time_us` preserves the exact input timestamp in
  microseconds and is the timestamp used by the checksum. Negative input
  timestamps are rejected. Inter-arrival fields are microseconds.
- `first_trade_id_raw` and `last_trade_id_raw` are the authoritative IDs.
  `first_trade_id`/`last_trade_id` contain the parsed `u64` when possible;
  otherwise they contain FNV-1a 64-bit of the UTF-8 ID bytes. Such hashes can
  collide and must not be treated as the original IDs. `trade_ids_numeric` is 1
  only when every ID parses as `u64`. Rust's `u64` parser accepts a leading
  plus sign (for example, `"+5"` parses as 5); the original text is retained
  and breaks ties between numerically equal IDs.
- `source_id` and `symbol_id` come from separate persistent registries in
  `AGGREGATOR_ID_REGISTRY`, assigned from the first available positive ID.
  Preserve/share that registry to keep the mappings stable. `aggregation_level`
  is 1 for one-minute bars; `price_scale` and `quantity_scale` are decimal
  exponents, both 8; `time_unit=1` means milliseconds.

## Metric and flag conventions

- Input `price` and `qty` are integer values scaled by `10^8`; notional is
  their integer product. All price-change fields are integer price changes,
  not percentage returns. The v3 field names use “change” to make this explicit.
- `buy_trade_ratio` and `sell_trade_ratio` are trade-count fractions multiplied
  by `10^8`, computed over known-side trades; both are zero if there are none.
  `trade_intensity` is `n_trades * 10^8 / active_seconds`, where
  `active_seconds` counts occupied one-second buckets. `price_efficiency` is
  `(close - open) * 10^8 / sum(abs(consecutive price changes))`, or zero when
  the denominator is zero.
- `is_buyer_maker=0` contributes to buy flow and `=1` to sell flow. Other
  values remain in `raw_ticks` and contribute to `total_qty`/`total_notional`,
  but not buy/sell totals. Therefore consumers must not assume
  `total_qty = buy_qty + sell_qty`.
- Footprint `level_count` counts all observed prices. A level's `total_qty` is
  the sum of its known-side buy and sell quantities, so it is zero if that
  price only had ticks with an unknown side. The value-area target is 70% of
  known-side quantity (`buy_qty + sell_qty`); unknown-side quantity does not
  affect the target or level volumes.
- Footprint levels are ordered by ascending observed price. A level is a buy
  imbalance if buy quantity is at least 3 times sell quantity (or sell is zero
  and buy is positive); sell imbalance is symmetric. `stacked_*_imbalance` is
  the longest run of consecutive observed levels with that side's imbalance;
  adjacent observed prices need not be one tick apart. Footprint level flag bit
  0 means buy imbalance and bit 1 means sell imbalance.
- The value area expands outwards from the point of control until it includes
  at least 70% of known-side quantity. Ties in point-of-control volume prefer
  the lower price; expansion ties prefer the higher price. When known-side
  quantity is zero, the target is zero and the value area remains at the POC;
  the footprint should then be interpreted with the unknown-side coverage flag.
- Histograms have 16 equal-width bins. The bin for value `x` is
  `floor((x-min)*16/(max-min))`, capped at bin 15; when `min=max`, all values go
  in bin 0. `*_min` and `*_max` are observed extrema, not bin edges.
  `trade_size_hist` counts trades per trade-quantity bin; it is not weighted by
  quantity. `price_change_hist` counts consecutive integer price changes and
  `interarrival_hist` counts microsecond inter-arrival times. `price_volume_hist`
  uses observed prices as bin values and weights each tick by its raw quantity.
  Thus the first three histograms count observations; the price-volume
  histogram sums quantities per bin.
- `positive_change_magnitude` and `negative_change_magnitude` sum the absolute
  positive and negative price changes respectively. They are not the
  components of `realized_variance`, which is the sum of squared price changes.
  High-order moments use exact decimal strings because they can exceed 128 bits;
  other wide integer values are also strings in v3 for JSON consumer safety.
- `trade_imbalance`, `volume_imbalance`, and `notional_imbalance` are signed
  buy-minus-sell differences for trade count, quantity, and notional. They are
  not absolute values. `vwap` is `total_notional / total_qty`; `mean_size` and
  `average_trade_size` are `size_sum / n_trades`. These integer divisions
  truncate toward zero. VWAP retains the input price scale of `10^8`.
- `direction_changes` counts reversals between consecutive non-zero price
  changes; zero changes do not reset the previous non-zero direction.
  `max_positive_delta` is at least zero and `max_negative_delta` is at most
  zero, because both extrema are initialized to zero.
- `bipower_variation` is the unscaled sum
  `Σ |change[i]| * |change[i-1]|`; it does not include the conventional `π/2`
  estimator factor. `absorption_buy` is total known-side buy quantity when
  `close <= open`, and `absorption_sell` is total known-side sell quantity when
  `close >= open`. `exhaustion_buy` is buy quantity at the highest observed
  level when `close <= open`; `exhaustion_sell` is sell quantity at the lowest
  observed level when `close >= open`. These absorption/exhaustion fields are
  implementation heuristics, not independently validated market signals.
- Header `flags` bit 0 means at least one coverage flag is set. Bit 1 is
  currently always set and reserved; consumers should not assign it another
  meaning. `coverage_flags` bits are: 0 receive time unavailable; 1 at least one
  non-numeric ID; 2 pre-ingestion duplicate count unavailable; 3 original
  arrival order unavailable; 4 unknown side value; 5 expected count
  unavailable; 6 invalid/drop counts unavailable. `quality_flags` currently
  copies `coverage_flags`.
- `unfinished_auction_high` is 1 when the highest-price level has zero sell
  quantity; `unfinished_auction_low` is 1 when the lowest-price level has zero
  buy quantity. These are the implementation's current predicates; confirm
  their interpretation against the intended auction convention before treating
  them as market semantics. These and `sequence_contiguous` are encoded as 0/1.
  `sequence_contiguous` is 1 only when every ID parses as `u64` and the
  event-time-ordered IDs have no duplicate numeric value, reversal, or gap.
- `duplicate_count` counts repeated IDs within this bar after ingestion.
  Numeric IDs are compared by parsed `u64` value (so `"02"` and `"2"` are
  duplicates); non-numeric IDs are compared by exact text. It cannot recover
  duplicates removed before the `ticks` table.
  `out_of_order_count` counts numeric ID regressions in event-time order, not
  original arrival order; the source table does not preserve arrival order.
  `sequence_gaps` counts missing values between adjacent numeric IDs in that
  same ordered sequence. Non-numeric IDs break both regression and gap
  comparisons.

## Checksum and source coverage

- `checksum` is FNV-1a 64-bit over ticks in the exact `raw_ticks` order. Start
  with offset basis `0xcbf29ce484222325`; for each tick append, in order:
  `source`, `symbol`, and `id` as UTF-8 bytes, each preceded by its byte length
  as an unsigned 64-bit big-endian integer; then `time_us` as signed 64-bit
  big-endian, `price` and `qty` as unsigned 64-bit big-endian, and
  `is_buyer_maker` as one byte. For each byte, XOR it into the hash then
  multiply with wrapping arithmetic by `0x100000001b3`. The result is an
  unsigned 64-bit decimal string in JSON. IDs are always encoded as text.
- The `ticks` table does not contain receive timestamps, original arrival
  order, pre-deduplication duplicate counts, expected tick counts, or rejected
  and dropped tick counts. Numeric fields for those unavailable inputs are set
  to zero, and `coverage_flags`/`quality_flags` indicate missing coverage.
