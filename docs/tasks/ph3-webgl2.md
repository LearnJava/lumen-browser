# Задача: WebGL2 подмножество

**Developer:** P1
**Ветка:** `p1-webgl2`
**Размер:** M
**Крейты:** `lumen-paint`, `lumen-js`

## Goal

Довести функциональный `canvas.getContext('webgl2')` до рабочего подмножества
WebGL 2.0 (WebGL 2.0 Specification, поверх ES 3.0): VAO, `drawElements`
(индексированный рендер), UBO/`uniform*v`, GLSL ES 3.00 (`#version 300 es`,
`in`/`out`, `texture()`), и главное — present framebuffer в страничный
`<canvas>` (сейчас пиксели можно только вычитать через `readPixels`).

## Current state (сверено с кодом 2026-07-05)

WebGL **1.0** уже функционален; «webgl2» пока только fingerprint-заглушка.

- `crates/js/src/webgl_canvas.rs:57` — `install_webgl_canvas()`: полноценный WebGL 1.0.
  JS-шим `WEBGL_SHIM` (`webgl_canvas.rs:220`) перехватывает
  `createElement('canvas').getContext(...)` и строит контекст поверх нативных
  `_lumen_webgl_*`. Реализовано: `createBuffer`/`bufferData`,
  `createShader`/`compileShader`/`linkProgram`/`useProgram`,
  `vertexAttribPointer`/`enableVertexAttribArray`, `uniform1f..4f`/`uniform1i`/
  `uniformMatrix4fv`, `activeTexture`/`bindTexture`/`texImage2D`, `drawArrays`,
  `readPixels`.
- `crates/js/src/webgl_bindings.rs:40` — старый `WEBGL_SHIM`: **stub** для
  `webgl`/`webgl2`, только `getParameter(UNMASKED_*)` для нормализации
  fingerprint. Никакого рендера. Возвращает `is_webgl2 ? 'WebGL 2.0'` строки
  (`webgl_bindings.rs:67`) — это косметика, не движок.
- `crates/engine/paint/src/webgl.rs:114` — `SoftwareWebGl`: CPU-растеризатор.
  `draw_arrays` (`webgl.rs:498`) с shaded-путём и flat-fill fallback.
  `drawElements` реализован (срез 2, 2026-09-21) — общий `draw_indexed` путь.
  **Нет**: VAO, instancing, MRT, UBO.
- `crates/engine/paint/src/glsl.rs:1` — GLSL ES **1.0** интерпретатор.
  `glsl.rs:24` явно: `#version` и препроцессор **не поддержаны**; `attribute`/
  `varying`/`gl_FragColor` (ES 1.0), а не `in`/`out`/`texture()` (ES 3.0).
- **Present-gap:** `webgl_canvas.rs:480` — `canvas.toDataURL()` → `'data:,'`;
  результат WebGL нигде не композитится на экранный `<canvas>`. Пиксели
  доступны только через `_lumen_webgl_read_pixels` (`webgl_canvas.rs:202`).
  Для сравнения: DOM-canvas 2D композитится через offscreen-путь
  (`offscreen_canvas.rs::flush_dirty`), у WebGL аналога нет.

## Entry points

- `crates/js/src/webgl_canvas.rs:57` — `install_webgl_canvas`: регистрация нативов + шим.
- `crates/js/src/webgl_canvas.rs:220` — `WEBGL_SHIM`: JS-контекст, тут добавлять webgl2-методы.
- `crates/engine/paint/src/webgl.rs:114` — `SoftwareWebGl`: состояние и растеризация.
- `crates/engine/paint/src/webgl.rs:498` — `draw_arrays` (образец для `draw_elements`).
- `crates/engine/paint/src/glsl.rs:1` — GLSL-интерпретатор (расширять до ES 3.00).
- `crates/js/src/lib.rs` — место установки бандла бингов (проверить порядок install; сейчас `webgl_canvas` перекрывает fingerprint-shim для `webgl`).

## Срезы (декомпозиция)

### Срез 1 — S — `getContext('webgl2')` возвращает функциональный контекст
В `WEBGL_SHIM` (`webgl_canvas.rs:220`) сейчас `getContext` обрабатывает `'webgl'`
(проверить точную ветку в файле после стр. 279). Добавить обработку `'webgl2'`/
`'experimental-webgl2'` → тот же `_makeContext(cid)` + флаг `is_webgl2`, чтобы
`getParameter(VERSION)` вернул `'WebGL 2.0'`, а также добавить WebGL2-константы
(`UNIFORM_BUFFER`, `SYNC_*`, `RGBA8`, `HALF_FLOAT` и т.п.). Убедиться, что
fingerprint-shim `webgl_bindings.rs` не затирает функциональный webgl2.

