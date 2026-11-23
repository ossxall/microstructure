export interface BasicWasmExports {
  memory: WebAssembly.Memory
  add: (left: number, right: number) => number
  alloc_f64: (length: number) => number
  free_f64: (pointer: number, length: number) => void
  resample_ohlcv: (pointer: number, barCount: number, timeframeMs: number) => number
}

export interface OhlcvBar {
  /** Unix timestamp in milliseconds. */
  time: number
  open: number
  high: number
  low: number
  close: number
  volume: number
}

let wasmInstance: Promise<BasicWasmExports> | undefined

export function loadBasicWasm(): Promise<BasicWasmExports> {
  wasmInstance ??= fetch('/wasm/basic.wasm')
    .then(async (response) => {
      if (!response.ok) {
        throw new Error(`No se pudo cargar basic.wasm (${response.status})`)
      }

      const { instance } = await WebAssembly.instantiate(await response.arrayBuffer())
      const exports = instance.exports as unknown as BasicWasmExports

      if (
        !(exports.memory instanceof WebAssembly.Memory)
        || typeof exports.add !== 'function'
        || typeof exports.alloc_f64 !== 'function'
        || typeof exports.free_f64 !== 'function'
        || typeof exports.resample_ohlcv !== 'function'
      ) {
        throw new Error('basic.wasm no exporta la interfaz OHLCV esperada')
      }

      return exports
    })

  return wasmInstance
}

export async function resampleOhlcv(
  bars: readonly OhlcvBar[],
  timeframeMs: number
): Promise<OhlcvBar[]> {
  if (bars.length === 0) {
    return []
  }

  if (
    !Number.isSafeInteger(timeframeMs)
    || timeframeMs <= 0
    || timeframeMs > 0xFFFFFFFF
  ) {
    throw new Error('El timeframe debe ser un entero positivo en milisegundos')
  }

  const wasm = await loadBasicWasm()
  const bufferLength = bars.length * 6
  const pointer = wasm.alloc_f64(bufferLength)

  if (!pointer) {
    throw new Error('No se pudo reservar memoria para remuestrear las barras')
  }

  try {
    const input = new Float64Array(wasm.memory.buffer, pointer, bufferLength)

    bars.forEach((bar, index) => {
      input.set(
        [bar.time, bar.open, bar.high, bar.low, bar.close, bar.volume],
        index * 6
      )
    })

    const outputCount = wasm.resample_ohlcv(pointer, bars.length, timeframeMs)

    if (outputCount > bars.length) {
      throw new Error('WASM devolvió un número de barras inválido')
    }

    const output = new Float64Array(
      wasm.memory.buffer,
      pointer,
      outputCount * 6
    ).slice()

    return Array.from({ length: outputCount }, (_, index) => {
      const offset = index * 6

      return {
        time: output[offset]!,
        open: output[offset + 1]!,
        high: output[offset + 2]!,
        low: output[offset + 3]!,
        close: output[offset + 4]!,
        volume: output[offset + 5]!
      }
    })
  } finally {
    wasm.free_f64(pointer, bufferLength)
  }
}
