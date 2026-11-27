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

Cada tab abre un feed aislado y contiene una lista de Charts. Cada Chart declara
su límite de history, si requiere live y los campos que calcula. History se pide
con `include_raw_ticks: true`; el cliente conserva el JSON completo de History y
de cada evento live para que los Charts puedan calcular desde cualquier atributo
de Bar. El OHLCV actual solo proyecta precio y `volume_flow.total_qty` para el
dibujo.

El límite inicial es 50 Bars completas. «Cargar 50 Bars más» aumenta el límite
de la consulta; como el endpoint de history devuelve las últimas `limit` Bars,
se reconcilian con las ya cargadas por timestamp y revisión.

El botón «Logs» abre el registro de estado de la aplicación: configuración de
endpoints, ciclo de vida de tabs/Charts, conexiones y resultados/errores de
history. Mantiene un máximo de 500 entradas, agrupa mensajes repetidos y no
registra cada evento live.
