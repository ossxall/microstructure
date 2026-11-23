// BLACKER - microstructure
// Copyright (C) 2026 Juan José Caballero Rey
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation version 3 of the License.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use crate::model::Tick;
use sha2::{Digest, Sha256};

/// Build a bounded, collision-resistant Redis key from the same identity used
/// by QuestDB's UPSERT key.
pub fn redis_key(prefix: &str, tick: &Tick) -> String {
    let mut digest = Sha256::new();
    for part in [
        tick.source.as_bytes(),
        tick.symbol.as_bytes(),
        tick.id.as_bytes(),
    ] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    digest.update(tick.time.to_be_bytes());

    let hash = digest.finalize();
    let mut key = String::with_capacity(prefix.len() + 1 + hash.len() * 2);
    key.push_str(prefix);
    key.push(':');
    for byte in hash {
        use std::fmt::Write;
        write!(key, "{byte:02x}").expect("writing to a String cannot fail");
    }
    key
}

#[cfg(test)]
mod tests {
    use super::redis_key;
    use crate::model::Tick;

    fn tick(price: u64) -> Tick {
        Tick {
            source: "binance".to_string(),
            symbol: "BTCUSDT".to_string(),
            id: "1:2".to_string(),
            time: 100,
            price,
            qty: 5,
            is_buyer_maker: 0,
        }
    }

    #[test]
    fn dedup_key_uses_trade_identity_not_trade_values() {
        assert_eq!(
            redis_key("merger:tick:v1", &tick(10)),
            redis_key("merger:tick:v1", &tick(20))
        );
        assert_ne!(
            redis_key("merger:tick:v1", &tick(10)),
            redis_key(
                "merger:tick:v1",
                &Tick {
                    id: "1".to_string(),
                    ..tick(10)
                }
            )
        );
    }
}
