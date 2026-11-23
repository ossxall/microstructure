CREATE TABLE IF NOT EXISTS bars_1m (
    time TIMESTAMP,
    source SYMBOL,
    symbol SYMBOL,
    revision LONG,
    event_id VARCHAR,
    payload VARCHAR
) TIMESTAMP(time)
PARTITION BY DAY
DEDUP UPSERT KEYS (time, source, symbol);

CREATE TABLE IF NOT EXISTS bar_events (
    time TIMESTAMP,
    event_id VARCHAR,
    source SYMBOL,
    symbol SYMBOL,
    revision LONG,
    payload VARCHAR,
    published BOOLEAN
) TIMESTAMP(time)
PARTITION BY DAY
DEDUP UPSERT KEYS (time, event_id);

CREATE TABLE IF NOT EXISTS aggregator_state (
    time TIMESTAMP,
    state_key SYMBOL,
    cursor_ms LONG
) TIMESTAMP(time)
PARTITION BY DAY
DEDUP UPSERT KEYS (time, state_key);
