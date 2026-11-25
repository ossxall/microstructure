use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use std::io::{Read, Write};

const PREFIX: &str = "gzip+base64:";
const MAX_STORED_PAYLOAD_BYTES: usize = 15 * 1024 * 1024;

/// Compresses payloads before they are stored in QuestDB while leaving the
/// Pulsar/API representation unchanged. Plain text is retained when gzip would
/// make a small payload larger, and remains readable for legacy rows.
pub fn encode(payload: &str) -> Result<String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(payload.as_bytes())
        .context("Failed to gzip QuestDB payload")?;
    let compressed = encoder
        .finish()
        .context("Failed to finish gzipping QuestDB payload")?;
    let encoded = format!("{PREFIX}{}", STANDARD.encode(compressed));
    let stored = if encoded.len() < payload.len() {
        encoded
    } else {
        payload.to_string()
    };

    ensure!(
        stored.len() <= MAX_STORED_PAYLOAD_BYTES,
        "QuestDB payload is {} bytes after compression; the safe per-row limit is {} bytes \
         because QuestDB QWP frames are capped at 16 MiB",
        stored.len(),
        MAX_STORED_PAYLOAD_BYTES
    );
    Ok(stored)
}

/// Decodes compressed payloads and passes through legacy/plain JSON values.
pub fn decode(stored: &str) -> Result<String> {
    let Some(encoded) = stored.strip_prefix(PREFIX) else {
        return Ok(stored.to_string());
    };

    let compressed = STANDARD
        .decode(encoded)
        .context("QuestDB payload has invalid base64 gzip data")?;
    let mut decoder = GzDecoder::new(compressed.as_slice());
    let mut payload = String::new();
    decoder
        .read_to_string(&mut payload)
        .context("Failed to decompress QuestDB payload")?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::{MAX_STORED_PAYLOAD_BYTES, decode, encode};

    #[test]
    fn preserves_legacy_plain_payloads() {
        let payload = r#"{"event_id":"bar-1","data":{"raw_ticks":[]}}"#;
        assert_eq!(decode(payload).unwrap(), payload);
        assert_eq!(encode(payload).unwrap(), payload);
    }

    #[test]
    fn compresses_large_json_below_the_qwp_row_limit_and_round_trips() {
        let tick = r#"{"time":"1764000000000","time_us":"1764000000123","source":"binance","symbol":"BTCUSDT","id":"123456789","price":"912345678901","qty":"12345678","is_buyer_maker":0},"#;
        let payload = format!("[{}]", tick.repeat(250_000));

        let stored = encode(&payload).unwrap();

        assert!(stored.starts_with("gzip+base64:"));
        assert!(stored.len() < MAX_STORED_PAYLOAD_BYTES);
        assert_eq!(decode(&stored).unwrap(), payload);
    }

    #[test]
    fn rejects_payloads_that_remain_over_the_safe_qwp_limit() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut bytes = Vec::with_capacity(MAX_STORED_PAYLOAD_BYTES + 1);
        for _ in 0..=MAX_STORED_PAYLOAD_BYTES {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            bytes.push(b'!' + (state % 94) as u8);
        }
        let payload = String::from_utf8(bytes).unwrap();
        assert!(encode(&payload).is_err());
    }
}
