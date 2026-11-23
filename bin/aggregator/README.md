# Aggregator

Builds self-contained 1-minute bars from the `ticks` table in QuestDB, stores
each bar back in QuestDB, and publishes a JSON event to Pulsar.

## Run

Start QuestDB and Pulsar, then configure and run the service:

```sh
cp bin/aggregator/.env.example bin/aggregator/.env.dev
cargo run --manifest-path bin/aggregator/Cargo.toml
```

The process reads `.env.dev` from this directory; environment variables take
precedence. The `ticks` table must be populated by the merger. The aggregator
creates `bars_1m`, `bar_events` (the event outbox), and `aggregator_state`.

## Processing and delivery

- Minute windows use UTC epoch-minute boundaries and the half-open range
  `[start, end)`. A bar timestamp is the beginning of its minute.
- A bar is created for each `(source, symbol)` with at least one tick. Empty
  minutes do not produce bars.
- The persisted JSON contains the schema from `logic.md` and the ordered
  `raw_ticks`, retaining every field available in the QuestDB tick row plus
  `time_us` so consumers can independently verify the checksum.
- Pulsar events wrap the bar in `data`; `event_id` is an envelope field, not a
  `Bar1m` field. Consumers should deduplicate by that stable ID.
- The bar and a pending event-outbox row are written to QuestDB before the
  event is sent, using separate durable QWP flushes so large bar payloads do not
  exceed QuestDB's per-frame limit. If the process stops between those writes,
  rescanning the bucket repairs the missing outbox row. The outbox is retried
  after failures/restarts. Event delivery is **at least once**; consumers should
  deduplicate using `event_id`, which is stable for a bar revision.
- Bars use `(time, source, symbol)` as the QuestDB UPSERT key. If a tick arrives
  late within `REPROCESS_MINUTES`, the aggregator recomputes the affected bar,
  increments its revision, upserts it, and emits a new event.
- `aggregator_state` stores the next minute to process, so empty minutes and
  catch-up after downtime do not lose the processing cursor.
- Run a single active aggregator instance per QuestDB database: the cursor and
  source/symbol ID registry are designed for one writer.

QuestDB and Pulsar do not share a transaction. The outbox closes the crash gap
between bar persistence and publishing, but a crash after Pulsar accepts an
event and before QuestDB records it as published can cause a duplicate event.
Use the deterministic `event_id` for consumer-side idempotency.

## Metric conventions and source limitations

`price` and `qty` remain integers scaled by `10^8`; notional is their integer
product. `is_buyer_maker=0` contributes to buy flow and `=1` to sell flow;
other values remain in `raw_ticks` and contribute to total flow, but are
excluded from side-specific totals. Price changes and their moments/histogram
are integer price changes, not percentage returns. Inter-arrival times are
microseconds.

The footprint uses observed price levels, a 70% volume value area, and a 3:1
buy/sell imbalance threshold. The value-area target uses known-side quantity;
unknown-side ticks remain in total flow but do not affect the footprint volume
target. The histogram fields have 16 equal-width bins;
each histogram also includes its minimum and maximum so its bins can be
interpreted. See `logic.md` for formulas, flag assignments, and the exact
checksum byte encoding.

In schema version 3, every signed or unsigned 64-bit and 128-bit integer is
serialized as an exact base-10 JSON string (including histogram arrays). This
avoids precision loss in JavaScript and other binary64-based consumers.

Some integrity inputs are not present in the current `ticks` schema: receive
timestamps, original arrival order, pre-ingestion duplicate counts, expected
sequence counts, and rejected/dropped tick counts. Their corresponding
unavailable fields are zero, and the `coverage_flags`/`quality_flags` bits
identify the missing coverage. `duplicate_count` counts repeated IDs present
in the bar (numeric IDs by value, non-numeric IDs by exact text), while
`out_of_order_count` counts numeric ID regressions in event-time order; neither
recovers original arrival order. Numeric trade IDs are used for ordering and
gap checks; non-numeric IDs are preserved verbatim in both the identity section
and `raw_ticks`.

Serialized Pulsar payload sizes depend on the number of raw ticks retained in a
bar. Set the aggregator log filter to `debug` to record each encoded event's
byte size, and compare high-activity bars with the deployed broker's configured
message limit; this repository does not assume a fixed broker limit.

Because the source table has no receive timestamp, `REPROCESS_MINUTES` is the
late-tick correction horizon, not a proof that arbitrarily late arrivals have
been observed. Ticks remain in QuestDB; increase this window or rebuild affected
bars from the raw table if data can arrive later than the configured horizon.

The source/symbol numeric IDs are allocated in `AGGREGATOR_ID_REGISTRY`.
Keep this file on persistent storage and share it only with a single active
aggregator instance to preserve stable IDs.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `PULSAR_URL` | `pulsar://localhost:6650` | Pulsar broker |
| `PULSAR_BARS_TOPIC` | `persistent://public/default/bars-1m` | Bar event topic |
| `QUESTDB_CONNECT` | `ws::addr=localhost:9000` | QuestDB QWP connection |
| `QUESTDB_SF_DIR` | `./data/questdb-sf` | Persistent QuestDB client store-and-forward directory |
| `POLL_INTERVAL_SECONDS` | `60` | Aggregator polling cadence |
| `ALLOWED_LATENESS_SECONDS` | `5` | Delay after minute close before first calculation |
| `REPROCESS_MINUTES` | `5` | Recent minutes rescanned for late ticks |
| `AGGREGATOR_ID_REGISTRY` | `./data/ids.json` | Persistent source/symbol ID map |
