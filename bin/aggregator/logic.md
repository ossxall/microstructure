# **Diseño de un Tick-to-Bar Aggregator de microestructura (1 minuto)**

El objetivo es construir un agregador que transforme los *trades individuales* de Binance en barras de 1 minuto que conserven suficiente información para calcular prácticamente cualquier indicador de microestructura del mercado, sin almacenar todos los ticks dentro de cada barra.

La idea central es utilizar **estadísticas suficientes, acumuladores incrementales, histogramas cuantizados y escalado de precisión**.

No queremos una OHLCV convencional. Queremos una barra que funcione como un pequeño resumen estadístico del comportamiento del mercado durante esos 60 segundos.

## **1. Arquitectura**

**INPUT — Raw Trades**

`timestamp, price, qty, id, is_buyer_maker`

**Normalizer**

Unidades, timestamps, dirección agresora, validación y orden

**Incremental Feature Engine**

- Price moments
- Volume moments
- Order flow
- Trade intensity
- Inter-arrival times
- Price-volume relationship
- Intrabar path
- Distribution sketches

**Scaled Bar Encoder**

Fixed-point + integer accumulators + quantized distributions

**1m Microstructure Bar**

Un único registro autocontenido por minuto

La arquitectura debe ser independiente del indicador: el agregador conserva información y los indicadores se calculan posteriormente sobre las barras.

## **2. Información que debe contener cada BAR**

Propongo dividir cada barra en 9 bloques.

### **Bloque A — Identidad y temporalidad**

| Campo            | Descripción                                |
| ---------------- | ------------------------------------------ |
| `ts`             | Inicio del minuto                          |
| `symbol_id`      | Identificador del instrumento              |
| `source_id`      | Exchange                                   |
| `n_trades`       | Número de operaciones                      |
| `first_ts_delta` | Tiempo del primer trade respecto al inicio |
| `last_ts_delta`  | Tiempo del último trade                    |
| `first_trade_id` | Primer ID                                  |
| `last_trade_id`  | Último ID                                  |
| `sequence_gaps`  | Cantidad de discontinuidades de IDs        |
| `late_trades`    | Trades recibidos fuera de orden            |

Los IDs pueden almacenarse como enteros de 64 bits, sin convertirlos en strings.

### **Bloque B — Price action**

| Campo               | Descripción                                        |
| ------------------- | -------------------------------------------------- |
| `open`              | Primer precio                                      |
| `high`              | Precio máximo                                      |
| `low`               | Precio mínimo                                      |
| `close`             | Último precio                                      |
| `vwap`              | Precio medio ponderado                             |
| `twap`              | Precio medio temporal aproximado, si se implementa |
| `mid_range`         | `(high + low) / 2`                                 |
| `price_change`      | `close - open`                                     |
| `return`            | Retorno logarítmico                                |
| `max_up_move`       | Máximo movimiento positivo intrabar                |
| `max_down_move`     | Máximo movimiento negativo intrabar                |
| `realized_variance` | Suma de retornos al cuadrado                       |
| `realized_bipower`  | Variación bipower                                  |
| `price_skewness`    | Asimetría de retornos                              |
| `price_kurtosis`    | Curtosis de retornos                               |

**Importante:** para realized variance no debemos calcular solamente el retorno entre apertura y cierre. Debemos acumular los retornos entre trades consecutivos:

\\[ RV=\sum\_{i=2}^{N}r_i^2 \\]

Donde:

\\[ r_i=\ln(P_i/P\_{i-1}) \\]

Esto permite conservar una estimación de la volatilidad realizada a nivel de transacción.

### **Bloque C — Volume y trade size**

