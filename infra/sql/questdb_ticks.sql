CREATE TABLE IF NOT EXISTS ticks (
    timestamp TIMESTAMP,
    source SYMBOL,
    symbol SYMBOL,
    id VARCHAR,
    price LONG,
    qty LONG,
    is_buyer_maker SHORT
) TIMESTAMP(timestamp)
PARTITION BY DAY
DEDUP UPSERT KEYS (timestamp, source, symbol, id);
