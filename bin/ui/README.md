# Nuxt Starter Template

[![Nuxt UI](https://img.shields.io/badge/Made%20with-Nuxt%20UI-00DC82?logo=nuxt&labelColor=020420)](https://ui.nuxt.com)

Use this template to get started with [Nuxt UI](https://ui.nuxt.com) quickly.

- [Live demo](https://starter-template.nuxt.dev/)
- [Documentation](https://ui.nuxt.com/docs/getting-started/installation/nuxt)

<a href="https://starter-template.nuxt.dev/" target="_blank">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://ui.nuxt.com/assets/templates/nuxt/starter-dark.png">
    <source media="(prefers-color-scheme: light)" srcset="https://ui.nuxt.com/assets/templates/nuxt/starter-light.png">
    <img alt="Nuxt Starter Template" src="https://ui.nuxt.com/assets/templates/nuxt/starter-light.png" width="830" height="466">
  </picture>
</a>

> The starter template for Vue is on https://github.com/nuxt-ui-templates/starter-vue.

## Quick Start

```bash [Terminal]
npm create nuxt@latest -- -t ui
```

## Deploy your own

[![Deploy with Vercel](https://vercel.com/button)](https://vercel.com/new/clone?repository-name=starter&repository-url=https%3A%2F%2Fgithub.com%2Fnuxt-ui-templates%2Fstarter&demo-image=https%3A%2F%2Fui.nuxt.com%2Fassets%2Ftemplates%2Fnuxt%2Fstarter-dark.png&demo-url=https%3A%2F%2Fstarter-template.nuxt.dev%2F&demo-title=Nuxt%20Starter%20Template&demo-description=A%20minimal%20template%20to%20get%20started%20with%20Nuxt%20UI.)

## Setup

Make sure to install the dependencies:

```bash
pnpm install
```

## Development Server

Start the development server on `http://localhost:3000`:

```bash
pnpm dev
```

## WebAssembly

El módulo Rust de ejemplo está en `wasm/` y exporta `add`. Para compilarlo
manualmente desde `bin/ui`:

```bash
rustup target add wasm32-unknown-unknown
cargo build --manifest-path wasm/Cargo.toml --target wasm32-unknown-unknown --release
mkdir -p public/wasm
cp wasm/target/wasm32-unknown-unknown/release/ui_wasm.wasm public/wasm/basic.wasm
```

Nuxt sirve ese archivo en `/wasm/basic.wasm`. Desde código de `app/`, se puede
cargar y llamar así:

```ts
import { loadBasicWasm } from '~/utils/basicWasm'

const wasm = await loadBasicWasm()
const result = wasm.add(2, 3) // 5
```

La vista de chart obtiene las últimas 50 barras del timeframe seleccionado de
`POST /api/market/get-history` y se suscribe al WebSocket `/api/market/ws` con
el mismo `source`, `symbol` y `timeframe`. La consulta histórica omite
`raw_ticks` para evitar respuestas de cientos de MB; el Institutional Trade
Profile carga aparte los `raw_ticks` de las últimas Bars 1m (5 por defecto,
configurable entre 1 y 60). El WebSocket sigue entregando Bars completos. Solo
se normalizan el tiempo y OHLCV usados para dibujar. El proxy HTTP usa
`NUXT_MARKET_SERVER_URL` (por defecto `http://localhost:3001/api/market`) y la
URL pública del WebSocket se puede cambiar con
`NUXT_PUBLIC_MARKET_WEBSOCKET_URL` (por defecto
`ws://localhost:3001/api/market/ws`).
Debajo del chart principal, VELAS DE DELTA muestra un chart independiente con
hasta las últimas 15 Bars del timeframe seleccionado que incluyen `raw_ticks`.
Permite alternar entre delta reiniciado por Bar (PER_BAR) y delta acumulado (CVD);
los valores y la escala usan cantidades, nunca precios.

## Production

Build the application for production:

```bash
pnpm build
```

Locally preview production build:

```bash
pnpm preview
```

Check out the [deployment documentation](https://nuxt.com/docs/getting-started/deployment) for more information.

## Renovate integration

Install [Renovate GitHub app](https://github.com/apps/renovate/installations/select_target) on your repository and you are good to go.