| Campo                  | Descripción                      |
| ---------------------- | -------------------------------- |
| `volume`               | Volumen total                    |
| `quote_volume`         | Volumen en moneda cotizada       |
| `buy_volume`           | Volumen de compradores agresores |
| `sell_volume`          | Volumen de vendedores agresores  |
| `buy_quote_volume`     | Notional de compras              |
| `sell_quote_volume`    | Notional de ventas               |
| `max_trade_qty`        | Mayor trade                      |
| `min_trade_qty`        | Menor trade                      |
| `sum_qty_squared`      | Suma de cantidades al cuadrado   |
| `sum_notional_squared` | Suma de notionals al cuadrado    |
| `trade_size_skewness`  | Asimetría del tamaño             |
| `trade_size_kurtosis`  | Curtosis del tamaño              |

### **Bloque D — Order Flow**

Este bloque es fundamental.

Definimos:

\\[ s_i = \begin{cases} +1 & \text{buyer maker = false}\\\ -1 & \text{buyer maker = true} \end{cases} \\]

Es decir, `is_buyer_maker=false` representa una compra agresora y `true` una venta agresora.

Acumulamos:

\\[ I_V=V\_{buy}-V\_{sell} \\]

\\[ I_N=N\_{buy}-N\_{sell} \\]

Donde \\(I_V\\) es el imbalance de volumen e \\(I_N\\) el imbalance de notional.

Campos:

- `signed_volume`
- `signed_notional`
- `signed_trade_count`
- `buy_trade_count`
- `sell_trade_count`
- `max_buy_trade`
- `max_sell_trade`
- `buy_sell_ratio`
- `volume_imbalance_ratio`
- `notional_imbalance_ratio`
- `cumulative_delta`
- `delta_max`
- `delta_min`
- `delta_close`

Con esto podemos construir CVD, delta, agresión relativa y presión compradora/vendedora.

### **Bloque E — Trade intensity y timing**

Necesitamos información sobre la actividad temporal.

- `mean_interarrival_us`
- `min_interarrival_us`
- `max_interarrival_us`
- `sum_interarrival_squared`
- `interarrival_histogram`
- `trades_per_second`
- `volume_per_second`
- `buy_trades_per_second`
- `sell_trades_per_second`
- `active_seconds`
- `max_trades_1s`
- `max_volume_1s`

También recomiendo 12 subintervalos de 5 segundos, con estos datos por intervalo:

- Número de trades.
- Volumen.
- Notional.
- Signed volume.
- Buy count.
- Sell count.
- Open y close.

Esto permite observar cómo evoluciona la actividad dentro del minuto sin conservar los ticks.

### **Bloque F — Dependencia precio-volumen**

Para medir el impacto del flujo sobre el precio:

- `sum_signed_qty`
- `sum_signed_qty_squared`
- `sum_price_signed_qty`
- `sum_abs_return`
- `sum_return_signed_volume`
- `sum_return_squared`
- `sum_abs_return_squared_volume`
- `sum_price_qty`
- `sum_price_squared_qty`

Con estos acumuladores podemos calcular aproximaciones de:

- Kyle's Lambda.
- Amihud Illiquidity.
- Price Impact.
- Volume-Price Correlation.
- Signed Flow-Return Correlation.
- Trade Direction Persistence.

### **Bloque G — Intrabar path**

Una barra OHLC pierde demasiada información sobre el camino del precio.

Por eso recomiendo conservar:

- Número de cambios de precio.
- Número de upticks.
- Número de downticks.
- Número de zero ticks.
- Máximo movimiento consecutivo positivo.
- Máximo movimiento consecutivo negativo.
- Máxima excursión desde el open.
- Máxima reversión.
- Número de cambios de dirección.
- Suma de movimientos absolutos.

Esto mejora significativamente los análisis de microestructura y permite aproximar métricas de eficiencia del movimiento.

### **Bloque H — Distribuciones cuantizadas**

Este bloque es la clave para reducir almacenamiento sin perder demasiada información.

En vez de guardar cada operación, guardamos histogramas compactos:

1. Distribución de retornos.
2. Distribución de tamaños de trade.
3. Distribución de inter-arrival times.
4. Distribución de signed volume.
5. Distribución de trades por segundo.

Cada histograma tendrá bins definidos mediante escalas logarítmicas o cuantización fija.

Por ejemplo:

