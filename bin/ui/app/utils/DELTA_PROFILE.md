# Delta Profile — Rolling Window

`deltaProfileSeries.ts` provides the `RollingDeltaProfile` accumulator and the
open-charts foreground series. The chart candles continue to use the selected
timeframe; the profile has its own 1m history request and WebSocket subscription
so changing the candle timeframe never changes its source data.

## Data and precision

- Only `footprint.levels[]` contributes to the profile. `trades[]` and
  `raw_ticks[]` are not traversed.
- Price and quantity values are read as scaled integers and accumulated as
  `BigInt`. Bars with incompatible sources, symbols, or scales are rejected
  with a diagnostic rather than silently combined.
- The profile checks per-level `buy_qty + sell_qty = total_qty` and
  `buy_qty - sell_qty = delta`. `volume_flow` is a consistency check; if it
  disagrees, the footprint remains authoritative and a diagnostic is shown.
- Revisions replace previous `bar_key` contributions. Duplicate events and
  older revisions are ignored. Changed live snapshots may replace one another
  at the same revision; a finalized bar replaces a live snapshot at the same
  revision, and delayed live events cannot replace a finalized bar. The window
  is bounded by 1m timestamps and supports out-of-order updates.
- A bar with the same timestamp as another key does not contribute twice; the
  highest revision wins, with the key as a deterministic tie-break.
- POC ties choose the lower price. Value Area expands from the POC toward the
  adjacent available footprint level with more volume; equal volumes expand
  upward first, matching the bar aggregator.
- When a bar marks unknown-side trades in `identity.coverage_flags`, total
  `volume_flow` and trade counts may exceed the known-side footprint totals.
  Those fields are checked as lower bounds rather than exact matches.

## Lifecycle and limitations

The default excludes live/in-progress Bars; enabling **Incluir Bar en
formación** lets its newer revisions replace the active contribution. History
records without an `is_final` marker are treated as closed, matching the server
contract. A bar without footprint levels contributes no levels even if its
`volume_flow` is nonzero; this is surfaced as a diagnostic.

The chart's existing server endpoint only returns persisted Bars for a requested
timeframe. The profile therefore requests `/get-history` with `timeframe: 1m`
and listens on a second WebSocket subscription. The server currently forwards
the 1m event stream as provided; if a source does not publish enriched 1m Bars,
the profile reports the resulting lack of valid levels rather than deriving
delta from OHLCV.

Tick grouping is optional and uses a display-price tick size converted to the
Bar's `price_scale`; it changes only the price bucket, never the source values.
The overlay is drawn in the main pane's foreground layer and limited to the
visible chart price range.
