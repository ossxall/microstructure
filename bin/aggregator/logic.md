Tick example

"time","source","symbol","id","price","qty","is_buyer_maker"
"2026-11-23T00:05:30.104000Z","binance","BTCUSDT","8322178460",6117280000000,8100000,1
"2026-11-23T00:05:30.161000Z","binance","BTCUSDT","8322178461",6117290000000,100000,0
"2026-11-23T00:05:30.161000Z","binance","BTCUSDT","8322178462",6117290000000,5200000,0

Toda información del Tick debe usarse en los calculos.
Ninguna información del Tick debe perderse al realizar los calculos. Por ningun motivo sin excepción.

Cada Bar de 1m debe ser barra autocontenida, 1 bar con tiene toda la información. Persistir en QuestDB.

MicroBar1m 
│
├── header
│   ├── schema_version             uint16
│   ├── timestamp                  uint64
│   ├── symbol_id                  uint16
│   ├── source_id                  uint8
│   ├── flags                      uint32
│   ├── aggregation_level          uint8
│   ├── price_scale                uint8
│   ├── quantity_scale             uint8
│   └── time_unit                  uint8
│
├── identity
│   ├── first_trade_id             uint64
│   ├── last_trade_id              uint64
│   ├── n_trades                   uint64
│   ├── first_event_ts             uint64
│   ├── last_event_ts              uint64
│   ├── first_receive_ts           uint64
│   ├── last_receive_ts            uint64
│   ├── coverage_flags             uint32
│   └── sequence_contiguous        uint8
│
├── price
│   ├── open                       int64
│   ├── high                       int64
│   ├── low                        int64
│   ├── close                      int64
│   ├── upticks                    uint64
│   ├── downticks                  uint64
│   ├── zero_ticks                 uint64
│   ├── direction_changes          uint64
│   ├── first_price_change         int64
│   ├── last_price_change          int64
│   ├── max_price_change           int64
│   └── min_price_change           int64
│
├── volume_flow
│   ├── total_qty                  uint128
│   ├── total_notional             uint128
│   ├── buy_qty                    uint128
│   ├── sell_qty                   uint128
│   ├── buy_notional               uint128
│   ├── sell_notional              uint128
│   ├── signed_qty                 int128
│   ├── signed_notional            int128
│   ├── buy_count                  uint64
│   ├── sell_count                 uint64
│   ├── max_trade_qty              uint128
│   ├── max_trade_notional         uint128
│   └── vwap                       uint128
│
├── footprint
│   ├── level_count                uint32
│   ├── poc_price                  int64
│   ├── poc_volume                 uint128
│   ├── poc_delta                  int128
│   ├── value_area_high            int64
│   ├── value_area_low             int64
│   ├── max_positive_delta         int128
│   ├── max_negative_delta         int128
│   ├── imbalance_count            uint32
│   ├── stacked_buy_imbalance      uint32
│   ├── stacked_sell_imbalance     uint32
│   ├── unfinished_auction_high    uint8
│   ├── unfinished_auction_low     uint8
│   │
│   └── levels[LEVEL_COUNT]
│       ├── price                  int64
│       ├── buy_qty                uint128
│       ├── sell_qty               uint128
│       ├── buy_notional           uint128
│       ├── sell_notional          uint128
│       ├── buy_count              uint32
│       ├── sell_count             uint32
│       ├── delta                  int128
│       ├── total_qty              uint128
│       ├── total_count            uint32
│       └── flags                  uint16
│
├── returns
│   ├── return_count               uint64
│   ├── return_first               int64
│   ├── return_last                int64
│   ├── return_sum                 int128
│   ├── return_sum_sq              uint128
│   ├── return_sum_cu              int128
│   ├── return_sum_qu              uint128
│   ├── realized_variance          uint128
│   ├── bipower_variation          uint128
│   ├── positive_variation         uint128
│   ├── negative_variation         uint128
│   ├── max_return                 int64
│   └── min_return                 int64
│
├── trade_size_statistics
│   ├── size_sum                   uint128
│   ├── size_sum_sq                uint128
│   ├── size_sum_cu                uint128
│   ├── size_sum_qu                uint128
│   ├── max_size                   uint128
│   ├── min_size                   uint128
│   └── mean_size                  uint128
│
├── timing
│   ├── first_interarrival         uint64
│   ├── last_interarrival          uint64
│   ├── min_interarrival           uint64
│   ├── max_interarrival           uint64
│   ├── sum_interarrival           uint128
│   ├── sum_interarrival_sq        uint128
│   ├── interarrival_count         uint64
│   ├── active_seconds             uint8
│   └── activity_profile           uint32[60]
│
├── distributions
│   ├── trade_size_hist            uint64[N_SIZE]
│   ├── return_hist                uint64[N_RETURN]
│   ├── interarrival_hist          uint64[N_TIME]
│   └── price_volume_hist          uint64[N_PRICE]
│
├── microstructure
│   ├── trade_imbalance            int128
│   ├── volume_imbalance           int128
│   ├── notional_imbalance         int128
│   ├── buy_trade_ratio            uint64
│   ├── sell_trade_ratio           uint64
│   ├── trade_intensity            uint64
│   ├── average_trade_size         uint128
│   ├── price_efficiency            int64
│   ├── absorption_buy             uint128
│   ├── absorption_sell            uint128
│   ├── exhaustion_buy             uint128
│   └── exhaustion_sell            uint128
│
└── integrity
    ├── sequence_gaps              uint64
    ├── duplicate_count            uint64
    ├── out_of_order_count         uint64
    ├── invalid_trade_count        uint64
    ├── dropped_trade_count        uint64
    ├── expected_trade_count       uint64
    ├── received_trade_count       uint64
    ├── quality_flags              uint32
    └── checksum                   uint64
