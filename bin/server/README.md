# Server

`POST /api/market/get-history` serves finalized Bars from the aggregator's
append-only Bar store. It refreshes the read-only index before reading and
returns history in ascending timestamp order. QuestDB remains a fallback for
Bars absent from the local store; local records take precedence when both
sources contain the same timestamp.

By default, the server resolves the store under
`bin/aggregator/data/bars` and opens timeframe-specific directories such as
`bars-5m`. If `AGGREGATOR_BAR_STORE_DIR` is set, configure the same path for
both the aggregator and server. Relative paths are resolved from the
aggregator's directory. `AGGREGATOR_BAR_SEGMENT_BYTES` should also match the
aggregator setting.

Run with:

```sh
cargo run --manifest-path bin/server/Cargo.toml
```
