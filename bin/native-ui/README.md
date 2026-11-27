# Native UI

La aplicación se conecta directamente al servidor de mercado:

```bash
cargo run --manifest-path bin/native-ui/Cargo.toml
```

Por defecto utiliza `http://127.0.0.1:3001/api/market`, con:

- `POST /get-history` para cargar las últimas Bars.
- `/ws` para recibir las Bars live.

Se puede cambiar la dirección base con `NATIVE_UI_SERVER_URL`. Para indicar una
dirección WebSocket distinta, se puede definir `NATIVE_UI_WEBSOCKET_URL`.

Cada tab abre un feed aislado y mantiene su propio layout de Charts; añadir o
cerrar un Chart actualiza las necesidades de cálculo de ese feed. El código de
cada Chart vive en su propio módulo bajo `src/charts/`, mientras que cada pestaña
conserva su feed, layout y snapshot. Cada Chart declara los campos que calcula.
History se pide con
`include_raw_ticks: true`; el cliente conserva el JSON completo de History y de
cada evento live para que los Charts puedan calcular desde cualquier atributo
de Bar. La capa de cálculo reconcilia history y live por timestamp y publica un
snapshot inmutable compartido por todos los Charts del feed. `ChartSnapshot` expone
las Bars completas en `full_bars`. Para acceder a los campos de una Bar, usa
`BarSnapshot::attributes()`: devuelve el objeto de Bar tanto para history como
para eventos live (en estos últimos, extrae el objeto `data`). Usa
`BarSnapshot::source_payload()` solo si también necesitas el envoltorio del
evento live y sus metadatos, como `event_id` o `is_final`. En history, el payload
suele ser directamente el objeto Bar. Ambos métodos exponen la estructura JSON
parseada, no el texto original byte por byte. Los datos se comparten mediante
`Arc`, sin clonar el contenido al publicar el snapshot. El OHLCV actual solo
proyecta precio y `volume_flow.total_qty` para el dibujo.

Arrastra el asa `⠿` de un Chart y suéltalo sobre un borde de otro: arriba/abajo
para reordenarlos en filas, o a la izquierda/derecha para ponerlos en la misma
fila. Una línea verde indica dónde quedará el Chart.

El límite inicial es 50 Bars completas. «Cargar 50 Bars más» aumenta el límite
de la consulta; como el endpoint de history devuelve las últimas `limit` Bars,
se reconcilian con las ya cargadas por timestamp y revisión.

El botón «Logs» abre el registro de estado de la aplicación: configuración de
endpoints, ciclo de vida de tabs/Charts, conexiones y resultados/errores de
history. Mantiene un máximo de 500 entradas, agrupa mensajes repetidos y no
registra cada evento live.
