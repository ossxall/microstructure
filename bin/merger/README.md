# Merger

Consumes the MessagePack tick batches produced by `bin/ingestor`, deduplicates
them with Redis, and persists them in QuestDB.

## Run locally

Start Redis, Pulsar, and QuestDB:

```sh
docker compose -f infra.yaml up -d
```

Then run the consumer from the repository root:

```sh
cp bin/merger/.env.example bin/merger/.env.dev
cargo run --manifest-path bin/merger/Cargo.toml
```

The service reads `.env.dev` from this directory when it exists. Process
environment variables take precedence.

## Topics and message format

The consumer uses a durable `Shared` subscription on the `public/default`
namespace and discovers every topic matching `ticks-*`. It starts at the
earliest message position, so a newly created subscription can drain ticks
published before the merger started. `PULSAR_SUBSCRIPTION` must remain stable
between restarts to keep the broker cursor. The namespace is rescanned every
30 seconds so topics created while the merger is running are added too.

Messages use the same MessagePack envelope as the ingestor:

```text
{ ticks: [{ source, symbol, id, time, price, qty, is_buyer_maker }] }
```

`time` is epoch milliseconds; `price` and `qty` are integer values scaled by
`10^8`. They are stored unchanged as QuestDB `LONG` columns. An out-of-range
value is rejected instead of being rounded or truncated.

## Delivery and deduplication

For each batch the merger:

1. Checks tick identities in Redis.
2. Writes new ticks to QuestDB and waits for a local-durable acknowledgement.
3. Marks persisted identities in Redis with `REDIS_TTL_SECONDS`.
4. Acknowledges the Pulsar message.

On an error, the Pulsar message is negatively acknowledged and retried; after
`PULSAR_MAX_REDELIVER_COUNT` attempts it is routed to
`PULSAR_DEAD_LETTER_TOPIC`. The Redis keys are a bounded fast-path cache, not
the correctness boundary: the QuestDB table also has `DEDUP UPSERT KEYS
(time, source, symbol, id)`, covering crashes between database writes, Redis
updates, and Pulsar acknowledgements.

QuestDB's client-side store-and-forward queue is enabled with periodic disk
sync. Keep `QUESTDB_SF_DIR` on persistent storage. The QuestDB service in
`infra.yaml` enables `cairo.commit.mode=adaptive`, which is required for
`request_durable_ack=local`. QuestDB data itself is persisted in the
`questdb-data` Docker volume.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `PULSAR_URL` | `pulsar://localhost:6650` | Pulsar broker address |
| `PULSAR_SUBSCRIPTION` | `tick-persistence` | Durable consumer subscription |
| `PULSAR_MAX_REDELIVER_COUNT` | `10` | Attempts before routing to the DLQ |
| `PULSAR_DEAD_LETTER_TOPIC` | `persistent://public/default/merger-dead-letter` | Poison-message topic |
| `REDIS_URL` | `redis://localhost:6379/0` | Redis connection |
| `REDIS_KEY_PREFIX` | `merger:tick:v1` | Prefix for hashed trade identities |
| `REDIS_TTL_SECONDS` | `2592000` | Redis dedup-cache lifetime |
| `QUESTDB_CONNECT` | `ws::addr=localhost:9000` | QuestDB QWP connection string; may include authentication/TLS options |
| `QUESTDB_SF_DIR` | `./data/questdb-sf` | Persistent client store-and-forward directory, relative to `bin/merger` |

The merger creates the `ticks` table at startup using
[`infra/sql/questdb_ticks.sql`](../../infra/sql/questdb_ticks.sql). The schema
uses epoch-millisecond timestamps, scaled integer price/quantity, and the
original `is_buyer_maker` byte value.

The service manages QuestDB's `sf_dir`, `sf_durability`,
`request_durable_ack`, and sender-pool options itself; do not set those in
`QUESTDB_CONNECT`.