| Bin | Significado                  |
| --- | ---------------------------- |
| 0   | Trade extremadamente pequeño |
| 1   | Trade pequeño                |
| 2   | Trade mediano                |
| 3   | Trade grande                 |
| 4   | Trade muy grande             |
| 5   | Trade extremo                |

No utilizaría únicamente percentiles calculados al final: los histogramas conservan mejor información sobre la distribución completa.

### **Bloque I — Calidad y estado**

- `is_complete`
- `is_partial`
- `has_missing_sequence`
- `has_duplicate_trades`
- `has_out_of_order_trades`
- `data_quality_score`
- `schema_version`
- `scale_version`

## **3. Agregación escalada: reducir el peso de cada BAR**

Aquí está la parte más importante del diseño.

No recomiendo almacenar todos los valores como `float64`.

Utilizaría tres técnicas combinadas:

1. Fixed-point arithmetic.
2. Integer accumulators de 64 bits.
3. Histogramas compactos de 16 bits.

### **Escalas recomendadas**

Tomando como referencia tus datos:

| Variable    | Representación                        |
| ----------- | ------------------------------------- |
| Precio      | Entero en ticks del exchange          |
| Cantidad    | Entero en lotes mínimos               |
| Notional    | Entero escalado                       |
| Timestamp   | Microsegundos desde inicio del minuto |
| Retorno     | Fixed-point × 10⁹                     |
| RV          | Fixed-point × 10¹²                    |
| Ratio       | Fixed-point × 10⁶                     |
| Histogramas | uint16                                |
| Contadores  | uint32                                |

Una consideración importante: la escala del precio y de la cantidad debe venir de los filtros `tickSize` y `stepSize` del exchange. No debemos asumir que todos los símbolos tienen la misma precisión.

### **Agregación jerárquica**

En lugar de acumular directamente todo en un único vector de estadísticas, propongo:

**Nivel 1 — Tick accumulator**

Cada trade actualiza los acumuladores incrementales.

**Nivel 2 — 5-second accumulator**

Cada 5 segundos se genera un micro-resumen de actividad.

**Nivel 3 — 1-minute aggregator**

Combina los 12 micro-resúmenes y las estadísticas globales.

**Nivel 4 — Scaled encoder**

Convierte las estadísticas a enteros, cuantiza distribuciones y serializa.

Esto permite que el sistema tenga un coste de memoria constante independientemente de la cantidad de ticks recibidos.

**Una precaución:** las estadísticas no lineales, como varianza, covarianza, skewness y kurtosis, no se pueden combinar sumando simplemente los resultados de cada subintervalo. Hay que utilizar acumuladores combinables con sus respectivos momentos, o actualizar los acumuladores globales directamente con cada tick.

## **4. Esquema final de la BAR**

Recomiendo separar el esquema lógico del formato físico. El lógico sirve para desarrollar indicadores; el físico minimiza almacenamiento.

Esta sería la estructura lógica:

```
MicroBar1m = {    # Price-volume interaction    "sum_return_signed_volume": 0,    "sum_return_abs_volume": 0,    "sum_price_signed_volume": 0,    # Trade timing    "first_ts_delta": 0,    "last_ts_delta": 0,    "sum_interarrival": 0,    "sum_interarrival_sq": 0,    "max_interarrival": 0,    # Path    "upticks": 0,    "downticks": 0,    "zero_ticks": 0,    "direction_changes": 0,    "sum_abs_price_change": 0,    # Histograms    "return_hist": [],    "trade_size_hist": [],    "interarrival_hist": [],    "signed_volume_hist": [],    # 5-second profiles (12 slots)    "subbars": [],    # Integrity    "first_trade_id": 0,    "last_trade_id": 0,    "sequence_gaps": 0,    "quality_flags": 0,    "schema_version": 1}
```

Es un esquema lógico ilustrativo: eliminaría campos duplicados en la implementación física y utilizaría una codificación binaria.

