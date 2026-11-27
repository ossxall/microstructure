CREATE TABLE IF NOT EXISTS {event_table} (
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

CREATE TABLE IF NOT EXISTS {state_table} (
    time TIMESTAMP,
    state_key SYMBOL,
    cursor_ms LONG
) TIMESTAMP(time)
PARTITION BY DAY
DEDUP UPSERT KEYS (time, state_key);
