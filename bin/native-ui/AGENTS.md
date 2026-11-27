# Instrucciones para `native-ui`

- Al trabajar en este proyecto, consulta `egui_plot-0.38.0/` cuando necesites verificar la API o el comportamiento de `egui_plot`. Es la copia local de referencia para la versión usada por este proyecto.
- Empieza por su `README.md` y `egui_plot/Cargo.toml`; para detalles concretos, revisa los ejemplos y el código fuente dentro de esa carpeta.
- No supongas que la API coincide con otras versiones de `egui_plot`: comprueba los nombres, tipos y firmas en esta copia antes de implementarlos.
- Trata `egui_plot-0.38.0/` como código de dependencia: no lo modifiques salvo que la tarea pida expresamente cambiar esa dependencia.
- Mantén los cambios de la aplicación en el código de `native-ui` y no cambies la configuración de dependencias solo por consultar esta referencia.

## Tema visual

- Mantén una identidad oscura, minimalista y de alto contraste: fondo negro (`#000000`), texto y ejes blancos (`#FFFFFF`) y cuadrícula discreta LightSlateGray (`#778899`).
- En los gráficos, usa verde lima (`#00FF00`) para bordes y mechas de ambas velas, la línea de precio, señales alcistas y soportes importantes. Usa cuerpo negro (`#000000`) para velas alcistas y blanco (`#FFFFFF`) para bajistas; los volúmenes pueden usar LimeGreen (`#32CD32`).
- Reserva rojo (`#FF0000`) para el precio Ask, señales bajistas, resistencias y niveles de riesgo como Stop Loss. Usa LightSlateGray (`#778899`) para Bid.
- Para indicadores, evita competir visualmente con las velas: usa cian (`#00FFFF`) para la media móvil rápida, DodgerBlue (`#1E90FF`) para la lenta, Gold (`#FFD700`) para EMA de tendencia y entradas, Magenta (`#FF00FF`) para VWAP, MediumPurple (`#9370DB`) para RSI y DeepSkyBlue (`#00BFFF`) para MACD. Usa DimGray (`#696969`) para bandas de Bollinger.
- En objetos y señales, usa DarkGreen (`#006400`) para zonas de demanda, DarkRed (`#8B0000`) para zonas de oferta, LimeGreen (`#32CD32`) para Take Profit y Gainsboro (`#DCDCDC`) para anotaciones. Los acentos secundarios disponibles son MediumSeaGreen (`#3CB371`), GreenYellow (`#ADFF2F`), PaleGreen (`#98FB98`), Turquoise (`#40E0D0`), LightCoral (`#F08080`) y DarkSlateGray (`#2F4F4F`).
- Aplica amarillo y naranja con moderación para alertas y niveles relevantes; reserva violetas y magentas para indicadores secundarios u osciladores, y grises para cuadrículas, separadores y anotaciones discretas. Conserva siempre el contraste y el significado consistente de cada color.