Por ejemplo, `volume`, `sum_qty` y `sum_qty_sq` pueden ser el mismo acumulador o tener nombres distintos según la unidad utilizada. No hay que almacenar duplicados innecesarios.

## **5. Indicadores que podremos calcular**

La intención es que el BAR sea independiente de los indicadores.

| Familia         | Indicadores                                          |
| --------------- | ---------------------------------------------------- |
| Volatilidad     | RV, BV, realized volatility, skewness, kurtosis      |
| Order flow      | CVD, delta, volume imbalance                         |
| Liquidez        | Amihud, price impact, Kyle lambda aproximada         |
| Trade flow      | Trade intensity, trade frequency                     |
| Agresión        | Buy/sell pressure, aggressor ratio                   |
| Microestructura | Tick rule, zero-return ratio, reversal rate          |
| Distribución    | Trade size distribution, return distribution         |
| Temporalidad    | Interarrival volatility, clustering                  |
| Dependencia     | Return-flow correlation, trade direction persistence |
| Precio          | VWAP, price efficiency, path length                  |
| Actividad       | Volume burst, trade burst, activity imbalance        |

**Hay una limitación importante:** no existe una compresión de barras de 1 minuto que permita reconstruir exactamente cualquier indicador imaginable a nivel de tick. Por ejemplo, autocorrelaciones a retardos arbitrarios, secuencias exactas de trades y ciertos estimadores de distribución necesitan información adicional. Asimismo, OFI (Order Flow Imbalance) real necesita snapshots o eventos del libro de órdenes L2; los trades por sí solos no bastan.

Mi propuesta conserva suficientes estadísticas para una gama muy amplia de indicadores y permite extender el esquema cuando aparezcan nuevas necesidades.

## **6. Cómo procesar tus datos de Binance**

Tus tres registros:

| Trade      | Precio  | Qty   | Agresor |
| ---------- | ------- | ----- | ------- |
| 8322178460 | 61172.8 | 0.081 | Sell    |
| 8322178461 | 61172.9 | 0.001 | Buy     |
| 8322178462 | 61172.9 | 0.052 | Buy     |

Asumiendo que precio y cantidad están escalados por \\(10^8\\):

- Precio real = precio / \\(10^8\\).
- Cantidad real = qty / \\(10^8\\).

El resultado parcial de esa barra sería:

**BTCUSDT · 1m**

3 trades

OPEN

# **61,172.8**

CLOSE

# **61,172.9**

HIGH / LOW

**61,172.9 / 61,172.8**

VWAP

**61,172.8321**

TOTAL VOLUME

**0.134 BTC**

BUY / SELL

**0.053 / 0.081 BTC**

**Volume Delta**

## **+0.026 BTC**

**Buy/Sell Ratio**\
**0.6543**

**Signed Trade Count**\
**+1**

Observa que el delta es positivo aunque el primer trade sea una venta agresora.

## **7. Algoritmo incremental**

La parte fundamental es que no necesitamos mantener los ticks en memoria.

Pseudocódigo:

```
def on_trade(bar, trade):    price = trade.price    qty = trade.qty    # Aggressor side    side = -1 if trade.is_buyer_maker else 1    notional = price * qty    signed_qty = side * qty    # OHLC    if bar.n_trades == 0:        bar.open = price    bar.high = max(bar.high, price)    bar.low = min(bar.low, price)    bar.close = price    # Volume    bar.volume += qty    bar.quote_volume += notional    # Aggressor flow    if side == 1:        bar.buy_volume += qty        bar.buy_count += 1    else:        bar.sell_volume += qty        bar.sell_count += 1    bar.signed_volume += signed_qty    bar.signed_notional += side * notional    # VWAP accumulator    bar.sum_price_qty += notional
```

En producción, sustituiría las operaciones con `float` por enteros escalados y acumuladores de precisión suficiente. También implementaría el cálculo de momentos centrales y los retornos usando una rutina numéricamente estable.

## **8. Formato físico y peso de almacenamiento**

Mi recomendación es utilizar un esquema binario columnar o una estructura binaria empaquetada, no JSON.

