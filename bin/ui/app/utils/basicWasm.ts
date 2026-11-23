export interface BasicWasmExports {
  add: (left: number, right: number) => number
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

      if (typeof exports.add !== 'function') {
        throw new Error('basic.wasm no exporta la función add')
      }

      return exports
    })

  return wasmInstance
}
