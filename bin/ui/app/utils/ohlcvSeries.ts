import type { ChartEngine } from '../packages/open-charts/src/core/ChartEngine'
import type { MainPane, SeriesDefinition } from '../packages/open-charts/src/core/types'
import type { OhlcvBar } from './basicWasm'

interface ChartOhlcvBar extends Omit<OhlcvBar, 'time'> {
  /** Unix timestamp in seconds, as expected by open-charts. */
  time: number
}

interface OhlcvParams {
  bullColor: string
  bearColor: string
}

export function createOhlcvSeries(): SeriesDefinition<ChartOhlcvBar, ChartOhlcvBar, OhlcvParams> {
  const params: OhlcvParams = {
    bullColor: '#089981',
    bearColor: '#f23645'
  }

  return {
    id: 'ohlcv',
    label: 'OHLCV',
    color: params.bullColor,
    layer: 'foreground',
    width: '100%',
    height: '100%',
    params,

    compute(data) {
      return [...data]
    },

    render(
      ctx: CanvasRenderingContext2D,
      pane: MainPane,
      engine: ChartEngine,
      _data: readonly ChartOhlcvBar[],
      values: readonly ChartOhlcvBar[],
      seriesParams: OhlcvParams,
      priceMin: number,
      priceMax: number
    ) {
      const candleWidth = Math.max(1, Math.floor(engine.barWidth * 0.7))
      const halfWidth = candleWidth / 2

      for (let index = engine.viewStart; index < engine.viewEnd && index < values.length; index++) {
        const bar = values[index]

        if (!bar) {
          continue
        }

        const x = Math.round(engine.utils.xOf(index)) + 0.5
        const yHigh = engine.utils.yOf(bar.high, pane, priceMin, priceMax)
        const yLow = engine.utils.yOf(bar.low, pane, priceMin, priceMax)
        const yOpen = engine.utils.yOf(bar.open, pane, priceMin, priceMax)
        const yClose = engine.utils.yOf(bar.close, pane, priceMin, priceMax)
        const color = bar.close >= bar.open ? seriesParams.bullColor : seriesParams.bearColor

        ctx.strokeStyle = color
        ctx.lineWidth = 1
        ctx.beginPath()
        ctx.moveTo(x, yHigh)
        ctx.lineTo(x, yLow)
        ctx.stroke()

        const bodyTop = Math.min(yOpen, yClose)
        const bodyHeight = Math.max(1, Math.abs(yClose - yOpen))

        ctx.fillStyle = color
        ctx.fillRect(x - halfWidth, bodyTop, candleWidth, bodyHeight)
      }
    },

    updateIncremental(data, values, isNewBar) {
      const lastBar = data.at(-1)

      if (!lastBar) {
        return
      }

      if (isNewBar) {
        values.push(lastBar)
      } else {
        values[values.length - 1] = lastBar
      }
    },

    tooltipRow(values, index) {
      const bar = values[index]

      if (!bar) {
        return null
      }

      return {
        label: 'OHLCV',
        value: `O ${bar.open.toFixed(2)} · H ${bar.high.toFixed(2)} · L ${bar.low.toFixed(2)} · C ${bar.close.toFixed(2)} · V ${bar.volume.toFixed(4)}`
      }
    },

    priceTags(_data, values) {
      const lastBar = values.at(-1)

      return lastBar
        ? [{
            value: lastBar.close,
            color: lastBar.close >= lastBar.open ? params.bullColor : params.bearColor,
            label: ''
          }]
        : []
    },

    valueRange(data, _values, start, end) {
      let low = Infinity
      let high = -Infinity

      for (let index = start; index < end; index++) {
        const bar = data[index]

        if (bar) {
          low = Math.min(low, bar.low)
          high = Math.max(high, bar.high)
        }
      }

      return Number.isFinite(low) && Number.isFinite(high)
        ? { lo: low, hi: high }
        : { lo: 0, hi: 1 }
    },

    legend(data, _values, index, seriesParams) {
      const bar = data[index]

      if (!bar) {
        return []
      }

      const color = bar.close >= bar.open ? seriesParams.bullColor : seriesParams.bearColor

      return [
        { label: 'O', value: bar.open.toFixed(2), color },
        { label: 'H', value: bar.high.toFixed(2), color },
        { label: 'L', value: bar.low.toFixed(2), color },
        { label: 'C', value: bar.close.toFixed(2), color },
        { label: 'V', value: bar.volume.toFixed(4), color }
      ]
    }
  }
}