| Componente             | Tipo            | Peso aproximado |
| ---------------------- | --------------- | --------------- |
| Identidad y timestamps | uint64 / uint32 | 32 B            |
| OHLC y VWAP            | int64           | 40 B            |
| Volume y order flow    | int64           | 96 B            |
| Momentos estadísticos  | int64 / int128  | 100–140 B       |
| Timing y path          | uint32 / int64  | 60 B            |
| Histogramas            | uint16          | 128–256 B       |
| Subbars de 5s          | Packed integers | 180–300 B       |
| Calidad y metadata     | uint32          | 16 B            |

OBJETIVO DE DISEÑO

# **500–900 bytes**

por BAR de 1 minuto, como presupuesto inicial de codificación compacta.

Es una estimación de diseño, no un tamaño medido. Depende de los acumuladores, escalas, alineación y número de histogramas.

Para BTCUSDT:

- 1.440 barras diarias.
- 43.200 barras mensuales (30 días).
- 525.600 barras anuales.

Con un objetivo de 700 bytes por barra:

| Periodo | Almacenamiento |
| ------- | -------------- |
| Día     | 0,96 MB        |
| Mes     | 30,24 MB       |
| Año     | 367,9 MB       |

Sin contar índices, compresión del almacenamiento ni metadata compartida.

## **9. Decisiones de ingeniería que considero obligatorias**

Hay cinco aspectos que implementaría desde el principio:

1. **Out-of-order trades:** mantener una ventana de eventos tardíos y recalcular las barras afectadas. No asumir que los eventos llegan siempre ordenados.
2. **Deduplicación:** utilizar `source + symbol + trade_id` como clave única.
3. **Minute rollover:** cerrar la barra solamente cuando haya vencido su watermark de eventos, no simplemente cuando cambie el reloj.
4. **Overflow:** utilizar acumuladores de 128 bits cuando los momentos de orden superior o los notionals puedan exceder 64 bits.
5. **Versionado:** cada BAR debe incluir la versión del esquema y de las escalas.

Además, conservaría la metadata del instrumento: `tickSize`, `stepSize`, precisión y unidad de cantidad. Es necesaria para interpretar correctamente las barras históricas.

## **10. Mi arquitectura definitiva**

## **Microstructure Data Engine**

Arquitectura recomendada

1. **Trade Normalizer**

   Normalización de unidades, IDs, timestamps y dirección agresora.
2. **Streaming Aggregator**

   Acumuladores incrementales sin guardar los ticks.
3. **Microstructure Statistics Engine**

   Momentos, covarianzas, order flow, path, timing y distribuciones.
4. **Multi-resolution Aggregator**

   Resúmenes de 5 segundos y barras de 1 minuto.
5. **Scaled Binary Encoder**

   Fixed-point, enteros compactos e histogramas cuantizados.
6. **BAR Storage**

   Una barra autocontenida, versionada y recuperable.
7. **Indicator Engine**

   Indicadores calculados posteriormente, sin necesidad de consultar los ticks originales.

### **Una última decisión importante**

Yo separaría dos niveles de información:

- **Core statistics:** estadísticas exactas o con precisión numérica controlada. Nunca sacrificaría precisión aquí.
- **Distribution sketches:** histogramas y perfiles cuantizados. Aquí sí aceptaría pérdida controlada.

Así conseguimos una barra pequeña sin degradar innecesariamente los indicadores.

**Mi recomendación final:** diseñar el BAR para que sea un *sufficient-statistics container* de microestructura, no una simple vela OHLCV. El objetivo es que cualquier indicador nuevo pueda consumir la barra sin modificar el proceso de ingestión, salvo cuando ese indicador requiera información que deliberadamente no se ha conservado, como profundidad L2 o secuencias exactas de trades.

El siguiente paso natural sería implementar este diseño en Python o Rust y validar, mediante replay de ticks reales, que las estadísticas de cada BAR coinciden con los cálculos realizados directamente sobre los ticks originales.