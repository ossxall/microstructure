# Ingestor

Connects to a market-data WebSocket, normalizes trades to `Tick`, batches them,
and publishes each symbol's batch to Apache Pulsar.

Configure the source and comma-separated symbols with environment variables:

```sh
SOURCE=binance \
SYMBOLS=BTCUSDT,ADAUSDT \
PULSAR_URL=pulsar://localhost:6650 \
cargo run --manifest-path bin/ingestor/Cargo.toml
```

To use the sample environment file, copy it to `.env.dev`:

```sh
cp bin/ingestor/.env.example bin/ingestor/.env.dev
cargo run --manifest-path bin/ingestor/Cargo.toml
```

`Config` loads `.env.dev` from the ingestor directory automatically. Variables
already set in the process environment take precedence.

`SOURCE` currently supports `binance` and `dydx`. `PULSAR_URL` defaults to
`pulsar://localhost:6650`. Each symbol is published to its own topic:

```text
persistent://public/default/ticks-{source}-{symbol}
```

Messages use MessagePack and contain a batch with the `Tick` fields in this
order: `source`, `symbol`, `id`, `time`, `price`, `qty`, `is_buyer_maker`.
Price and quantity are integer values scaled by `10^8`.
