CREATE TABLE IF NOT EXISTS ticks (
    time TIMESTAMP,
    source SYMBOL,
    symbol SYMBOL,
    id VARCHAR,
    price LONG,
    qty LONG,
    is_buyer_maker SHORT
) TIMESTAMP(time)
PARTITION BY DAY
DEDUP UPSERT KEYS (time, source, symbol, id);
