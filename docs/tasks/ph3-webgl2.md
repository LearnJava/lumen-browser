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
- `crates/js/src/webgl_bindings.rs` — старый fingerprint-only `WEBGL_SHIM` (dead
  file: не объявлен как `mod` в `lib.rs`, ни во что не компилируется). Срез 1
  подтвердил, что он не мог перекрывать функциональный шим — упоминание ниже
  было устаревшим.
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

### Срез 1 — S — `getContext('webgl2')` возвращает функциональный контекст — DONE 2026-09-21 (P1)
`_makeContext(cid, isWebgl2)` (`webgl_canvas.rs:135`) получил второй параметр:
`getContext` (`webgl_canvas.rs:391`) передаёт `t === 'webgl2'` при создании
контекста (тот же `_ctx`, что и для `'webgl'`/`'experimental-webgl'` — контекст
один функциональный объект на канвас, различаются только версия и набор
enum'ов). `getParameter(VERSION)`/`SHADING_LANGUAGE_VERSION` теперь возвращают
`'WebGL 2.0'`/`'WebGL GLSL ES 3.00'` для webgl2-контекста вместо всегда
`'WebGL 1.0'`. Добавлены WebGL2-only enum'ы (`RGBA8`, `HALF_FLOAT`,
`UNIFORM_BUFFER`, `PIXEL_PACK_BUFFER`/`PIXEL_UNPACK_BUFFER`,
`COPY_READ_BUFFER`/`COPY_WRITE_BUFFER`, `TRANSFORM_FEEDBACK_BUFFER`,
`SYNC_GPU_COMMANDS_COMPLETE`/`SYNC_FLUSH_COMMANDS_BIT`,
`ALREADY_SIGNALED`/`TIMEOUT_EXPIRED`/`CONDITION_SATISFIED`/`WAIT_FAILED`) —
условно, только на webgl2-контексте (`gl.UNIFORM_BUFFER === undefined` на
webgl1, как и в реальных браузерах, для feature-detection). `webgl_bindings.rs`
(старый fingerprint-stub) не подключён как модуль (`lib.rs` не содержит `mod
webgl_bindings`) — не мог затирать функциональный контекст, документация была
устаревшей. 5 новых тестов `webgl_canvas.rs` (версия 1 vs 2, отсутствие/наличие
WebGL2-enum'ов, полный draw+readback pipeline на webgl2-контексте). `cargo
clippy -p lumen-js -p lumen-paint --all-targets --features lumen-js/v8-backend
-D warnings` зелёные.

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

### Срез 3 — S/M — VAO (`createVertexArray`/`bindVertexArray`) — DONE 2026-09-21 (P1)
`SoftwareWebGl` получил новый тип `VertexArrayObject` (набор `attribs:
HashMap<u32, AttribPointer>` + захваченный `bound_element_array_buffer`,
теми же полями, что раньше были плоскими полями самого контекста) и
`vaos: HashMap<u32, VertexArrayObject>` + `bound_vertex_array` (0 = default,
т.е. старые `self.attribs`/`self.bound_element_array_buffer`, ничего не
переехало физически — просто появился второй адресуемый слой). Новые
приватные аксессоры `attribs_mut`/`attribs_active`/`element_array_binding[_mut]`
прозрачно читают/пишут либо default, либо активный VAO — через них
переведены `enable/disableVertexAttribArray`, `vertexAttribPointer`,
`bindBuffer(ELEMENT_ARRAY_BUFFER, …)`, `bufferData(ELEMENT_ARRAY_BUFFER, …)`,
`drawElements` и шейдерный путь (`collect_vertex_attribs`/`collect_positions`)
— ни один из них не хранит больше прямых ссылок на старые поля. `create_vertex_array`
(id≠0, монотонный)/`bind_vertex_array` (неизвестный id откатывается на
default, не запоминает висячий id)/`delete_vertex_array` (спека: удаление
активного VAO неявно биндит default)/`is_vertex_array`. JS-шим:
`_lumen_webgl_create_vertex_array`/`_bind_vertex_array`/`_delete_vertex_array`/
`_is_vertex_array` натив + `gl.createVertexArray`/`bindVertexArray`/
`deleteVertexArray`/`isVertexArray`, тот же `_wrap`/`_unwrap` паттерн, что и у
буферов; `webgl_bindings.rs` (упомянутый выше fingerprint-стаб) на самом деле
не существует в дереве — устаревшее упоминание, срез 1 уже фиксировал, что
он не подключён как `mod`. 5 новых тестов `webgl.rs` (id-уникальность,
изоляция attrib-pointers между VAO, изоляция `ELEMENT_ARRAY_BUFFER`-биндинга,
откат на default при неизвестном id, неявный rebind при удалении активного
VAO) + 2 новых V8-теста `webgl_canvas.rs` (переключение VAO прячет/возвращает
атрибут через полный draw+readback pipeline; `isVertexArray` true→false
после `deleteVertexArray`). `cargo clippy -p lumen-paint -p lumen-js
--all-targets --features lumen-js/v8-backend -D warnings` зелёные.

### Срез 4 — M — GLSL ES 3.00 в интерпретаторе — DONE 2026-09-21 (P1)
`glsl.rs::parse` получил параметр `ShaderStage` (`Vertex`/`Fragment`) — так же,
как срезы 1-3 узнают WebGL2 из явного флага `getContext`, а не из угадывания.
`#version 300 es` не потребовал отдельного кода: лексер уже пропускал любую
строку с `#` как директиву препроцессора (было верно и для ES 1.0). Новые
ветки `Token::KwIn`/`Token::KwOut` в `parse_top_level` резолвятся относительно
`stage` на существующие три ES 1.0 бакета: vertex `in` → `attributes`, vertex
`out`/fragment `in` → `varyings` (тот же механизм интерполяции, что и
`varying`), fragment `out vec4 <name>;` → новое поле `ParsedShader::frag_out_name`.
`exec_main` пре-сидит имя fragment-output как обычный local (`Val::Vec4([0;4])`,
как уже делалось для `varyings`) и после выполнения `main()` копирует его
финальное значение в `env.frag_color` — так `webgl.rs` продолжает читать один
и тот же `frag_color` независимо от ES 1.0 (`gl_FragColor`) или ES 3.00
(именованный `out`). `texture()` уже был алиасом `texture2D` с среза 1
(`eval_call` match `"texture2D" | "texture"`), доработки не потребовалось.
Единственный внешний вызывающий (`webgl.rs::compile_shader`) передаёт стадию
по `Shader::kind` (`VERTEX_SHADER`/`FRAGMENT_SHADER`). 5 новых тестов `glsl.rs`
(`#version 300 es` пропускается, vertex `in`→attribute, vertex
`out`/fragment `in` делят varying, `texture()` как алиас, реальный сэмплер) +
1 новый V8-тест `webgl_canvas.rs` (`es3_glsl_shader_pipeline_paints_pixels`:
полный compile→link→drawArrays→readPixels на паре ES 3.00 шейдеров с `in`/`out`
и произвольным `out vec4 outColor`). `cargo clippy -p lumen-paint -p lumen-js
--all-targets --features lumen-js/v8-backend -D warnings` зелёные.

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

### Срез 6 — XS — `uniform*v` / `uniformMatrix3fv` — DONE 2026-09-21 (P1)
`uniform1fv`/`2fv`/`3fv`/`4fv`/`1iv` уже были прокинуты в JS-шиме
(`webgl_canvas.rs`) как тонкие обёртки над скалярными `uniform1..4f`/`uniform1i`
натива (единственный элемент массива — интерпретатор не поддерживает
uniform-массивы, только один вектор на локацию, так что это точный
эквивалент). Оставался только `uniformMatrix3fv` — был `function() {}`-стабом
(«mat3 not tracked»). Новый `SoftwareWebGl::uniform_matrix3fv(location,
values: &[f32; 9])` встраивает 3×3-матрицу в тот же padded-`Val::Mat4`
(единичная диагональ в неиспользуемой строке/столбце), которым уже пользуется
GLSL-конструктор `mat3()` — существующая `Mat4`-арифметика интерпретатора
подхватывает её без изменений. Новый натив `_lumen_webgl_uniform_mat3fv` +
`gl.uniformMatrix3fv` в шиме. 2 новых теста `webgl.rs` (встраивание,
игнорирование отрицательной локации/короткого входа) + 1 V8-тест
`webgl_canvas.rs` (`uniformMatrix3fv` с нулевой матрицей вырождает треугольник
в точку — доказывает, что юниформ реально доходит до вершинного шейдера).
`Val` получил `#[derive(PartialEq)]` для теста сравнения. `cargo clippy
-p lumen-paint -p lumen-js --all-targets --features lumen-js/v8-backend
-D warnings` зелёные.

## Tests

- Юнит `crates/engine/paint/src/webgl.rs` (mod tests, `webgl.rs:922`): добавить
  `draw_elements_indexed_quad`, VAO-переключение, ES 3.00-шейдер рисует градиент.
- Юнит `crates/js/src/webgl_canvas.rs` (mod tests): `getContext('webgl2')`
  не null; `readPixels` после `drawElements` даёт ожидаемый цвет.
- graphic_tests: новый `graphic_tests/NN-webgl2.html` (магента-рамка) —
  WebGL2-треугольник/quad с ES 3.00 шейдером; демо в `1000000-final.html`;
  запись в `COVERAGE.md` + `TESTS` в `run.py` (проверит present-путь, срез 5).

## Definition of done

- [x] **`getContext('webgl2')` возвращает функциональный контекст** (не fingerprint-stub) — срез 1, 2026-09-21.
- [x] **`drawElements` + `ELEMENT_ARRAY_BUFFER`** (u8/u16/u32 индексы) работают — срез 2, 2026-09-21.
- [x] **VAO (`createVertexArray`/`bindVertexArray`) реализованы** — срез 3, 2026-09-21.
- [x] **GLSL ES 3.00 (`#version 300 es`, `in`/`out`, `texture()`) исполняется** — срез 4, 2026-09-21.
- [x] **Present:** результат WebGL композитится на страничный `<canvas>` (видно в окне, не только `readPixels`) — срез 5, 2026-09-21.
- [x] **`uniform*v`/`uniformMatrix3fv`** реализованы — срез 6, 2026-09-21.
- [ ] graphic_test `NN-webgl2` проходит (порог 0.5%).
- [x] `CAPABILITIES.md` + `subsystems/paint.md` обновлены (webgl2 ✅/🟡).