### Срез 2 — S — `drawElements` + `ELEMENT_ARRAY_BUFFER` — DONE 2026-09-21 (P1)
`SoftwareWebGl` получил отдельное хранилище индексов (`element_buffers`,
`bound_element_array_buffer`, `buffer_data_elements`) — `bind_buffer` теперь
трекает оба таргета (`ARRAY_BUFFER`/`ELEMENT_ARRAY_BUFFER`), индексы всегда
хранятся расширенными до u32 независимо от исходного типа (`Uint8Array`/
`Uint16Array`/`Uint32Array`). `draw_arrays`/`draw_arrays_shaded`/
`collect_positions` рефакторены на общий `draw_indexed(mode, indices: &[usize])`:
`drawArrays` передаёт непрерывный диапазон, новый `draw_elements(mode, count,
gl_type, offset_bytes)` — срез индексного буфера (offset в байтах, gl_type ∈
`UNSIGNED_BYTE`/`_SHORT`/`_INT` даёт размер элемента для конвертации в индекс
начала); оба пути делят и шейдерный, и flat-fill рендер без дублирования.
JS-шим: `_lumen_webgl_buffer_data_elements` (новый натив, `bufferData` в
`webgl_canvas.rs` роутит по таргету), `gl.drawElements` → `_lumen_webgl_draw_elements`
(тот же `present()`-хук после мутации framebuffer, что и `drawArrays`). 5 новых
тестов `webgl.rs` (индексированный quad, byte-offset, отсутствующий индексный
буфер — noop, выход за границы — noop не паникует, шейдерный путь) + 1 новый
V8-тест `webgl_canvas.rs` (полный pipeline с `Uint16Array`-индексами через
`readPixels`). `cargo clippy -p lumen-paint -p lumen-js --all-targets --features
lumen-js/v8-backend -D warnings` зелёные.

### Срез 3 — S/M — VAO (`createVertexArray`/`bindVertexArray`)
Сейчас атрибуты — плоский `attribs: HashMap<u32, AttribPointer>`
(`webgl.rs:142`). Добавить объекты VAO (набор AttribPointer + element-binding),
методы `create_vertex_array`/`bind_vertex_array`/`delete_vertex_array`; при
активном VAO читать атрибуты из него. JS-шим fingerprint (`webgl_bindings.rs:56`)
уже эмулирует VAO-заглушку — заменить на реальную в `webgl_canvas`.

### Срез 4 — M — GLSL ES 3.00 в интерпретаторе
В `glsl.rs`: распознавать `#version 300 es` (пропускать строку), маппить
`in`/`out` (вместо `attribute`/`varying`), встроенную `texture()` (= `texture2D`),
и выходную переменную фрагмента (произвольное `out vec4`, а не только
`gl_FragColor`). Держать обратную совместимость с ES 1.0 (детект по наличию
`#version 300`).

### Срез 5 — M — Present framebuffer в страничный `<canvas>` — DONE 2026-09-21 (P1)
Главный видимый gap закрыт. `webgl_canvas.rs::CONTEXTS` теперь хранит
`WebGlEntry { gl, nid }` — `nid` берётся из `el.__nid__` в момент
`getContext('webgl'|'webgl2')` и прокидывается через новый 3-аргументный
`_lumen_webgl_create(nid, w, h)`. `_lumen_webgl_clear`/`_lumen_webgl_draw_arrays`
после мутации framebuffer зовут новую `present(id)`: flip bottom-left→top-left
(`flip_rows_rgba`, тот же переворот, что уже делал `readPixels` по под-прямоугольнику)
+ `canvas2d::present_rgba(nid, w, h, rgba)` — тот же хук, что уже использует
WebGPU-present (`_lumen_webgpu_canvas_present`), так что шелл композитит WebGL
как обычный `canvas:{nid}`. Контексты без backing-элемента (`nid` отсутствует,
юнит-тесты `install_minimal_dom`) просто не презентуют — не паникуют.
Остаток/побочный эффект: `CanvasNoiseGenerator` по-прежнему не подключён к
software-GL пути, так что presented-бufer, как и `readPixels`, отдаёт точные
пиксели — `drawImage(webglCanvas, …)` на 2D-канвас теперь тоже это унаследовал
(см. CAPABILITIES.md, тот же класс остатка, что и у WebGPU-present). 2 новых
теста (`clear_presents_to_page_canvas`, `context_without_nid_does_not_present`);
ловушка — ассерты обязаны идти через `rt.flush_canvas_updates()`
(marshal на JS-поток раннера), а не напрямую `canvas2d::flush_dirty()` с
вызывающего потока теста — у раннера свой `thread_local!`.

### Срез 6 — XS — `uniform*v` / `uniformMatrix3fv`
Добавить `uniform2fv`/`3fv`/`4fv`/`1iv` и `uniformMatrix3fv` (WebGL2 часто
их использует), прокинуть в `SoftwareWebGl::uniform_*` и JS-шим.

## Tests

- Юнит `crates/engine/paint/src/webgl.rs` (mod tests, `webgl.rs:922`): добавить
  `draw_elements_indexed_quad`, VAO-переключение, ES 3.00-шейдер рисует градиент.
- Юнит `crates/js/src/webgl_canvas.rs` (mod tests): `getContext('webgl2')`
  не null; `readPixels` после `drawElements` даёт ожидаемый цвет.
- graphic_tests: новый `graphic_tests/NN-webgl2.html` (магента-рамка) —
  WebGL2-треугольник/quad с ES 3.00 шейдером; демо в `1000000-final.html`;
  запись в `COVERAGE.md` + `TESTS` в `run.py` (проверит present-путь, срез 5).

## Definition of done

- [ ] `getContext('webgl2')` возвращает функциональный контекст (не fingerprint-stub).
- [x] **`drawElements` + `ELEMENT_ARRAY_BUFFER`** (u8/u16/u32 индексы) работают — срез 2, 2026-09-21.
- [ ] VAO (`createVertexArray`/`bindVertexArray`) реализованы.
- [ ] GLSL ES 3.00 (`#version 300 es`, `in`/`out`, `texture()`) исполняется.
- [x] **Present:** результат WebGL композитится на страничный `<canvas>` (видно в окне, не только `readPixels`) — срез 5, 2026-09-21.
- [ ] graphic_test `NN-webgl2` проходит (порог 0.5%).
- [ ] `CAPABILITIES.md` + `subsystems/paint.md` обновлены (webgl2 ✅/🟡).
