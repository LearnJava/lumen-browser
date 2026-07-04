# SYMBOLS

Auto-generated public API index. Regenerate: `python scripts/gen_symbols.py`

**Usage:** grep for a symbol → get `file:line` → `Read file offset=N limit=30`.

## lumen-a11y  (28 symbols)

`crates/engine/a11y/src/lib.rs:25` **enum** `LiveRegion` — `aria-live` values per WAI-ARIA §6.6
`crates/engine/a11y/src/lib.rs:34` **enum** `AriaCurrent` — `aria-current` values per WAI-ARIA §5.4.1
`crates/engine/a11y/src/lib.rs:53` **struct** `AXState` — ARIA state and property flags for one accessibility node
`crates/engine/a11y/src/lib.rs:114` **struct** `AXNode` — One node in the accessibility tree
`crates/engine/a11y/src/lib.rs:146` **struct** `AXTree` — Accessibility tree rooted at a document node
`crates/engine/a11y/src/lib.rs:161` **fn** `build_ax_tree` — Build an `AXTree` from a `Document` starting at `root_id`
`crates/engine/a11y/src/names.rs:18` **fn** `compute_name` — Compute the accessible name for a DOM node (ACCNAME-1.2 §4.3)
`crates/engine/a11y/src/names.rs:176` **fn** `compute_description` — Compute the accessible description for a DOM node (ACCNAME-1.2 §4.3.2)
`crates/engine/a11y/src/platform/linux.rs:32` **struct** `AtSpiBridge` — Linux AT-SPI2 accessibility bridge
`crates/engine/a11y/src/platform/linux.rs:41` **fn** `new` — Create a new, uninitialized AT-SPI2 bridge
`crates/engine/a11y/src/platform/linux.rs:46` **fn** `last_tree` — Return the last-received accessibility tree, if any
`crates/engine/a11y/src/platform/linux.rs:51` **fn** `focused_node` — Return the currently focused node, if any
`crates/engine/a11y/src/platform/macos.rs:26` **struct** `MacA11yBridge` — macOS NSAccessibility bridge
`crates/engine/a11y/src/platform/macos.rs:35` **fn** `new` — Create a new, uninitialized NSAccessibility bridge
`crates/engine/a11y/src/platform/macos.rs:40` **fn** `last_tree` — Return the last-received accessibility tree, if any
`crates/engine/a11y/src/platform/macos.rs:45` **fn** `focused_node` — Return the currently focused node, if any
`crates/engine/a11y/src/platform/mod.rs:25` **trait** `PlatformBridge` — Trait for platform-specific accessibility bridges
`crates/engine/a11y/src/platform/mod.rs:61` **struct** `NullBridge` — No-op bridge for headless runs, tests, and unsupported platforms
`crates/engine/a11y/src/platform/mod.rs:75` **fn** `platform_bridge` — Create the platform bridge appropriate for the current OS
`crates/engine/a11y/src/platform/windows.rs:49` **struct** `WinUiaBridge` — Windows UI Automation bridge
`crates/engine/a11y/src/platform/windows.rs:65` **fn** `new` — Create a new, uninitialised UIA bridge
`crates/engine/a11y/src/platform/windows.rs:75` **fn** `last_tree` — Return the last-received accessibility tree, if any
`crates/engine/a11y/src/platform/windows.rs:80` **fn** `focused_node` — Return the currently focused node, if any
`crates/engine/a11y/src/platform/windows.rs:238` **fn** `ax_role_to_msaa` — Map a Lumen `AXRole` to a Windows MSAA `ROLE_SYSTEM_*` constant
`crates/engine/a11y/src/roles.rs:14` **enum** `AXRole` — All WAI-ARIA 1.2 roles
`crates/engine/a11y/src/roles.rs:185` **fn** `as_str` — Canonical lowercase WAI-ARIA role string
`crates/engine/a11y/src/roles.rs:266` **fn** `parse` — Parse a WAI-ARIA role string (case-insensitive)
`crates/engine/a11y/src/roles.rs:349` **fn** `implicit_role` — Compute the implicit WAI-ARIA role for a DOM node per HTML-AAM §5

## lumen-bench  (3 symbols)

`crates/bench/src/ci_gate.rs:36` **fn** `run_ci_gate` — Run the CI performance gate
`crates/bench/src/util.rs:9` **fn** `get_rss_bytes` — Returns the current process RSS (resident set size) in bytes
`crates/bench/src/util.rs:48` **fn** `extract_style_blocks` — Concatenates all `<style>` text blocks from the document

## lumen-bidi-server  (26 symbols)

`crates/bidi-server/src/protocol.rs:159` **struct** `BidiState` — Connection-level BiDi state
`crates/bidi-server/src/protocol.rs:228` **fn** `new` — Новое пустое состояние соединения (без живого окна — Phase 1 stub behavior)
`crates/bidi-server/src/protocol.rs:234` **fn** `with_live_session` — State connected to a live shell window (SDC-2): real navigation,
`crates/bidi-server/src/protocol.rs:273` **fn** `locale`
`crates/bidi-server/src/protocol.rs:282` **fn** `timezone`
`crates/bidi-server/src/protocol.rs:289` **fn** `is_offline`
`crates/bidi-server/src/protocol.rs:296` **fn** `user_agent_for`
`crates/bidi-server/src/protocol.rs:309` **fn** `viewport_for`
`crates/bidi-server/src/protocol.rs:321` **fn** `cache_behavior`
`crates/bidi-server/src/protocol.rs:328` **fn** `intercept_count`
`crates/bidi-server/src/protocol.rs:337` **fn** `preload_scripts_for_context` — Return preload scripts that apply to `context_id`
`crates/bidi-server/src/protocol.rs:349` **fn** `begin_download` — Register a new download and emit `browser.downloadWillBegin` if subscribed
`crates/bidi-server/src/protocol.rs:376` **fn** `update_download` — Update download progress and emit `browser.downloadItemUpdated` if subscribed
`crates/bidi-server/src/protocol.rs:401` **fn** `complete_download` — Mark download as completed and emit `browser.downloadItemCompleted` if subscribed
`crates/bidi-server/src/protocol.rs:421` **fn** `abort_download` — Mark download as aborted and emit `browser.downloadItemAborted` if subscribed
`crates/bidi-server/src/protocol.rs:444` **fn** `record_cookie_change` — Record a cookie change (add/update/remove) and emit `storage.cookie*` events
`crates/bidi-server/src/protocol.rs:491` **fn** `fire_user_prompt` — Open a user-prompt dialog and emit `browsingContext.userPromptOpened` if subscribed
`crates/bidi-server/src/protocol.rs:530` **fn** `open_prompt_count` — Number of currently open user prompts (for testing)
`crates/bidi-server/src/protocol.rs:536` **fn** `cookie_count` — Number of cookies in the session (for testing)
`crates/bidi-server/src/protocol.rs:542` **fn** `download_count` — Number of active download items
`crates/bidi-server/src/protocol.rs:548` **fn** `preload_script_count` — Number of registered preload scripts
`crates/bidi-server/src/protocol.rs:560` **fn** `record_response_body`
`crates/bidi-server/src/protocol.rs:574` **struct** `DispatchResult` — Результат обработки одной команды
`crates/bidi-server/src/protocol.rs:589` **fn** `dispatch` — Обработать одно BiDi-сообщение, вернуть фреймы для отправки клиенту
`crates/bidi-server/src/server.rs:23` **fn** `spawn` — Spawn the BiDi server on `127.0.0.1:port`. Non-blocking — runs in a background thread
`crates/bidi-server/src/transport.rs:20` **fn** `handle` — Handle one accepted TCP stream: WS upgrade → BiDi command loop

## lumen-canvas  (98 symbols)

`crates/engine/canvas/src/color.rs:3` **struct** `CanvasColor` — RGBA color used by the Canvas 2D API
`crates/engine/canvas/src/color.rs:11` **fn** `rgba`
`crates/engine/canvas/src/color.rs:16` **fn** `with_alpha_mult` — Multiply `self.a` by `alpha` (0.0–1.0)
`crates/engine/canvas/src/color.rs:25` **fn** `from_css_str` — Parse a CSS color string.  Supports:
`crates/engine/canvas/src/fp_noise.rs:17` **struct** `CanvasNoiseGenerator` — Per-session canvas fingerprint noise generator
`crates/engine/canvas/src/fp_noise.rs:27` **fn** `new` — Create a new noise generator with the given per-session seed
`crates/engine/canvas/src/fp_noise.rs:48` **fn** `next_noise_u8` — Generate next noise byte (0..=255) clamped to safe range
`crates/engine/canvas/src/fp_noise.rs:56` **fn** `apply_noise_to_pixel` — Add per-channel noise to an RGBA pixel
`crates/engine/canvas/src/fp_noise.rs:66` **fn** `apply_noise_to_buffer` — Apply noise to an entire RGBA buffer (row-major, top-left origin)
`crates/engine/canvas/src/fp_noise.rs:77` **fn** `reset` — Reset the RNG state to the seed (for reproducibility)
`crates/engine/canvas/src/lib.rs:35` **enum** `CompositeOperation` — CSS `globalCompositeOperation` — Porter-Duff compositing mode
`crates/engine/canvas/src/lib.rs:74` **fn** `from_str` — Parse from the CSS string literal used in `ctx.globalCompositeOperation`
`crates/engine/canvas/src/lib.rs:97` **fn** `as_str` — Canonical CSS string name for this operation
`crates/engine/canvas/src/lib.rs:121` **enum** `LineCap` — CSS `lineCap` — how line endpoints are rendered
`crates/engine/canvas/src/lib.rs:134` **fn** `from_str` — Parse from CSS string
`crates/engine/canvas/src/lib.rs:146` **enum** `LineJoin` — CSS `lineJoin` — how line segments connect at corners
`crates/engine/canvas/src/lib.rs:159` **fn** `from_str` — Parse from CSS string
`crates/engine/canvas/src/lib.rs:176` **struct** `DrawState` — All drawing state captured by `save()` and restored by `restore()`
`crates/engine/canvas/src/lib.rs:244` **struct** `ColorStop` — One colour stop in a [`CanvasGradient`]
`crates/engine/canvas/src/lib.rs:253` **enum** `GradientKind` — Gradient kind — stores the defining geometry in user (pre-CTM) space
`crates/engine/canvas/src/lib.rs:267` **struct** `CanvasGradient` — Canvas gradient object (`createLinearGradient` / `createRadialGradient` / `createConicGradient`)
`crates/engine/canvas/src/lib.rs:276` **fn** `linear` — Create a linear gradient from `(x0,y0)` to `(x1,y1)`
`crates/engine/canvas/src/lib.rs:280` **fn** `radial` — Create a radial gradient between two circles
`crates/engine/canvas/src/lib.rs:284` **fn** `conic` — Create a conic gradient starting at `angle` (radians) around `(cx,cy)`
`crates/engine/canvas/src/lib.rs:289` **fn** `add_color_stop` — Add a colour stop at `offset ∈ [0,1]`
`crates/engine/canvas/src/lib.rs:295` **fn** `sample` — Sample the gradient colour at device pixel `(x, y)`
`crates/engine/canvas/src/lib.rs:358` **enum** `RepeatMode` — Pattern repetition mode (`createPattern` second argument)
`crates/engine/canvas/src/lib.rs:374` **struct** `CanvasPattern` — Canvas pattern object (`createPattern`)
`crates/engine/canvas/src/lib.rs:387` **fn** `new` — Create a new pattern from RGBA8 pixel data
`crates/engine/canvas/src/lib.rs:392` **fn** `sample` — Sample the pattern colour at device pixel `(x, y)`
`crates/engine/canvas/src/lib.rs:424` **enum** `PaintSource` — Paint source: a solid colour, a gradient, or a pattern
`crates/engine/canvas/src/lib.rs:439` **fn** `sample` — Sample the paint at device pixel centre `(x + 0.5, y + 0.5)`
`crates/engine/canvas/src/lib.rs:450` **fn** `as_color_or_black` — Return the solid colour, or transparent black if this is a gradient/pattern
`crates/engine/canvas/src/lib.rs:502` **struct** `Context2D` — HTML Canvas 2D rendering context
`crates/engine/canvas/src/lib.rs:572` **fn** `new` — Create a new context with a transparent black buffer and identity CTM
`crates/engine/canvas/src/lib.rs:608` **fn** `set_noise_generator` — Set the optional noise generator for fingerprint randomization
`crates/engine/canvas/src/lib.rs:615` **fn** `get_image_data` — Get a copy of pixel data with optional noise applied (for `getImageData()`)
`crates/engine/canvas/src/lib.rs:628` **fn** `from_pixels` — Create a context pre-filled with the given RGBA8 pixel buffer
`crates/engine/canvas/src/lib.rs:638` **fn** `width` — Canvas width in device pixels
`crates/engine/canvas/src/lib.rs:640` **fn** `height` — Canvas height in device pixels
`crates/engine/canvas/src/lib.rs:643` **fn** `color_space` — Canvas color space (sRGB, Display P3, or Rec2020)
`crates/engine/canvas/src/lib.rs:646` **fn** `set_color_space` — Set the canvas color space for wide-gamut image handling
`crates/engine/canvas/src/lib.rs:649` **fn** `pixels` — Raw RGBA8 pixel data (no noise applied)
`crates/engine/canvas/src/lib.rs:652` **fn** `resize` — Resize the canvas (clears the buffer and resets the CTM to identity)
`crates/engine/canvas/src/lib.rs:663` **fn** `scale_resize` — Resize the canvas by scaling existing pixels to the new dimensions (nearest-neighbour)
`crates/engine/canvas/src/lib.rs:695` **fn** `save` — `save()` — push the current drawing state onto the stack
`crates/engine/canvas/src/lib.rs:720` **fn** `restore` — `restore()` — pop and restore the most recently saved drawing state
`crates/engine/canvas/src/lib.rs:745` **fn** `translate` — `translate(tx, ty)` — apply a translation to the current CTM
`crates/engine/canvas/src/lib.rs:752` **fn** `rotate` — `rotate(angle)` — rotate by `angle` radians clockwise around the origin
`crates/engine/canvas/src/lib.rs:759` **fn** `scale` — `scale(sx, sy)` — apply a uniform or non-uniform scale
`crates/engine/canvas/src/lib.rs:767` **fn** `transform` — `transform(a, b, c, d, e, f)` — post-multiply the CTM by the given matrix
`crates/engine/canvas/src/lib.rs:780` **fn** `set_transform` — `setTransform(a, b, c, d, e, f)` — replace the CTM with the given matrix
`crates/engine/canvas/src/lib.rs:785` **fn** `reset_transform` — `resetTransform()` — reset the CTM to the identity matrix
`crates/engine/canvas/src/lib.rs:803` **fn** `clear_rect` — `clearRect(x, y, w, h)` — erase region to transparent black
`crates/engine/canvas/src/lib.rs:821` **fn** `fill_rect` — `fillRect(x, y, w, h)` — fill region with current `fillStyle`
`crates/engine/canvas/src/lib.rs:831` **fn** `stroke_rect` — `strokeRect(x, y, w, h)` — stroke the outline of a rectangle
`crates/engine/canvas/src/lib.rs:842` **fn** `begin_path` — `beginPath()` — discard current path
`crates/engine/canvas/src/lib.rs:848` **fn** `move_to` — `moveTo(x, y)` — start a new sub-path at user-space `(x, y)`
`crates/engine/canvas/src/lib.rs:856` **fn** `line_to` — `lineTo(x, y)` — add a line segment from pen to `(x, y)`
`crates/engine/canvas/src/lib.rs:868` **fn** `close_path` — `closePath()` — add a line back to the current sub-path start
`crates/engine/canvas/src/lib.rs:879` **fn** `bezier_curve_to` — `bezierCurveTo(cp1x, cp1y, cp2x, cp2y, x, y)` — cubic Bézier from pen
`crates/engine/canvas/src/lib.rs:900` **fn** `quadratic_curve_to` — `quadraticCurveTo(cpx, cpy, x, y)` — quadratic Bézier from pen
`crates/engine/canvas/src/lib.rs:913` **fn** `arc` — `arc(cx, cy, r, startAngle, endAngle[, anticlockwise])` — add circular arc
`crates/engine/canvas/src/lib.rs:936` **fn** `ellipse` — `ellipse(cx, cy, rx, ry, rotation, startAngle, endAngle[, anticlockwise])`
`crates/engine/canvas/src/lib.rs:983` **fn** `arc_to` — `arcTo(x1, y1, x2, y2, radius)` — tangent arc between two lines
`crates/engine/canvas/src/lib.rs:1033` **fn** `rect` — `rect(x, y, w, h)` — add a closed rectangle sub-path
`crates/engine/canvas/src/lib.rs:1042` **fn** `fill` — `fill()` — fill the current path with `fillStyle`
`crates/engine/canvas/src/lib.rs:1055` **fn** `stroke` — `stroke()` — stroke the current path with `strokeStyle`
`crates/engine/canvas/src/lib.rs:1082` **fn** `clip` — `clip()` — intersect the current clipping region with the current path (even-odd rule)
`crates/engine/canvas/src/lib.rs:1099` **fn** `fill_with_path2d` — `fill(path2d)` — fill a `Path2D` object using the current `fillStyle`
`crates/engine/canvas/src/lib.rs:1114` **fn** `stroke_with_path2d` — `stroke(path2d)` — stroke a `Path2D` object using the current `strokeStyle`
`crates/engine/canvas/src/lib.rs:1130` **fn** `clip_with_path2d` — `clip(path2d)` — intersect the clipping region with a `Path2D` object (even-odd rule)
`crates/engine/canvas/src/lib.rs:1145` **fn** `is_point_in_path2d` — `isPointInPath(path2d, x, y)` — test whether `(x, y)` lies inside a `Path2D`
`crates/engine/canvas/src/lib.rs:1164` **fn** `draw_image` — `drawImage(src_pixels, src_w, src_h, dx, dy, dw, dh)` — blit source image onto canvas
`crates/engine/canvas/src/lib.rs:1189` **fn** `draw_image_cropped` — `drawImage(src, sx, sy, sw, sh, dx, dy, dw, dh)` — the 9-argument form with
`crates/engine/canvas/src/lib.rs:1248` **fn** `put_image_data` — `putImageData(data, sw, sh, dx, dy)` — write RGBA8 pixel data directly to canvas
`crates/engine/canvas/src/lib.rs:1271` **fn** `create_image_data` — `createImageData(sw, sh)` — return a zero-filled RGBA8 buffer of `sw × sh` pixels
`crates/engine/canvas/src/lib.rs:1281` **fn** `fill_text_glyphs` — Draw pre-rasterized glyph bitmaps at text position
`crates/engine/canvas/src/path.rs:3` **enum** `PathSegment` — A single segment in a 2D path (HTML Canvas 2D §4.12.4)
`crates/engine/canvas/src/path.rs:16` **type** `PathCommand` — Alias kept for API symmetry with the HTML spec (`PathCommand` = verb)
`crates/engine/canvas/src/path2d.rs:14` **struct** `Path2dData` — A reusable 2D path object independent of any rendering context
`crates/engine/canvas/src/path2d.rs:25` **fn** `new` — Create an empty `Path2D`
`crates/engine/canvas/src/path2d.rs:34` **fn** `from_svg_str` — Parse from an SVG path data string (`M 0 0 L 100 0 Z` etc.)
`crates/engine/canvas/src/path2d.rs:41` **fn** `move_to` — `moveTo(x, y)` — start a new sub-path at `(x, y)`
`crates/engine/canvas/src/path2d.rs:48` **fn** `line_to` — `lineTo(x, y)` — add a straight line from the current pen to `(x, y)`
`crates/engine/canvas/src/path2d.rs:59` **fn** `close_path` — `closePath()` — add a line back to the current sub-path start
`crates/engine/canvas/src/path2d.rs:68` **fn** `bezier_curve_to` — `bezierCurveTo(cp1x, cp1y, cp2x, cp2y, x, y)` — cubic Bézier from pen
`crates/engine/canvas/src/path2d.rs:84` **fn** `quadratic_curve_to` — `quadraticCurveTo(cpx, cpy, x, y)` — quadratic Bézier from pen
`crates/engine/canvas/src/path2d.rs:95` **fn** `arc` — `arc(cx, cy, r, startAngle, endAngle[, ccw])` — circular arc tessellated to lines
`crates/engine/canvas/src/path2d.rs:112` **fn** `arc_to` — `arcTo(x1, y1, x2, y2, radius)` — tangent arc
`crates/engine/canvas/src/path2d.rs:150` **fn** `ellipse` — `ellipse(cx, cy, rx, ry, rotation, startAngle, endAngle[, ccw])` — elliptical arc
`crates/engine/canvas/src/path2d.rs:187` **fn** `rect` — `rect(x, y, w, h)` — add a closed rectangle sub-path
`crates/engine/canvas/src/path2d.rs:198` **fn** `add_path` — `addPath(path[, transform])` — append another path's segments, optionally transformed
`crates/engine/canvas/src/path2d.rs:215` **fn** `to_device_space` — Return segments transformed by a CTM `[a, b, c, d, e, f]`
`crates/engine/canvas/src/rasterize.rs:7` **fn** `fill_path` — Fill `path` using the even-odd scanline algorithm with the given paint source
`crates/engine/canvas/src/rasterize.rs:45` **fn** `stroke_path` — Stroke `path` by drawing each line segment as a thick rectangle
`crates/engine/canvas/src/rasterize.rs:73` **fn** `build_clip_mask` — Build a boolean clip mask by rasterizing `path` with even-odd rule
`crates/engine/canvas/src/rasterize.rs:107` **fn** `collect_lines` — Extract `(x0, y0, x1, y1)` line tuples from `path`, tessellating Bézier curves

## lumen-core  (273 symbols)

`crates/core/src/capability.rs:7` **enum** `Capability`
`crates/core/src/capability.rs:27` **struct** `CapabilityToken`
`crates/core/src/color.rs:4` **enum** `ColorSpace` — Цветовое пространство изображения и canvas
`crates/core/src/color.rs:20` **fn** `name` — Возвращает название пространства как строку (для CSS canvas.colorSpace)
`crates/core/src/color.rs:36` **fn** `detect_color_space_from_icc` — Определяет основное цветовое пространство ICC-профиля
`crates/core/src/crash.rs:65` **struct** `CrashRecorder` — Рекордер событий с кольцевым буфером и дампом при панике
`crates/core/src/crash.rs:79` **fn** `new` — Рекордер с ёмкостью буфера по умолчанию ([`DEFAULT_CAPACITY`]) и без
`crates/core/src/crash.rs:86` **fn** `with_capacity` — Рекордер с заданной ёмкостью буфера и без downstream-sink-а
`crates/core/src/crash.rs:101` **fn** `with_downstream` — Рекордер, форвардящий каждое событие дальше указанному sink-у после
`crates/core/src/crash.rs:111` **fn** `recent_events` — Снимок текущего содержимого буфера в виде готовых строк дампа
`crates/core/src/crash.rs:127` **fn** `total_recorded` — Сколько событий записано всего с момента старта (включая вытесненные
`crates/core/src/crash.rs:142` **fn** `install_panic_hook` — Установить process-global panic-hook, который при панике пишет дамп
`crates/core/src/crash.rs:192` **fn** `format_crash_dump` — Собрать текст crash-дампа из снимка событий и сообщения паники
`crates/core/src/crash.rs:224` **fn** `write_crash_dump` — Записать готовый текст дампа в новый файл `lumen-crash-<unix_ms>.log`
`crates/core/src/error.rs:7` **enum** `Error`
`crates/core/src/error.rs:39` **type** `Result`
`crates/core/src/event.rs:9` **struct** `TabId`
`crates/core/src/event.rs:18` **enum** `RequestStage` — Стадия сетевого запроса, на которой произошёл сбой
`crates/core/src/event.rs:39` **fn** `as_str` — Машинно-читаемый тег стадии для логов и сериализации (`"dns"`/`"tcp"`/
`crates/core/src/event.rs:52` **enum** `SubresourceKind` — Тип subresource-ресурса, найденного preload-сканером
`crates/core/src/event.rs:67` **enum** `FetchPriority` — Приоритет выборки subresource-а. Отражает HTML Living Standard §17.2.3
`crates/core/src/event.rs:79` **fn** `for_kind` — Приоритет по типу subresource (Fetch Standard §2.2)
`crates/core/src/event.rs:91` **enum** `Event`
`crates/core/src/ext.rs:20` **trait** `NetworkTransport` — Сетевой транспорт. Подменяется на mock для тестов или на альтернативный стек
`crates/core/src/ext.rs:40` **trait** `EventSink` — Приёмник событий из подсистем (network, навигация, вкладки)
`crates/core/src/ext.rs:47` **struct** `NoopEventSink` — EventSink, который молча игнорирует все события. Дефолт для подсистем,
`crates/core/src/ext.rs:58` **trait** `StorageBackend` — Хранилище ключ/значение для cookies, истории, кэша
`crates/core/src/ext.rs:90` **trait** `SearchProvider` — Поисковая система для omnibox
`crates/core/src/ext.rs:101` **trait** `FilterListSource` — Источник списка фильтров рекламы / трекеров
`crates/core/src/ext.rs:117` **trait** `RequestFilter` — Решение «блокировать ли исходящий запрос». Реализация смотрит URL и
`crates/core/src/ext.rs:144` **enum** `ResourceType` — Тип ресурса исходящего запроса для EasyList type-опций (`$script`,
`crates/core/src/ext.rs:171` **struct** `RequestContext` — Контекст исходящего запроса, передаваемый в
`crates/core/src/ext.rs:183` **fn** `unknown` — Контекст без информации: оба поля `None`. Заставляет
`crates/core/src/ext.rs:208` **trait** `DnsResolver` — DNS-резолвер: hostname → список IP-адресов (с портом, готовых к connect)
`crates/core/src/ext.rs:233` **trait** `HstsEnforcement` — HSTS-политика: должны ли HTTP-запросы к данному host принудительно
`crates/core/src/ext.rs:257` **enum** `HttpAuthScheme` — HTTP authentication scheme, разрешённый `HttpClient` для re-request
`crates/core/src/ext.rs:268` **fn** `as_str`
`crates/core/src/ext.rs:289` **struct** `HttpAuthChallenge` — Запрос учётных данных от credential-провайдера. Передаётся в
`crates/core/src/ext.rs:302` **struct** `HttpCredentials` — Учётные данные для HTTP auth: username + plaintext password
`crates/core/src/ext.rs:325` **trait** `HttpCredentialProvider` — Поставщик учётных данных HTTP-auth
`crates/core/src/ext.rs:334` **trait** `CookieProvider` — HTTP cookie storage provider. Bridges lumen-network (fetch pipeline) to
`crates/core/src/ext.rs:373` **trait** `EncodingDetector` — Определение кодировки HTML-документа. Для кириллицы критично уметь
`crates/core/src/ext.rs:383` **enum** `FontStyle` — Начертание face-а: `font-style` из CSS Fonts L4. Phase 0 — три
`crates/core/src/ext.rs:392` **fn** `parse_keyword` — Парсит CSS-ключевое слово `normal | italic | oblique` (case-insensitive)
`crates/core/src/ext.rs:414` **struct** `FaceRecord` — Метаданные одного face-а в индексе шрифтов
`crates/core/src/ext.rs:454` **trait** `FontProvider` — Источник системных шрифтов. Реализация — в `lumen-font::system_fonts`
`crates/core/src/ext.rs:508` **fn** `match_face` — CSS Fonts L4 §5.2 алгоритм матчинга — извлечён из trait-а в свободную
`crates/core/src/ext.rs:547` **fn** `match_face_no_stretch` — Legacy функция match_face для backward compatibility (без stretch)
`crates/core/src/ext.rs:847` **trait** `JsRuntime` — JavaScript runtime — исполнение JS-кода (HTML inline scripts, `eval`,
`crates/core/src/ext.rs:913` **struct** `SuspendedHeap` — Serialized JS heap snapshot for T2→T3 hibernation (ADR-008, Invariant 2)
`crates/core/src/ext.rs:920` **fn** `new` — Create a new suspended heap from compressed bytes
`crates/core/src/ext.rs:925` **fn** `len` — Get the size in bytes of the compressed snapshot
`crates/core/src/ext.rs:930` **fn** `is_empty` — Check if the snapshot is empty
`crates/core/src/ext.rs:937` **enum** `JsValue` — Простые JSON-совместимые типы для передачи через trait-границу
`crates/core/src/ext.rs:950` **fn** `object` — Хелпер: построить object из key-value пар
`crates/core/src/ext.rs:958` **fn** `to_json_string` — Сериализовать в JSON-строку (используется automation API — SDC-1a/1b —
`crates/core/src/ext.rs:1009` **enum** `JsError` — Ошибка исполнения JavaScript: либо syntax error (parse), либо runtime
`crates/core/src/ext.rs:1030` **type** `JsResult`
`crates/core/src/ext.rs:1035` **struct** `NullJsRuntime` — Null implementation — всегда возвращает `JsError::NotImplemented`
`crates/core/src/ext.rs:1085` **trait** `UnicodeProvider` — Unicode-таблицы: line break (UAX #14), grapheme/word segmentation
`crates/core/src/ext.rs:1110` **struct** `NullUnicodeProvider` — Null-реализация `UnicodeProvider` — все методы возвращают пустые векторы
`crates/core/src/ext.rs:1138` **trait** `IdnaProvider` — IDN (Internationalized Domain Names) полный UTS #46. Свой Punycode-encoder
`crates/core/src/ext.rs:1148` **struct** `NullIdnaProvider` — Null-реализация `IdnaProvider` — все методы возвращают `None`. Потребитель
`crates/core/src/ext.rs:1173` **trait** `PublicSuffixList` — Public Suffix List — отделение публичных суффиксов от регистрируемых
`crates/core/src/ext.rs:1194` **struct** `NullPublicSuffixList` — Null-реализация `PublicSuffixList` — все запросы возвращают `None`/`false`
`crates/core/src/ext.rs:1220` **trait** `ContentDecoder` — HTTP `Content-Encoding` декодер. Один экземпляр trait-а = один кодек
`crates/core/src/ext.rs:1235` **struct** `UnsupportedContentDecoder` — Stub-реализация `ContentDecoder` для encoding-а, на который нет
`crates/core/src/ext.rs:1266` **trait** `FontFormat` — Декодер альтернативных файловых форматов шрифта (WOFF2, WOFF) в raw
`crates/core/src/ext.rs:1284` **struct** `NullFontFormat` — Null-реализация `FontFormat` — `can_decode` всегда `false`,
`crates/core/src/ext.rs:1309` **trait** `ImageDecoder` — Plug-in декодер растровых изображений для форматов, не встроенных в
`crates/core/src/ext.rs:1336` **trait** `SpellChecker` — Spell checker — проверка орфографии для form field / contenteditable
`crates/core/src/ext.rs:1350` **struct** `NullSpellChecker` — Null-реализация `SpellChecker` — `check` всегда возвращает `true`, чтобы
`crates/core/src/ext.rs:1367` **trait** `HyphenationProvider` — Hyphenation — поиск позиций мягких переносов для CSS `hyphens: auto`
`crates/core/src/ext.rs:1378` **struct** `NullHyphenationProvider` — Null-реализация `HyphenationProvider` — никаких переносов не предлагается
`crates/core/src/ext.rs:1395` **enum** `WsMessage` — Сообщение, полученное от WebSocket-сервера (RFC 6455 §5.6)
`crates/core/src/ext.rs:1411` **trait** `WebSocketSession` — Открытое WebSocket-соединение. Объект владеет TCP/TLS-стримом
`crates/core/src/ext.rs:1431` **trait** `WebSocketProvider` — Фабрика WebSocket-соединений. Реализуется `lumen-network::HttpClient`
`crates/core/src/ext.rs:1449` **struct** `SseEvent` — Полностью разобранное SSE-событие (HTML Living Standard §9.2.6)
`crates/core/src/ext.rs:1465` **trait** `SseSession` — Открытое SSE-соединение (EventSource). Блокирующий интерфейс
`crates/core/src/ext.rs:1495` **trait** `SseProvider` — Фабрика SSE-соединений. Реализуется `lumen-network::HttpClient`
`crates/core/src/ext.rs:1511` **enum** `JsSseEvent` — A single queued event from an SSE connection, ready for delivery to JS
`crates/core/src/ext.rs:1537` **trait** `JsSseSession` — A live SSE connection from the JS runtime's perspective
`crates/core/src/ext.rs:1548` **trait** `JsSseProvider` — Factory that opens SSE connections for the JS runtime
`crates/core/src/ext.rs:1574` **trait** `FetchInterceptor` — Перехватчик fetch-запросов уровня Service Worker
`crates/core/src/ext.rs:1586` **struct** `JsFetchResult` — Full HTTP response for a synchronous JS `fetch()` call
`crates/core/src/ext.rs:1605` **trait** `JsFetchProvider` — Synchronous HTTP fetch bridge for the JS runtime
`crates/core/src/ext.rs:1742` **struct** `AbortToken` — A cheaply-clonable cooperative cancellation flag for aborting in-flight fetches
`crates/core/src/ext.rs:1751` **fn** `new` — Creates a new, non-aborted `AbortToken`
`crates/core/src/ext.rs:1761` **fn** `abort` — Signals abortion by setting the internal flag to `true`
`crates/core/src/ext.rs:1769` **fn** `is_aborted` — Returns whether this token has been aborted
`crates/core/src/ext.rs:1831` **struct** `SseCancel` — An interruptible-delay handle shared across threads
`crates/core/src/ext.rs:1837` **fn** `new` — Creates a new, not-yet-cancelled handle
`crates/core/src/ext.rs:1844` **fn** `signal` — Signals cancellation and wakes any thread parked in [`sleep`](Self::sleep)
`crates/core/src/ext.rs:1852` **fn** `is_cancelled` — Returns whether cancellation has been signalled
`crates/core/src/ext.rs:1861` **fn** `sleep` — Blocks up to `dur`, returning early if cancellation is signalled
`crates/core/src/ext.rs:1930` **trait** `ClipboardProvider` — Synchronous access to the host platform clipboard for the JS runtime
`crates/core/src/ext.rs:1951` **enum** `WebAuthnError` — Failure reason from a [`CredentialProvider`] operation
`crates/core/src/ext.rs:1969` **fn** `dom_exception_name` — The `DOMException` name `lumen-js` should reject the promise with
`crates/core/src/ext.rs:1985` **struct** `WebAuthnCreateRequest` — A WebAuthn credential-creation (registration) request
`crates/core/src/ext.rs:2015` **struct** `WebAuthnCreateResponse` — The result of a successful [`CredentialProvider::create`]
`crates/core/src/ext.rs:2038` **struct** `WebAuthnGetRequest` — A WebAuthn assertion (authentication) request
`crates/core/src/ext.rs:2055` **struct** `WebAuthnGetResponse` — The result of a successful [`CredentialProvider::get`]
`crates/core/src/ext.rs:2085` **trait** `CredentialProvider` — Provider of WebAuthn / passkey credentials, backing `navigator.credentials`
`crates/core/src/ext.rs:2105` **enum** `JsWsEvent` — A single queued event from a WebSocket connection, ready for delivery to JS
`crates/core/src/ext.rs:2135` **trait** `JsWebSocketSession` — A live WebSocket connection from the JS runtime's perspective
`crates/core/src/ext.rs:2154` **trait** `JsWebSocketProvider` — Factory that opens WebSocket connections for the JS runtime
`crates/core/src/ext.rs:2191` **enum** `IdbSchemaOp` — Persistence boundary for the IndexedDB JS shim
`crates/core/src/ext.rs:2254` **enum** `IdbRecordOp` — A record-level operation against one object store, executed within a
`crates/core/src/ext.rs:2331` **enum** `IdbOpResult` — Result of executing a single [`IdbRecordOp`]
`crates/core/src/ext.rs:2342` **trait** `IdbBackend`
`crates/core/src/ext.rs:2399` **trait** `SwBackend` — Per-origin Service Worker registration persistence
`crates/core/src/ext.rs:2421` **trait** `CacheBackend` — Per-origin Cache API persistence (W3C Service Worker spec §cache-objects)
`crates/core/src/ext.rs:2454` **enum** `ClockMode` — Clock mode for deterministic testing (BrowserSession::set_clock, 8F.1)
`crates/core/src/ext.rs:2478` **trait** `BrowserSession` — Browser automation session — unified interface for in-process tests, MCP agents,
`crates/core/src/ext.rs:2613` **struct** `NullBrowserSession` — Null implementation of `BrowserSession` — all methods return `NotImplemented`
`crates/core/src/ext.rs:2722` **enum** `MemoryPressureLevel` — OS memory pressure level (ADR-008, task 10H)
`crates/core/src/ext.rs:2742` **trait** `MemoryPressureSource` — Source of OS memory pressure signals (ADR-008, task 10H)
`crates/core/src/ext.rs:2749` **struct** `NullMemoryPressureSource` — Null implementation — always reports `Low`. For tests and platforms without
`crates/core/src/ext.rs:2771` **trait** `EvictableCache` — Common interface for all cross-tab shared memory caches (ADR-008, task 10D.3)
`crates/core/src/ext.rs:2805` **struct** `CacheRegistry` — Registry of all cross-tab shared memory caches (ADR-008, task 10D.3)
`crates/core/src/ext.rs:2811` **fn** `new` — Create an empty registry
`crates/core/src/ext.rs:2816` **fn** `register` — Register a cache. Caches are notified in registration order
`crates/core/src/ext.rs:2821` **fn** `broadcast_pressure` — Broadcast a memory pressure event to all registered caches
`crates/core/src/ext.rs:2828` **fn** `total_used_bytes` — Total memory currently used across all registered caches, in bytes
`crates/core/src/ext.rs:2836` **fn** `total_budget_bytes` — Total memory budget across all caches with a finite budget, in bytes
`crates/core/src/ext.rs:2845` **fn** `clear_all` — Evict all entries in every registered cache
`crates/core/src/ext.rs:2852` **fn** `len` — Number of registered caches
`crates/core/src/ext.rs:2857` **fn** `is_empty` — `true` if no caches are registered
`crates/core/src/ext.rs:3240` **struct** `KnowledgeHistoryHit` — Result of a full-text history search. Mirrors `lumen_knowledge::SearchHit`
`crates/core/src/ext.rs:3256` **struct** `KnowledgeNoteHit` — Result of a full-text notes search
`crates/core/src/ext.rs:3273` **struct** `KnowledgeReadLaterHit` — Result of a full-text read-later search
`crates/core/src/ext.rs:3288` **struct** `KnowledgeTabHit` — Result of a live open-tabs search
`crates/core/src/ext.rs:3309` **trait** `KnowledgeStore` — Unified knowledge-store interface covering the §12 feature set:
`crates/core/src/ext.rs:3474` **trait** `AiBackend` — Synchronous AI inference backend for the sidebar AI assistant (§12.8)
`crates/core/src/ext.rs:3486` **struct** `NullAiBackend` — Null AI backend — always returns an informational stub
`crates/core/src/ext.rs:3522` **struct** `AudioDeviceDescriptor` — Describes a single audio input or output device available on the host platform
`crates/core/src/ext.rs:3544` **struct** `AudioCaptureConfig` — Constraints forwarded from JS `getUserMedia({audio: {…}})`
`crates/core/src/ext.rs:3561` **enum** `AudioCaptureError` — Errors returned by [`AudioCaptureProvider::capture`]
`crates/core/src/ext.rs:3577` **trait** `AudioCaptureHandle` — Live audio capture stream returned by [`AudioCaptureProvider::capture`]
`crates/core/src/ext.rs:3605` **trait** `AudioCaptureProvider` — Platform audio capture backend backing `navigator.mediaDevices.getUserMedia({audio})`
`crates/core/src/ext.rs:3625` **struct** `NullAudioCaptureProvider` — Stub `AudioCaptureProvider` that returns zero devices and always rejects capture
`crates/core/src/ext.rs:3676` **struct** `ScreenSourceDescriptor` — Describes a capturable screen source (monitor or application window)
`crates/core/src/ext.rs:3693` **struct** `ScreenCaptureConfig` — Constraints forwarded from JS `getDisplayMedia({video: {…}})`
`crates/core/src/ext.rs:3706` **enum** `ScreenCaptureError` — Errors returned by [`ScreenCaptureProvider::capture`]
`crates/core/src/ext.rs:3716` **struct** `VideoFrame` — Single captured video frame (raw RGBA pixels, top-to-bottom row-major)
`crates/core/src/ext.rs:3729` **trait** `ScreenCaptureHandle` — Live screen capture session returned by [`ScreenCaptureProvider::capture`]
`crates/core/src/ext.rs:3754` **trait** `ScreenCaptureProvider` — Platform screen capture backend backing `navigator.mediaDevices.getDisplayMedia`
`crates/core/src/ext.rs:3769` **struct** `NullScreenCaptureProvider` — Stub `ScreenCaptureProvider` that returns zero sources and always rejects capture
`crates/core/src/ext.rs:3824` **trait** `AudioPlaybackProvider` — Platform audio playback backend backing `HTMLAudioElement` (PH3-11)
`crates/core/src/ext.rs:3893` **struct** `NullAudioPlaybackProvider` — Stub `AudioPlaybackProvider` installed when no real audio backend is available
`crates/core/src/ext.rs:3922` **trait** `WakeLockProvider` — Platform provider for Screen Wake Lock API (W3C Screen Wake Lock Level 1)
`crates/core/src/ext.rs:3938` **struct** `NullWakeLockProvider` — Stub provider used in tests and headless mode
`crates/core/src/ext.rs:3955` **trait** `DisplayColorProfile` — Цветовой профиль активного дисплея (OS level)
`crates/core/src/ext.rs:3965` **struct** `NullDisplayColorProfile` — No-op: всегда возвращает `ColorSpace::Srgb`
`crates/core/src/ext.rs:4078` **struct** `SwFetchRequest` — Message sent from the main thread to a Service Worker execution thread
`crates/core/src/ext.rs:4093` **struct** `SwWorkerHandle` — Opaque handle to a running Service Worker execution thread
`crates/core/src/ext.rs:4106` **type** `SwWorkerStore` — Map from `(origin, scope)` to live SW worker handles
`crates/core/src/form.rs:15` **struct** `FormEntry` — Запись формы — пара (name, value) с опциональным filename (для multipart)
`crates/core/src/form.rs:21` **enum** `FormValue`
`crates/core/src/form.rs:33` **fn** `text`
`crates/core/src/form.rs:40` **fn** `file`
`crates/core/src/form.rs:62` **fn** `encode_form_urlencoded` — Сериализует form-set как `application/x-www-form-urlencoded`
`crates/core/src/form.rs:97` **fn** `decode_form_value` — Decode urlencoded form value: `+` → пробел; `%HH` → байт. Не-валидные
`crates/core/src/form.rs:129` **fn** `encode_form_multipart` — Сериализует form-set как `multipart/form-data` (RFC 7578)
`crates/core/src/geom.rs:9` **struct** `Point`
`crates/core/src/geom.rs:23` **struct** `Size`
`crates/core/src/geom.rs:40` **struct** `Rect`
`crates/core/src/geom.rs:73` **fn** `origin`
`crates/core/src/geom.rs:80` **fn** `size`
`crates/core/src/geom.rs:87` **fn** `right`
`crates/core/src/geom.rs:91` **fn** `bottom`
`crates/core/src/hash.rs:30` **fn** `sha256` — SHA-256 хеш произвольных байт по FIPS 180-4
`crates/core/src/hash.rs:122` **fn** `hex_lower` — Закодировать байты в lowercase hex (без префиксов, без separator-ов)
`crates/core/src/hash.rs:135` **fn** `sha256_hex` — `hex_lower(&sha256(input))` — самая частая комбинация (HTTP Digest auth,
`crates/core/src/hash.rs:145` **fn** `sha1` — SHA-1 хеш произвольных байт по FIPS 180-3
`crates/core/src/hash.rs:207` **fn** `base64_encode` — Кодировать байты в Base64 по RFC 4648 §4 (стандартный алфавит, padding '=')
`crates/core/src/hash.rs:228` **fn** `ws_accept_key`
`crates/core/src/icc.rs:25` **enum** `ProfileClass` — Profile/device class (header bytes 12–15)
`crates/core/src/icc.rs:61` **enum** `DataColorSpace` — Colour space of profile data or of the PCS (header bytes 16–19 and 20–23)
`crates/core/src/icc.rs:89` **fn** `channels` — Number of channels for this colour space, or `None` if unknown
`crates/core/src/icc.rs:104` **struct** `XyzNumber` — A tristimulus value in the PCS (parsed from an `XYZType` tag)
`crates/core/src/icc.rs:117` **enum** `ToneCurve` — A tone-reproduction curve (`curveType` `'curv'` or `parametricCurveType` `'para'`)
`crates/core/src/icc.rs:144` **fn** `eval` — Evaluates the tone-reproduction curve at a device-encoded input `x`
`crates/core/src/icc.rs:216` **struct** `IccProfile` — A parsed ICC profile (read-only, owned)
`crates/core/src/icc.rs:251` **fn** `parse` — Parses an ICC profile from raw bytes
`crates/core/src/icc.rs:331` **fn** `color_space` — Maps the profile to one of Lumen's known [`crate::ColorSpace`] variants
`crates/core/src/icc.rs:385` **fn** `build_rgb_transform` — Compiles a matrix-shaper transform from device RGB to gamma-encoded sRGB
`crates/core/src/icc.rs:429` **fn** `build_rgb_transform_to` — Compiles a matrix-shaper transform from device RGB to gamma-encoded
`crates/core/src/icc.rs:478` **fn** `build_cmyk_transform` — Compiles a CMYK→sRGB colour transform from the profile's `A2B0` tag
`crates/core/src/icc.rs:505` **struct** `CmykTransform` — A compiled CMYK→sRGB transform built from a profile's `A2B0` tag
`crates/core/src/icc.rs:517` **fn** `apply` — Transforms one CMYK ink tuple (each channel in `[0, 1]`, `0` = no ink,
`crates/core/src/icc.rs:892` **struct** `RgbTransform` — A compiled RGB matrix-shaper transform: gamma-encoded device RGB → gamma-encoded
`crates/core/src/icc.rs:908` **fn** `apply` — Transforms one gamma-encoded device RGB triple (each in `[0, 1]`) to a
`crates/core/src/icc.rs:961` **fn** `cached_rgb_transform` — Returns the compiled RGB matrix-shaper transform for `profile_bytes`, building
`crates/core/src/icc.rs:983` **fn** `cached_rgb_transform_to` — Returns the compiled RGB matrix-shaper transform for `profile_bytes` targeting
`crates/core/src/icc.rs:1009` **fn** `cached_cmyk_transform` — Returns the compiled CMYK `A2B0` transform for `profile_bytes`, building and
`crates/core/src/idn.rs:24` **fn** `domain_to_ascii` — Преобразует домен в ASCII-форму (IDNA `ToASCII`)
`crates/core/src/idn.rs:53` **fn** `ensure_ascii` — Идемпотентная версия [`domain_to_ascii`] — если вход уже ASCII (например,
`crates/core/src/idn.rs:59` **type** `IdnError` — Ошибка для случаев, когда метка не может быть закодирована. Пока
`crates/core/src/json.rs:15` **enum** `JsonValue`
`crates/core/src/json.rs:27` **fn** `as_str`
`crates/core/src/json.rs:35` **fn** `as_number`
`crates/core/src/json.rs:43` **fn** `as_bool`
`crates/core/src/json.rs:51` **fn** `as_array`
`crates/core/src/json.rs:59` **fn** `as_object`
`crates/core/src/json.rs:67` **fn** `get`
`crates/core/src/json.rs:73` **enum** `JsonError`
`crates/core/src/json.rs:159` **type** `JsonResult`
`crates/core/src/json.rs:161` **fn** `parse`
`crates/core/src/memory_pressure.rs:22` **struct** `Win32MemoryPressureSource` — Win32 memory pressure source via `GlobalMemoryStatusEx` polling
`crates/core/src/memory_pressure.rs:28` **struct** `MemoryStatusEx` — MEMORYSTATUSEX (Windows SDK, winbase.h)
`crates/core/src/memory_pressure.rs:42` **fn** `GlobalMemoryStatusEx`
`crates/core/src/memory_pressure.rs:46` **fn** `memory_load_percent` — Returns memory load as a percentage (0–100), or `None` on API failure
`crates/core/src/memory_pressure.rs:94` **struct** `LinuxMemoryPressureSource` — Linux memory pressure source via `/proc/pressure/memory` PSI polling
`crates/core/src/memory_pressure.rs:143` **struct** `MacosMemoryPressureSource` — macOS memory pressure source via `host_statistics64(HOST_VM_INFO64)` polling
`crates/core/src/memory_pressure.rs:153` **struct** `VmStatistics64` — Subset of `vm_statistics64` from `<mach/vm_statistics.h>` needed for
`crates/core/src/memory_pressure.rs:189` **fn** `mach_host_self` — Returns the mach port for the current host (libSystem, always available)
`crates/core/src/memory_pressure.rs:193` **fn** `host_statistics64` — Fills `host_info_out` with `HOST_VM_INFO64_COUNT` × `u32` words of
`crates/core/src/memory_pressure.rs:202` **fn** `vm_used_total` — Polls VM statistics and returns `(used_pages, total_pages)`, or `None` on error
`crates/core/src/module.rs:9` **trait** `Module`
`crates/core/src/pcs.rs:23` **struct** `Xyz` — A CIE 1931 XYZ tristimulus value
`crates/core/src/pcs.rs:38` **struct** `Lab` — A CIE 1976 L*a*b* value
`crates/core/src/pcs.rs:56` **fn** `new` — Constructs an `Xyz` from raw components
`crates/core/src/pcs.rs:64` **fn** `to_lab` — Converts this XYZ to CIE L*a*b* about the given reference white
`crates/core/src/pcs.rs:83` **fn** `adapt` — Bradford chromatic adaptation of this tristimulus from `src_white` to
`crates/core/src/pcs.rs:89` **fn** `d50_to_d65` — Adapts a tristimulus referenced to D50 (the ICC PCS) into D65
`crates/core/src/pcs.rs:94` **fn** `d65_to_d50` — Adapts a tristimulus referenced to D65 into D50 (the ICC PCS)
`crates/core/src/pcs.rs:101` **fn** `new` — Constructs a `Lab` from raw components
`crates/core/src/pcs.rs:108` **fn** `to_xyz` — Converts this L*a*b* back to CIE XYZ about the given reference white
`crates/core/src/punycode.rs:49` **fn** `encode` — Кодирует Unicode-строку в Punycode согласно RFC 3492
`crates/core/src/sandbox.rs:22` **struct** `SandboxFlags` — Битовое поле sandbox-ограничений. Конкретный бит == «**запрет** этой
`crates/core/src/sandbox.rs:67` **fn** `empty` — Пустой набор — sandbox не активен (без ограничений)
`crates/core/src/sandbox.rs:73` **fn** `all_restrictions` — Все ограничения активны — стартовое состояние для `<iframe sandbox>`
`crates/core/src/sandbox.rs:98` **fn** `contains` — `true` если **все** биты из `other` установлены в `self` —
`crates/core/src/sandbox.rs:104` **fn** `is_empty` — `true` если ни один бит не установлен (sandbox = пустой набор
`crates/core/src/sandbox.rs:109` **fn** `remove` — Снять биты `other` из `self` — используется парсером для `allow-*`
`crates/core/src/sandbox.rs:114` **fn** `insert` — Добавить биты `other`
`crates/core/src/sandbox.rs:119` **fn** `bits` — Удобство для тестов / shell-а: получить сырой битсет
`crates/core/src/sandbox.rs:150` **fn** `parse_sandbox_value` — Парсит значение HTML атрибута `sandbox` в [`SandboxFlags`]
`crates/core/src/spell.rs:12` **enum** `SpellError` — Ошибка загрузки Hunspell-словаря
`crates/core/src/spell.rs:29` **struct** `HunspellDictionary` — Hunspell-словарь (.aff/.dic), развёрнутый в память при загрузке
`crates/core/src/spell.rs:41` **fn** `from_aff_dic` — Разбирает тексты .aff и .dic, разворачивает аффиксные формы в набор слов
`crates/core/src/sri.rs:16` **enum** `SriAlgorithm` — Алгоритм хеширования в SRI metadata
`crates/core/src/sri.rs:23` **fn** `as_str`
`crates/core/src/sri.rs:32` **fn** `digest_size` — Размер digest-а в байтах: SHA-256 → 32, SHA-384 → 48, SHA-512 → 64
`crates/core/src/sri.rs:52` **struct** `SriHash` — Одна запись `integrity` (один алгоритм + ожидаемый digest)
`crates/core/src/sri.rs:61` **struct** `IntegrityList` — Полный `integrity`-список (whitespace-separated). Если список пуст —
`crates/core/src/sri.rs:70` **fn** `parse` — Парсит integrity-атрибут. Whitespace-separated список `algo-base64`
`crates/core/src/sri.rs:85` **fn** `verify` — Проверить body через provider-хешер. Возвращает `Ok(true)` если
`crates/core/src/sri.rs:193` **trait** `DigestProvider` — Trait для подключения hash-implementaции извне
`crates/core/src/sri.rs:200` **enum** `SriError`
`crates/core/src/sri.rs:218` **type** `SriResult`
`crates/core/src/url.rs:23` **struct** `Url`
`crates/core/src/url.rs:36` **fn** `parse` — Распарсить URL. Минимально требуется непустая `scheme:`
`crates/core/src/url.rs:94` **fn** `scheme`
`crates/core/src/url.rs:98` **fn** `host`
`crates/core/src/url.rs:102` **fn** `port`
`crates/core/src/url.rs:106` **fn** `path`
`crates/core/src/url.rs:110` **fn** `query`
`crates/core/src/url.rs:114` **fn** `fragment`
`crates/core/src/url.rs:118` **fn** `as_str`
`crates/core/src/url.rs:123` **fn** `effective_port` — Порт с учётом дефолтов известных схем
`crates/core/src/url.rs:129` **fn** `host_ascii` — Host в ASCII-форме (Punycode) — для DNS, TLS SNI, Host header
`crates/core/src/url.rs:139` **fn** `path_and_query` — Path + `?query` (без fragment) — для HTTP request line
`crates/core/src/url.rs:148` **fn** `resolve` — Разрешить относительный или абсолютный `reference` относительно `self`
`crates/core/src/web_storage.rs:12` **struct** `WebStorage` — In-memory Web Storage partition (localStorage or sessionStorage)
`crates/core/src/web_storage.rs:19` **fn** `len` — Number of stored key-value pairs
`crates/core/src/web_storage.rs:24` **fn** `is_empty` — Returns `true` if the storage contains no items
`crates/core/src/web_storage.rs:29` **fn** `key` — Return the nth key in insertion order, or `None` if out of range
`crates/core/src/web_storage.rs:34` **fn** `get_item` — Return the value for `key`, or `None` if absent
`crates/core/src/web_storage.rs:39` **fn** `set_item` — Set `key` to `value`.  New keys are appended in insertion order
`crates/core/src/web_storage.rs:47` **fn** `remove_item` — Remove `key` and its value.  No-op if absent
`crates/core/src/web_storage.rs:54` **fn** `clear` — Remove all key-value pairs

## lumen-css-parser  (60 symbols)

`crates/engine/css-parser/src/parser.rs:38` **enum** `SimpleSelector`
`crates/engine/css-parser/src/parser.rs:50` **struct** `AttrSelector`
`crates/engine/css-parser/src/parser.rs:61` **enum** `AttrOp`
`crates/engine/css-parser/src/parser.rs:77` **enum** `PseudoClass`
`crates/engine/css-parser/src/parser.rs:345` **enum** `PseudoElementKind` — Pseudo-element селекторы (CSS Pseudo-Elements L4)
`crates/engine/css-parser/src/parser.rs:379` **enum** `DirArg` — Аргумент `:dir(...)` pseudo-class (CSS Selectors L4 §13.2)
`crates/engine/css-parser/src/parser.rs:390` **struct** `RelativeSelector` — Один элемент relative-selector-list-а из `:has()`. `combinator` — если
`crates/engine/css-parser/src/parser.rs:403` **struct** `NthSpec` — Формула `an+b` из CSS Selectors §6.6.5.1. Элемент с 1-based индексом `i`
`crates/engine/css-parser/src/parser.rs:413` **fn** `matches` — Возвращает true, если элемент с 1-based индексом `index` матчит формулу
`crates/engine/css-parser/src/parser.rs:432` **struct** `CompoundSelector`
`crates/engine/css-parser/src/parser.rs:437` **enum** `Combinator`
`crates/engine/css-parser/src/parser.rs:449` **struct** `ComplexSelector`
`crates/engine/css-parser/src/parser.rs:463` **fn** `specificity` — Specificity по CSS Selectors Level 3 §16:
`crates/engine/css-parser/src/parser.rs:483` **fn** `is_supported` — CSS Conditional L4 §4.2 — распознаёт ли движок этот селектор целиком?
`crates/engine/css-parser/src/parser.rs:492` **fn** `to_css_str` — Serialise this selector back to a CSS selector string
`crates/engine/css-parser/src/parser.rs:768` **struct** `Specificity`
`crates/engine/css-parser/src/parser.rs:787` **struct** `Declaration`
`crates/engine/css-parser/src/parser.rs:796` **struct** `Rule`
`crates/engine/css-parser/src/parser.rs:807` **struct** `PropertyRule` — CSS Properties and Values L1 §1.1 — регистрация custom property через
`crates/engine/css-parser/src/parser.rs:815` **struct** `Stylesheet`
`crates/engine/css-parser/src/parser.rs:890` **struct** `FontPaletteValuesRule` — `@font-palette-values --name { font-family: ...; base-palette: N; override-colors: ... }`
`crates/engine/css-parser/src/parser.rs:905` **struct** `ContainerRule` — `@container <name>? <condition> { rules }` — CSS Containment L3 §3
`crates/engine/css-parser/src/parser.rs:918` **struct** `CounterStyleRule` — `@counter-style <name> { ... }` — CSS Counter Styles L3 §2
`crates/engine/css-parser/src/parser.rs:927` **struct** `PageRule` — `@page <selector>? { decls }` — CSS Paged Media L3 §3
`crates/engine/css-parser/src/parser.rs:938` **struct** `ScopeRule` — `@scope (<root>) [to (<limit>)] { rules }` — CSS Cascade L6
`crates/engine/css-parser/src/parser.rs:951` **struct** `StartingStyleRule` — `@starting-style { rules }` — CSS Transitions L2 §3.4. Контейнер
`crates/engine/css-parser/src/parser.rs:957` **struct** `KeyframesRule` — `@keyframes name { offset { decls } ... }` — CSS Animations L1 §3
`crates/engine/css-parser/src/parser.rs:966` **struct** `Keyframe`
`crates/engine/css-parser/src/parser.rs:975` **struct** `SupportsRule` — `@supports <condition> { rules }` блок — CSS Conditional Rules L3 §2
`crates/engine/css-parser/src/parser.rs:994` **enum** `SupportsCondition` — Условие в `@supports (...)`. Грамматика:
`crates/engine/css-parser/src/parser.rs:1048` **fn** `evaluate` — Вычислить условие: вернуть `true`, если потребитель поддерживает
`crates/engine/css-parser/src/parser.rs:1073` **struct** `LayerRule` — `@layer name { rules }` блок
`crates/engine/css-parser/src/parser.rs:1083` **struct** `ImportRule` — `@import` декларация. Per CSS Cascade L4 §6.5 + Media Queries L4:
`crates/engine/css-parser/src/parser.rs:1097` **struct** `FontFaceRule` — `@font-face { font-family: ...; src: url(...) format(...); ... }`
`crates/engine/css-parser/src/parser.rs:1122` **struct** `FontFaceSource`
`crates/engine/css-parser/src/parser.rs:1131` **enum** `FontFaceSourceKind`
`crates/engine/css-parser/src/parser.rs:1140` **struct** `MediaRule` — Группа CSS-правил, вложенных в `@media`-блок
`crates/engine/css-parser/src/parser.rs:1148` **struct** `MediaQuery` — Media query — OR-список AND-clauses (Media Queries L4 §3). Пустой
`crates/engine/css-parser/src/parser.rs:1162` **struct** `MediaQueryClause` — Одна clause в media query — AND-список feature/media-type условий
`crates/engine/css-parser/src/parser.rs:1174` **enum** `MediaCondition`
`crates/engine/css-parser/src/parser.rs:1187` **enum** `MediaFeature`
`crates/engine/css-parser/src/parser.rs:1238` **enum** `MediaOrientation`
`crates/engine/css-parser/src/parser.rs:1245` **enum** `MediaHover` — Media Queries L4 §5.3/§5.5 — hover-способность указателя
`crates/engine/css-parser/src/parser.rs:1254` **enum** `MediaPointer` — Media Queries L4 §5.4/§5.6 — точность указателя
`crates/engine/css-parser/src/parser.rs:1266` **enum** `MediaContrast` — Media Queries L5 §5.5 — `prefers-contrast`: запрошенный пользователем
`crates/engine/css-parser/src/parser.rs:1280` **enum** `MediaReducedData` — Media Queries L5 §5.6 — `prefers-reduced-data`: запрос на экономию
`crates/engine/css-parser/src/parser.rs:1290` **enum** `MediaReducedTransparency` — Media Queries L5 §5.7 — `prefers-reduced-transparency`: запрос на
`crates/engine/css-parser/src/parser.rs:1300` **enum** `MediaScripting` — Media Queries L5 §6.2 — `scripting`: доступность JavaScript в текущем
`crates/engine/css-parser/src/parser.rs:1313` **enum** `MediaInvertedColors` — Media Queries L5 §5.8 — `inverted-colors`: инвертирует ли пользовательское
`crates/engine/css-parser/src/parser.rs:1321` **enum** `ColorScheme`
`crates/engine/css-parser/src/parser.rs:1330` **struct** `MediaContext` — Контекст, против которого матчатся media queries. Заполняется
`crates/engine/css-parser/src/parser.rs:1392` **fn** `matches` — Пустой query (= `@media all`) — true. Иначе хотя бы одна
`crates/engine/css-parser/src/parser.rs:1407` **fn** `matches` — Per Media Queries L4 §3.2: пустая `conditions` — clause invalid
`crates/engine/css-parser/src/parser.rs:1424` **fn** `matches`
`crates/engine/css-parser/src/parser.rs:1434` **fn** `matches`
`crates/engine/css-parser/src/parser.rs:1481` **fn** `parse`
`crates/engine/css-parser/src/parser.rs:1489` **fn** `parse_inline_style` — Парсит содержимое HTML-атрибута `style="..."` — declaration-list без
`crates/engine/css-parser/src/parser.rs:1496` **fn** `parse_selector_list` — Парсит строку CSS selector list (через запятую) и возвращает разобранные
`crates/engine/css-parser/src/parser.rs:1654` **fn** `parse_supports_condition` — Парсит `@supports`-условие из строки между `@supports` и `{`
`crates/engine/css-parser/src/parser.rs:1893` **fn** `parse_media_query` — Распарсить media query из строки между `@media` и `{`. Принимает

## lumen-devtools  (8 symbols)

`crates/devtools/src/cdp.rs:18` **fn** `dispatch` — Обработать одно CDP сообщение, вернуть JSON-строку для отправки клиенту
`crates/devtools/src/server.rs:11` **struct** `DevToolsServer` — Фоновый DevTools сервер. Живёт пока не дропнется (join handle отсоединён)
`crates/devtools/src/server.rs:19` **fn** `spawn` — Запустить сервер на `127.0.0.1:port`. Не блокирует — поток в фоне
`crates/devtools/src/server.rs:28` **fn** `port`
`crates/devtools/src/ws.rs:12` **enum** `WsError`
`crates/devtools/src/ws.rs:42` **fn** `upgrade` — Прочитать HTTP Upgrade запрос, проверить заголовки, отправить 101
`crates/devtools/src/ws.rs:104` **fn** `read_text_frame` — Прочитать один WebSocket фрейм (RFC 6455 §5.2)
`crates/devtools/src/ws.rs:125` **fn** `write_text_frame` — Отправить text фрейм (server→client, без маски)

## lumen-dom  (225 symbols)

`crates/engine/dom/src/contenteditable.rs:10` **enum** `DomCommand` — A single, reversible DOM modification
`crates/engine/dom/src/contenteditable.rs:40` **struct** `PasteData` — Data from a paste operation (clipboard or drag-drop)
`crates/engine/dom/src/contenteditable.rs:54` **struct** `DragData` — Data transferred in a drag-drop operation
`crates/engine/dom/src/contenteditable.rs:69` **fn** `new` — Create empty paste data
`crates/engine/dom/src/contenteditable.rs:74` **fn** `with_text` — Set text content
`crates/engine/dom/src/contenteditable.rs:80` **fn** `with_html` — Set HTML content
`crates/engine/dom/src/contenteditable.rs:86` **fn** `add_file` — Add a file to the paste data
`crates/engine/dom/src/contenteditable.rs:92` **fn** `preferred_content` — Preferred content for insertion: HTML (if available), else plain text
`crates/engine/dom/src/contenteditable.rs:99` **fn** `new` — Create empty drag data
`crates/engine/dom/src/contenteditable.rs:104` **fn** `with_text` — Set text content
`crates/engine/dom/src/contenteditable.rs:110` **fn** `with_html` — Set HTML content
`crates/engine/dom/src/contenteditable.rs:116` **fn** `add_url` — Add a URL to the drag data
`crates/engine/dom/src/contenteditable.rs:122` **fn** `add_file` — Add a file to the drag data
`crates/engine/dom/src/contenteditable.rs:128` **fn** `mark_move` — Mark this as a move operation (not copy)
`crates/engine/dom/src/contenteditable.rs:134` **fn** `preferred_content` — Preferred content for insertion: HTML (if available), else plain text
`crates/engine/dom/src/contenteditable.rs:145` **struct** `CommandHistory` — History of executed commands for undo/redo
`crates/engine/dom/src/contenteditable.rs:156` **fn** `new` — Create an empty history
`crates/engine/dom/src/contenteditable.rs:164` **fn** `insert_text` — Execute InsertText command: insert text at position and record
`crates/engine/dom/src/contenteditable.rs:174` **fn** `delete_range` — Execute DeleteRange command: delete range and record (with deleted text)
`crates/engine/dom/src/contenteditable.rs:192` **fn** `replace_text` — Execute ReplaceText command: replace range with new text and record
`crates/engine/dom/src/contenteditable.rs:217` **fn** `undo` — Undo the last command (move backward in history)
`crates/engine/dom/src/contenteditable.rs:261` **fn** `redo` — Redo the last undone command (move forward in history)
`crates/engine/dom/src/contenteditable.rs:291` **fn** `can_undo` — True if undo is possible
`crates/engine/dom/src/contenteditable.rs:296` **fn** `can_redo` — True if redo is possible
`crates/engine/dom/src/contenteditable.rs:301` **fn** `clear` — Clear all history
`crates/engine/dom/src/contenteditable.rs:307` **fn** `len` — Return the number of commands in history
`crates/engine/dom/src/contenteditable.rs:312` **fn** `is_empty` — True if there are no commands in history
`crates/engine/dom/src/contenteditable.rs:317` **fn** `current_pos` — Return the current position in history (how many commands have been executed/redone)
`crates/engine/dom/src/contenteditable.rs:329` **fn** `paste_into` — Handle paste operation: insert paste data at selection or cursor position
`crates/engine/dom/src/contenteditable.rs:361` **fn** `drop_into` — Handle drop operation: insert drag data at drop position
`crates/engine/dom/src/lib.rs:31` **enum** `ViewportWidth` — Width dimension of a `<meta name=viewport>` tag
`crates/engine/dom/src/lib.rs:43` **struct** `ViewportMeta` — Parsed `<meta name="viewport" content="…">` descriptor
`crates/engine/dom/src/lib.rs:58` **enum** `DomSnapshotError` — Error returned by [`Document::to_bytes`] and [`Document::from_bytes`]
`crates/engine/dom/src/lib.rs:89` **struct** `NodeLimitExceeded` — Returned by [`Document::try_create_element`] when [`MAX_DOM_NODES`] is reached
`crates/engine/dom/src/lib.rs:100` **struct** `NodeId`
`crates/engine/dom/src/lib.rs:103` **fn** `index`
`crates/engine/dom/src/lib.rs:107` **fn** `from_index`
`crates/engine/dom/src/lib.rs:113` **enum** `Namespace`
`crates/engine/dom/src/lib.rs:123` **struct** `QualName`
`crates/engine/dom/src/lib.rs:129` **fn** `html`
`crates/engine/dom/src/lib.rs:138` **struct** `Attribute`
`crates/engine/dom/src/lib.rs:148` **enum** `ShadowRootMode` — Shadow root mode per Shadow DOM spec §4.2
`crates/engine/dom/src/lib.rs:163` **enum** `NodeData`
`crates/engine/dom/src/lib.rs:196` **struct** `Node`
`crates/engine/dom/src/lib.rs:203` **fn** `element_name`
`crates/engine/dom/src/lib.rs:212` **fn** `get_attr` — Возвращает значение атрибута по имени (ASCII case-insensitive). На
`crates/engine/dom/src/lib.rs:228` **fn** `sandbox_flags` — Sandbox-ограничения для `<iframe sandbox="...">` по HTML LS §7.6.5
`crates/engine/dom/src/lib.rs:240` **fn** `input_type` — HTML5 form input type для `<input type="...">`. Возвращает None
`crates/engine/dom/src/lib.rs:254` **fn** `input_mode` — Virtual keyboard hint for `<input inputmode="...">` and `<textarea inputmode="...">`
`crates/engine/dom/src/lib.rs:269` **enum** `InputType` — HTML5 form input types (HTML Standard §4.10.5). Спека определяет
`crates/engine/dom/src/lib.rs:321` **fn** `parse` — Распарсить значение `type`-атрибута. Case-insensitive по
`crates/engine/dom/src/lib.rs:350` **fn** `as_str`
`crates/engine/dom/src/lib.rs:381` **fn** `is_textual` — Текстовая семантика — поле с буквенным контентом, на котором
`crates/engine/dom/src/lib.rs:391` **fn** `is_button_like` — Кнопочная семантика — submit/reset/button/image, рендерится
`crates/engine/dom/src/lib.rs:405` **enum** `InputMode` — HTML Living Standard `inputmode` attribute values — hint to user agent about
`crates/engine/dom/src/lib.rs:427` **fn** `parse` — Parse `inputmode` attribute value. Case-insensitive per HTML spec
`crates/engine/dom/src/lib.rs:440` **fn** `as_str`
`crates/engine/dom/src/lib.rs:456` **struct** `FormInfo` — Данные `<form>` элемента — URL назначения, метод и число полей ввода
`crates/engine/dom/src/lib.rs:472` **enum** `FormSubmitEvent` — Результат попытки отправить форму (HTML5 §4.10.22 form submission algorithm)
`crates/engine/dom/src/lib.rs:498` **enum** `DocumentMode` — Парсинг-режим документа по HTML5 §13.2.6.2 «The insertion mode»
`crates/engine/dom/src/lib.rs:521` **struct** `DomPosition` — A position within the document (WHATWG DOM §4.4)
`crates/engine/dom/src/lib.rs:534` **struct** `Range` — A contiguous range of document content (WHATWG DOM §4.5)
`crates/engine/dom/src/lib.rs:543` **fn** `collapsed` — Collapsed range: both endpoints at `pos`
`crates/engine/dom/src/lib.rs:548` **fn** `is_collapsed` — True when start and end are the same position
`crates/engine/dom/src/lib.rs:560` **struct** `Selection` — The current document text selection (WHATWG Selection API)
`crates/engine/dom/src/lib.rs:569` **fn** `is_collapsed` — True when anchor == focus (or no selection)
`crates/engine/dom/src/lib.rs:578` **fn** `get_range` — The selection as a normalised Range (start ≤ end in node order)
`crates/engine/dom/src/lib.rs:593` **fn** `collapse` — Collapse the selection to a single point
`crates/engine/dom/src/lib.rs:599` **fn** `extend_focus` — Extend the focus end to `pos` (anchor stays fixed)
`crates/engine/dom/src/lib.rs:604` **fn** `clear` — Remove the selection entirely
`crates/engine/dom/src/lib.rs:623` **struct** `CompositionState` — Tracks the current IME composition session
`crates/engine/dom/src/lib.rs:638` **enum** `FontFaceStatus` — The status of a FontFace: whether it's been loaded, is loading, or failed
`crates/engine/dom/src/lib.rs:652` **struct** `FontFace` — Represents a @font-face rule and its loading status
`crates/engine/dom/src/lib.rs:671` **fn** `new` — Create a new FontFace from @font-face rule components
`crates/engine/dom/src/lib.rs:694` **struct** `FontFaceSet` — A collection of FontFace objects representing all @font-face rules in the document
`crates/engine/dom/src/lib.rs:701` **fn** `new` — Create a new empty FontFaceSet
`crates/engine/dom/src/lib.rs:708` **fn** `add` — Add a FontFace to the set
`crates/engine/dom/src/lib.rs:713` **fn** `size` — Get the number of FontFaces in the set
`crates/engine/dom/src/lib.rs:718` **fn** `has_family` — Check if the set contains a FontFace with a specific family name
`crates/engine/dom/src/lib.rs:723` **fn** `get_by_family` — Get all FontFaces with a specific family name
`crates/engine/dom/src/lib.rs:728` **fn** `all` — Get all FontFaces
`crates/engine/dom/src/lib.rs:733` **fn** `clear` — Clear all FontFaces from the set
`crates/engine/dom/src/lib.rs:740` **enum** `PerformanceEntryType` — Type of a performance entry (mark, measure, navigation, resource, etc.)
`crates/engine/dom/src/lib.rs:771` **struct** `PerformanceEntry` — A single performance entry (mark, measure, or resource timing)
`crates/engine/dom/src/lib.rs:784` **fn** `new` — Create a new performance entry
`crates/engine/dom/src/lib.rs:799` **fn** `end_time` — Get the end time of this entry (start_time + duration)
`crates/engine/dom/src/lib.rs:807` **struct** `PerformanceEntries` — Collection of performance entries
`crates/engine/dom/src/lib.rs:814` **fn** `new` — Create a new empty performance entries collection
`crates/engine/dom/src/lib.rs:821` **fn** `add_entry` — Add a performance entry
`crates/engine/dom/src/lib.rs:826` **fn** `all` — Get all performance entries
`crates/engine/dom/src/lib.rs:831` **fn** `get_by_type` — Get entries by type (mark, measure, etc.)
`crates/engine/dom/src/lib.rs:839` **fn** `get_by_name` — Get entries by name
`crates/engine/dom/src/lib.rs:847` **fn** `get_first_by_name` — Get a single entry by name (returns the first match)
`crates/engine/dom/src/lib.rs:852` **fn** `clear` — Clear all performance entries
`crates/engine/dom/src/lib.rs:857` **fn** `len` — Get the count of entries
`crates/engine/dom/src/lib.rs:862` **fn** `is_empty` — Check if the collection is empty
`crates/engine/dom/src/lib.rs:870` **struct** `PerformanceObserver` — Placeholder for PerformanceObserver observer registration
`crates/engine/dom/src/lib.rs:879` **fn** `new` — Create a new PerformanceObserver
`crates/engine/dom/src/lib.rs:887` **fn** `observe` — Add entry types to observe
`crates/engine/dom/src/lib.rs:892` **fn** `disconnect` — Disconnect the observer
`crates/engine/dom/src/lib.rs:898` **fn** `observed_types` — Get the observed entry types
`crates/engine/dom/src/lib.rs:903` **fn** `is_observing` — Check if this observer is watching a specific entry type
`crates/engine/dom/src/lib.rs:908` **fn** `set_handle` — Set the observer handle (assigned by shell runtime when registered)
`crates/engine/dom/src/lib.rs:913` **fn** `handle` — Get the observer handle
`crates/engine/dom/src/lib.rs:925` **struct** `Document`
`crates/engine/dom/src/lib.rs:990` **fn** `new`
`crates/engine/dom/src/lib.rs:1014` **fn** `root`
`crates/engine/dom/src/lib.rs:1022` **fn** `mode` — Текущий парсинг-режим. Tree builder выставляет его при
`crates/engine/dom/src/lib.rs:1028` **fn** `set_mode` — Установить режим. Использует tree builder при инициализации
`crates/engine/dom/src/lib.rs:1033` **fn** `viewport_meta` — Parsed `<meta name="viewport">` descriptor, if the page declared one
`crates/engine/dom/src/lib.rs:1039` **fn** `set_viewport_meta` — Set the viewport meta descriptor. Called by the HTML parser when it
`crates/engine/dom/src/lib.rs:1045` **fn** `get_selection` — Current selection. The shell updates this on mouse events; JS reads it
`crates/engine/dom/src/lib.rs:1050` **fn** `set_selection` — Replace the current selection
`crates/engine/dom/src/lib.rs:1055` **fn** `clear_selection` — Clear the selection
`crates/engine/dom/src/lib.rs:1070` **fn** `target` — Текущий target — id из URL fragment (без ведущего `#`), к которому
`crates/engine/dom/src/lib.rs:1077` **fn** `set_target` — Установить current target (id без `#`). `None` — нет fragment-а в URL
`crates/engine/dom/src/lib.rs:1089` **fn** `attach_shadow` — Attach a shadow root to `host` and return its `NodeId`
`crates/engine/dom/src/lib.rs:1096` **fn** `shadow_root_of` — Return the shadow root attached to `host`, or `None` if not a shadow host
`crates/engine/dom/src/lib.rs:1101` **fn** `is_shadow_host` — Whether `id` is a shadow host (has an attached shadow root)
`crates/engine/dom/src/lib.rs:1105` **fn** `get`
`crates/engine/dom/src/lib.rs:1109` **fn** `get_mut`
`crates/engine/dom/src/lib.rs:1113` **fn** `len`
`crates/engine/dom/src/lib.rs:1117` **fn** `is_empty`
`crates/engine/dom/src/lib.rs:1129` **fn** `base_href` — HTML5 §4.2.3 — найти первый `<base href="...">` в документе и
`crates/engine/dom/src/lib.rs:1140` **fn** `body` — Returns the `<body>` element's `NodeId`, walking root → `<html>` → `<body>`
`crates/engine/dom/src/lib.rs:1152` **fn** `find_first_element` — Найти первый элемент, удовлетворяющий предикату. Pre-order обход
`crates/engine/dom/src/lib.rs:1173` **fn** `find_by_id` — Find a node by its `id` attribute (case-sensitive, per HTML spec)
`crates/engine/dom/src/lib.rs:1201` **fn** `node_count` — Number of nodes currently allocated in this document's arena (including the root)
`crates/engine/dom/src/lib.rs:1207` **fn** `create_element` — Create an element unconditionally. Used by the HTML parser — does **not** enforce
`crates/engine/dom/src/lib.rs:1219` **fn** `try_create_element` — Create an element, returning `Err(`[`NodeLimitExceeded`]`)` if the arena already
`crates/engine/dom/src/lib.rs:1229` **fn** `create_text`
`crates/engine/dom/src/lib.rs:1233` **fn** `create_comment`
`crates/engine/dom/src/lib.rs:1243` **fn** `create_fragment` — Allocate a `DocumentFragment` node in the arena
`crates/engine/dom/src/lib.rs:1251` **fn** `set_template_content` — Register `fragment` as the content container for `template`
`crates/engine/dom/src/lib.rs:1257` **fn** `template_content` — Return the content `DocumentFragment` for a `<template>` element, or
`crates/engine/dom/src/lib.rs:1261` **fn** `create_doctype`
`crates/engine/dom/src/lib.rs:1275` **fn** `append_child` — Append `child` as the last child of `parent`. If `child` already has a parent, it is detached first
`crates/engine/dom/src/lib.rs:1287` **fn** `insert_after` — Insert `new_node` immediately after `reference` in their shared parent
`crates/engine/dom/src/lib.rs:1306` **fn** `detach` — Remove `node` from its current parent. The node itself stays in the arena and can be re-attached
`crates/engine/dom/src/lib.rs:1320` **fn** `insert_before` — Insert `new_node` immediately before `reference` in `reference`'s parent
`crates/engine/dom/src/lib.rs:1340` **fn** `deep_clone` — Deep-clone `node` and (if `deep`) all its descendants
`crates/engine/dom/src/lib.rs:1364` **fn** `acquire_js_ref` — Increment the JS wrapper reference count for `node_id`
`crates/engine/dom/src/lib.rs:1382` **fn** `release_js_ref` — Decrement the JS wrapper reference count for `node_id`
`crates/engine/dom/src/lib.rs:1398` **fn** `js_ref_count` — Returns the number of live JS wrapper objects currently referencing `node_id`
`crates/engine/dom/src/lib.rs:1411` **fn** `is_detached` — Returns `true` if `node_id` is not reachable from the document tree
`crates/engine/dom/src/lib.rs:1438` **fn** `dead_node_ids` — Returns the IDs of all nodes that are safe to collect from the arena
`crates/engine/dom/src/lib.rs:1481` **fn** `begin_composition` — Begin a new IME composition session in the given editable element
`crates/engine/dom/src/lib.rs:1498` **fn** `update_composition` — Update the active composition with new preedit text and selection range
`crates/engine/dom/src/lib.rs:1512` **fn** `end_composition` — End the active composition and return its final state
`crates/engine/dom/src/lib.rs:1522` **fn** `get_composition` — Get the current composition state without removing it
`crates/engine/dom/src/lib.rs:1530` **fn** `is_composing` — Check if an IME composition is currently active
`crates/engine/dom/src/lib.rs:1538` **fn** `get_composition_range` — Get the composition range (offset and length) if composition is active
`crates/engine/dom/src/lib.rs:1546` **fn** `get_composition_target` — Get the target node that is receiving composition input
`crates/engine/dom/src/lib.rs:1552` **fn** `fonts` — Get a reference to the document's FontFaceSet collection
`crates/engine/dom/src/lib.rs:1558` **fn** `fonts_mut` — Get a mutable reference to the document's FontFaceSet collection
`crates/engine/dom/src/lib.rs:1566` **fn** `set_timing_origin` — Set the timing origin (navigation start time in milliseconds since epoch)
`crates/engine/dom/src/lib.rs:1572` **fn** `current_time` — Get the current time relative to timing_origin (milliseconds)
`crates/engine/dom/src/lib.rs:1580` **fn** `mark` — Record a performance mark at the current time
`crates/engine/dom/src/lib.rs:1589` **fn** `measure` — Record a performance measure between two marks
`crates/engine/dom/src/lib.rs:1603` **fn** `performance_entries` — Get a reference to the performance entries collection
`crates/engine/dom/src/lib.rs:1609` **fn** `performance_entries_mut` — Get a mutable reference to the performance entries collection
`crates/engine/dom/src/lib.rs:1614` **fn** `performance_entries_by_type` — Get all performance entries of a specific type
`crates/engine/dom/src/lib.rs:1622` **fn** `performance_entries_by_name` — Get all performance entries with a specific name
`crates/engine/dom/src/lib.rs:1627` **fn** `clear_performance_entries` — Clear all performance entries
`crates/engine/dom/src/lib.rs:1640` **fn** `to_bytes` — Serialise the entire document to a compact binary blob (bincode)
`crates/engine/dom/src/lib.rs:1645` **fn** `from_bytes` — Deserialise a document from a binary blob produced by [`to_bytes`]
`crates/engine/dom/src/lib.rs:1742` **fn** `check_form_gate` — Гейт отправки форм по sandbox-флагу HTML §7.6.5
`crates/engine/dom/src/lib.rs:1763` **fn** `find_ancestor_form` — Найти ближайший предок `<form>` для узла `node`
`crates/engine/dom/src/lib.rs:1780` **fn** `find_ancestor_dialog` — Walk up the DOM from `node` and return the first ancestor `<dialog>` element
`crates/engine/dom/src/lib.rs:1799` **fn** `node_is_contenteditable` — True when `node` carries `contenteditable=""` or `contenteditable="true"`
`crates/engine/dom/src/lib.rs:1814` **fn** `find_editing_host` — Walk up the tree from `node` (inclusive) and return the nearest element
`crates/engine/dom/src/lib.rs:1834` **fn** `is_element_draggable` — Return `true` when `node` is draggable by default HTML5 rules (HTML LS §9.3.3)
`crates/engine/dom/src/lib.rs:1855` **fn** `set_pointer_capture` — Set pointer capture for `pointer_id` to `node` (W3C Pointer Events L3 §4.1)
`crates/engine/dom/src/lib.rs:1863` **fn** `release_pointer_capture` — Release pointer capture for `pointer_id` from `node`
`crates/engine/dom/src/lib.rs:1870` **fn** `has_pointer_capture` — Returns `true` if `node` currently holds pointer capture for `pointer_id`
`crates/engine/dom/src/lib.rs:1878` **fn** `pointer_capture_target` — Returns the element that holds pointer capture for `pointer_id`, if any
`crates/engine/dom/src/lib.rs:1892` **fn** `collect_dom_form_fields` — Собрать имена и значения submittable-контролов формы из DOM-атрибутов
`crates/engine/dom/src/lib.rs:1994` **struct** `ValidityState` — Validity state for a form control — HTML5 §4.10.21.1 `ValidityState` interface
`crates/engine/dom/src/lib.rs:2019` **fn** `valid` — Returns `true` when all flags are `false` (element satisfies all constraints)
`crates/engine/dom/src/lib.rs:2040` **fn** `element_validity` — Returns the validity state for `node`, or `None` if the node is not a
`crates/engine/dom/src/lib.rs:2143` **fn** `check_validity_form` — Returns `true` if all submittable controls in `form_id` satisfy their
`crates/engine/dom/src/lib.rs:2151` **fn** `invalid_controls_in_form` — Returns the `NodeId`s of all invalid (failing constraint validation) controls
`crates/engine/dom/src/lib.rs:2168` **fn** `submit_form` — Execute HTML5 form submission algorithm (§4.10.22 «Form submission»)
`crates/engine/dom/src/lib.rs:2307` **struct** `AnchorInfo` — Информация об якорной ссылке (`<a href>`), найденной в документе
`crates/engine/dom/src/lib.rs:2340` **struct** `FlatTree` — Pre-computed composed tree (flat tree) for Shadow DOM layout traversal
`crates/engine/dom/src/lib.rs:2350` **fn** `children_of` — Composed-tree children of `id`
`crates/engine/dom/src/lib.rs:2365` **fn** `build_flat_tree` — Build the composed (flat) tree for the document
`crates/engine/dom/src/lib.rs:2460` **fn** `check_navigation_gate` — Гейт навигации по sandbox-флагу HTML §7.6.5
`crates/engine/dom/src/lib.rs:2484` **struct** `IframeInfo` — Данные `<iframe>` элемента — URL содержимого и sandbox-ограничения
`crates/engine/dom/src/lib.rs:2540` **fn** `collect_iframes` — Собрать все `<iframe>` элементы документа с их sandbox-ограничениями
`crates/engine/dom/src/lib.rs:2551` **fn** `check_popup_gate` — Гейт открытия popup-ов (`window.open()`, `target="_blank"`) по sandbox HTML §7.6.5
`crates/engine/dom/src/lib.rs:2570` **enum** `EditInputType` — Input event type per Input Events Level 2 §4.1.3
`crates/engine/dom/src/lib.rs:2601` **fn** `as_str` — The canonical `inputType` string for the `InputEvent` interface
`crates/engine/dom/src/lib.rs:2624` **struct** `InputEvent` — Data for a `beforeinput` or `input` DOM event (Input Events Level 2 §4.1)
`crates/engine/dom/src/lib.rs:2643` **fn** `trusted` — Construct a trusted input event (native input pipeline or automation
`crates/engine/dom/src/lib.rs:2654` **fn** `untrusted` — Construct an untrusted input event (synthesized by page script via
`crates/engine/dom/src/lib.rs:2675` **enum** `CompositionEventType` — Type of IME composition event (UI Events §5.2.5)
`crates/engine/dom/src/lib.rs:2686` **fn** `as_str` — The canonical DOM event name per UI Events §5.2.5
`crates/engine/dom/src/lib.rs:2700` **struct** `CompositionData` — Data for a `compositionstart` / `compositionupdate` / `compositionend` event
`crates/engine/dom/src/lib.rs:2727` **struct** `CompositionEvent` — An IME composition event (compositionstart / update / end)
`crates/engine/dom/src/lib.rs:2746` **fn** `new` — Create a new trusted composition event (native IME pipeline)
`crates/engine/dom/src/lib.rs:2758` **fn** `untrusted` — Create an untrusted composition event (synthesized by page script)
`crates/engine/dom/src/lib.rs:2769` **fn** `start` — Create a `compositionstart` event with initial IME text
`crates/engine/dom/src/lib.rs:2784` **fn** `update` — Create a `compositionupdate` event for interim preedit text
`crates/engine/dom/src/lib.rs:2799` **fn** `end` — Create a `compositionend` event for final committed text
`crates/engine/dom/src/lib.rs:2827` **fn** `split_text_node` — Split a text node at `byte_offset`, creating a second text node with the
`crates/engine/dom/src/lib.rs:2869` **fn** `insert_text_at` — Insert `text` into the text node at `pos`, returning the caret position
`crates/engine/dom/src/lib.rs:2927` **fn** `delete_range` — Delete the content of `range` from the document, returning a collapsed
`crates/engine/dom/src/lib.rs:2981` **fn** `insert_paragraph_break`
`crates/engine/dom/src/lib.rs:3010` **fn** `node_text_content` — Returns the full text content of `node` — concatenation of all descendant text nodes
`crates/engine/dom/src/lib.rs:3019` **fn** `node_child_count` — Number of direct DOM children of `node`
`crates/engine/dom/src/lib.rs:3028` **fn** `node_length` — DOM-spec "length" of `node`: UTF-16 code-unit count for text nodes, child
`crates/engine/dom/src/lib.rs:3040` **fn** `range_text` — Extracts the text covered by `range` (WHATWG DOM §4.6 `stringification`)
`crates/engine/dom/src/vtt.rs:7` **struct** `VttCueSettings` — Настройки позиционирования cue (WebVTT §6.3). Phase 0: сырые строки значений
`crates/engine/dom/src/vtt.rs:16` **struct** `VttCue`
`crates/engine/dom/src/vtt.rs:28` **enum** `VttError`
`crates/engine/dom/src/vtt.rs:44` **fn** `parse_vtt` — Разбирает WebVTT-текст в список cues
`crates/engine/dom/src/vtt.rs:212` **enum** `CueTextAlign` — Горизонтальное выравнивание текста внутри cue-бокса
`crates/engine/dom/src/vtt.rs:223` **struct** `CueBox` — Разрешённый бокс cue поверх видео
`crates/engine/dom/src/vtt.rs:235` **fn** `active_cues` — Cues, активные в момент `t` (секунды): `start_s <= t < end_s`. Исходный порядок сохраняется
`crates/engine/dom/src/vtt.rs:242` **fn** `strip_cue_markup` — Убирает WebVTT-разметку из текста cue: теги (`<v Имя>`, `</v>`, `<b>`, `<i>`, `<c.class>`,
`crates/engine/dom/src/vtt.rs:319` **fn** `resolve_cue_box` — Раскладывает cue-бокс в координатах видео-бокса
`crates/engine/dom/src/vtt.rs:379` **struct** `TrackInfo` — Информация о track-е медиа
`crates/engine/dom/src/vtt.rs:391` **struct** `VideoTracks` — Сбор track-ов для всех элементов <video>
`crates/engine/dom/src/vtt.rs:398` **fn** `collect_video_tracks` — Рекурсивно обходит документ и собирает <video> с их <track>

## lumen-driver  (99 symbols)

`crates/driver/src/automation.rs:24` **type** `AutomationRequest` — One outstanding request to the live shell window: a command plus the
`crates/driver/src/automation.rs:33` **type** `WakeFn` — A callback that interrupts a parked (`winit::event_loop::ControlFlow::Wait`)
`crates/driver/src/automation.rs:45` **struct** `AutomationHandle` — Thread-safe, cloneable handle for sending [`AutomationCommand`]s to a live
`crates/driver/src/automation.rs:57` **fn** `new` — Wrap the sending half of a shell's automation channel. No wake
`crates/driver/src/automation.rs:63` **fn** `set_wake` — Attach (or replace) the event-loop wake callback. Visible immediately
`crates/driver/src/automation.rs:74` **fn** `execute` — Send `command` to the live window and block for its reply, up to `timeout`
`crates/driver/src/context.rs:22` **struct** `SessionContext` — Isolated context for a single BrowserSession
`crates/driver/src/context.rs:45` **fn** `new` — Create a new context with default (Standard) fingerprint profile and real system clock
`crates/driver/src/context.rs:60` **fn** `with_fingerprint_profile` — Create a context with a specific fingerprint profile and real system clock
`crates/driver/src/context.rs:74` **fn** `fingerprint_profile`
`crates/driver/src/context.rs:78` **fn** `set_fingerprint_profile`
`crates/driver/src/context.rs:88` **fn** `user_agent`
`crates/driver/src/context.rs:94` **fn** `set_user_agent`
`crates/driver/src/context.rs:104` **fn** `clear_user_agent_override`
`crates/driver/src/context.rs:109` **fn** `clock_mode` — Returns the active clock mode
`crates/driver/src/context.rs:118` **fn** `set_clock_mode` — Set clock mode for `Date.now()` / `performance.now()` overrides (8F.1)
`crates/driver/src/context.rs:128` **fn** `read_clock_ms` — Read the current clock value in ms, advancing the monotonic counter if active
`crates/driver/src/context.rs:141` **fn** `frozen_clock_ms` — Convenience: returns `Some(ms)` only when clock is frozen (backward-compat)
`crates/driver/src/context.rs:149` **fn** `set_frozen_clock` — Set frozen clock (backward-compat wrapper; use `set_clock_mode` for new code)
`crates/driver/src/context.rs:154` **fn** `clear_frozen_clock` — Restore system clock (backward-compat wrapper; use `set_clock_mode` for new code)
`crates/driver/src/context.rs:159` **fn** `rng_seed` — Get RNG seed for deterministic randomness, or None if OS entropy is used
`crates/driver/src/context.rs:165` **fn** `set_rng_seed` — Set RNG seed for deterministic random numbers in JS Math.random() and crypto.getRandomValues()
`crates/driver/src/context.rs:170` **fn** `clear_rng_seed` — Clear RNG seed; resume using OS entropy
`crates/driver/src/context.rs:175` **fn** `is_fingerprint_frozen` — Check if fingerprint profile is frozen (cannot be changed)
`crates/driver/src/context.rs:181` **fn** `freeze_fingerprint` — Freeze current fingerprint profile: prevent further changes to set_fingerprint_profile()
`crates/driver/src/context.rs:186` **fn** `unfreeze_fingerprint` — Unfreeze fingerprint profile; allow changes again
`crates/driver/src/context.rs:190` **fn** `get_cookies_for_request`
`crates/driver/src/context.rs:195` **fn** `process_set_cookie`
`crates/driver/src/context.rs:202` **fn** `clear_cookies`
`crates/driver/src/context.rs:206` **fn** `get_storage`
`crates/driver/src/context.rs:212` **fn** `set_storage`
`crates/driver/src/context.rs:219` **fn** `clear_origin_storage`
`crates/driver/src/context.rs:223` **fn** `clear_all_storage`
`crates/driver/src/context.rs:227` **fn** `storage_keys`
`crates/driver/src/context.rs:234` **fn** `get_cached_response`
`crates/driver/src/context.rs:238` **fn** `cache_response`
`crates/driver/src/context.rs:242` **fn** `clear_http_cache`
`crates/driver/src/determinism.rs:39` **struct** `DeterministicConfig` — Configuration bundle for enabling deterministic mode on a `BrowserSession`
`crates/driver/src/determinism.rs:65` **fn** `with_seed` — Convenience constructor: fully deterministic mode with a specific RNG seed
`crates/driver/src/determinism.rs:77` **fn** `for_snapshot` — Convenience constructor for snapshot testing
`crates/driver/src/determinism.rs:89` **fn** `apply` — Apply this configuration to `session`
`crates/driver/src/determinism.rs:103` **fn** `seed_from_url` — Returns a deterministic u64 seed derived from a URL string
`crates/driver/src/gpu_session.rs:21` **struct** `RenderedPage` — Rendered page result from GpuSession rendering operations
`crates/driver/src/gpu_session.rs:53` **struct** `JsNavigateRequest` — Navigation request initiated by JS code (location.href=, history.pushState, etc)
`crates/driver/src/gpu_session.rs:64` **trait** `GpuSession` — Extended `BrowserSession` trait for GPU and streaming operations
`crates/driver/src/isolation.rs:40` **struct** `OriginGroup` — eTLD+1 site identifier used to group related origins
`crates/driver/src/isolation.rs:53` **fn** `for_origin` — Derive the origin group from a full origin URL or host string
`crates/driver/src/isolation.rs:70` **struct** `OriginIsolationContext` — Per-origin-group isolation container
`crates/driver/src/isolation.rs:89` **fn** `new` — Create a new isolation context for the given origin (URL or host string)
`crates/driver/src/isolation.rs:107` **fn** `site` — The site identifier (eTLD+1) of this context's origin group
`crates/driver/src/isolation.rs:115` **fn** `local_storage_for` — Get (or create) the `localStorage` partition for `origin`
`crates/driver/src/isolation.rs:126` **fn** `session_storage_for` — Get (or create) the `sessionStorage` partition for `origin`
`crates/driver/src/isolation.rs:134` **fn** `clear_session_storage_for` — Clear `sessionStorage` for `origin` (spec: cleared on top-level navigation)
`crates/driver/src/isolation.rs:139` **fn** `clear_all_session_storage` — Clear all `sessionStorage` partitions in this context
`crates/driver/src/isolation.rs:148` **fn** `idb_store_for` — Create an `IdbStore` scoped to `origin` using this context's backend
`crates/driver/src/isolation.rs:153` **fn** `idb_save` — Save an IndexedDB JSON snapshot for `origin`
`crates/driver/src/isolation.rs:158` **fn** `idb_load` — Load the IndexedDB JSON snapshot for `origin`, or `None` if absent
`crates/driver/src/isolation.rs:166` **fn** `cookie_jar` — Shared `Arc<CookieJar>` for this origin group
`crates/driver/src/isolation.rs:171` **fn** `same_group` — Check whether two origins belong to the same origin group (same eTLD+1)
`crates/driver/src/lib.rs:66` **trait** `BrowserSession` — Программный интерфейс к браузерному сеансу
`crates/driver/src/live_session.rs:42` **struct** `LiveWindowSession` — [`BrowserSession`] adapter that drives a live `lumen-shell` window through
`crates/driver/src/live_session.rs:50` **fn** `new` — Bind a new session to `handle`, the sending half of a live window's
`crates/driver/src/session.rs:53` **struct** `InProcessSession` — Headless in-process сессия браузера
`crates/driver/src/session.rs:91` **fn** `new` — Создать сессию с viewport 1024×720
`crates/driver/src/session.rs:107` **fn** `with_viewport` — Создать сессию с заданным размером viewport (логические пиксели)
`crates/driver/src/session.rs:139` **fn** `with_origin_isolation` — Create a session with per-origin-group isolation (Phase 1: 8E)
`crates/driver/src/session.rs:158` **fn** `isolation_context` — Access the per-origin-group isolation context, if this session was
`crates/driver/src/session.rs:163` **fn** `isolation_context_mut` — Mutable access to the per-origin-group isolation context
`crates/driver/src/session.rs:173` **fn** `set_pending_js_tasks` — Установить количество pending JS microtask/callback для условия `JsIdle`
`crates/driver/src/session.rs:204` **fn** `active_property_trees` — Active property trees snapshot from the compositor (PH1-7)
`crates/driver/src/session.rs:216` **fn** `scroll_page_by` — Off-main-thread page scroll (PH1-7)
`crates/driver/src/session.rs:236` **fn** `navigate_html` — Загрузить HTML-строку без навигации по URL. Используется для тестов
`crates/driver/src/session.rs:312` **fn** `screenshot_cpu_rgba` — Детерминированный CPU-рендер текущей страницы в RGBA8 (tiny-skia)
`crates/driver/src/session.rs:330` **fn** `screenshot_cpu_png` — Детерминированный CPU-рендер текущей страницы в PNG (tiny-skia)
`crates/driver/src/session.rs:344` **fn** `display_list_for_compare` — Строит [`lumen_paint::DisplayList`] из текущего состояния страницы
`crates/driver/src/session.rs:1122` **fn** `computed_style_json` — Возвращает полный набор computed-style свойств первого элемента,
`crates/driver/src/types.rs:15` **struct** `NodeRef` — Ссылка на DOM-узел, возвращаемая [`BrowserSession::query`]
`crates/driver/src/types.rs:30` **enum** `Target` — Цель для команд [`BrowserSession::click`], [`type_text`](BrowserSession::type_text),
`crates/driver/src/types.rs:41` **struct** `ScrollDelta` — Дельта скролла для [`BrowserSession::scroll`]
`crates/driver/src/types.rs:50` **enum** `WaitCondition` — Условие ожидания для [`BrowserSession::wait`]
`crates/driver/src/types.rs:65` **struct** `BoxModel` — Box-model одного узла из [`BrowserSession::layout_snapshot`]
`crates/driver/src/types.rs:82` **struct** `A11yState` — ARIA state flags for an accessibility node, derived from `lumen-a11y::AXState`
`crates/driver/src/types.rs:112` **struct** `A11yNode` — Узел accessibility-дерева из [`BrowserSession::a11y_tree`]
`crates/driver/src/types.rs:136` **struct** `NetworkEntry` — Запись из сетевого лога [`BrowserSession::network_log`]
`crates/driver/src/types.rs:149` **struct** `ConsoleEntry` — Запись из консоли [`BrowserSession::console_log`]
`crates/driver/src/types.rs:158` **enum** `ConsoleLevel` — Уровень console-сообщения
`crates/driver/src/types.rs:170` **struct** `ComputedProperties` — Значения вычисленных CSS-свойств элемента из [`BrowserSession::computed_style`]
`crates/driver/src/types.rs:185` **enum** `InputCommand` — Команда для injection в event-loop браузера с целью создания нативных DOM-событий
`crates/driver/src/types.rs:239` **enum** `AxQuery` — Запрос к accessibility-дереву для [`BrowserSession::query_a11y`] и [`query_a11y_all`](BrowserSession::query_a11y_all)
`crates/driver/src/types.rs:275` **enum** `FingerprintProfile` — Профиль отпечатка браузера (fingerprint profile) для BrowserSession
`crates/driver/src/types.rs:297` **fn** `to_http_profile` — Map this session-level profile to the network [`HttpProfile`] that drives
`crates/driver/src/types.rs:312` **enum** `AutomationCommand` — Command for automation API — sent to shell via IPC channel (SDC-1a)
`crates/driver/src/types.rs:335` **enum** `AutomationReply` — Reply from automation API — returned from shell after command execution
`crates/driver/src/winit_session.rs:66` **struct** `WinitSession` — Оконная сессия браузера
`crates/driver/src/winit_session.rs:93` **fn** `new` — Создать сессию с viewport 1024×720
`crates/driver/src/winit_session.rs:108` **fn** `with_viewport` — Создать сессию с заданным размером viewport (логические пиксели)
`crates/driver/src/winit_session.rs:134` **fn** `active_property_trees` — Active property trees snapshot from the threaded compositor (PH1-7)
`crates/driver/src/winit_session.rs:142` **fn** `scroll_page_by` — Off-main-thread page scroll via the threaded compositor (PH1-7)
`crates/driver/src/winit_session.rs:222` **fn** `navigate_html` — Load HTML string without URL navigation. Used in tests (headless mode)

## lumen-encoding  (13 symbols)

`crates/engine/encoding/src/decoder.rs:14` **fn** `decode` — Декодирует байты в строку. Алиас для [`decode_to_string`], короткий и
`crates/engine/encoding/src/decoder.rs:21` **fn** `decode_to_string` — То же, что [`decode`], но с явным именем — для случаев, когда из
`crates/engine/encoding/src/detect.rs:16` **fn** `detect` — Главная точка входа. Возвращает кодировку, в которой следует декодировать
`crates/engine/encoding/src/detect.rs:99` **fn** `sniff_meta_charset` — Ищет `<meta charset>` или `<meta http-equiv="Content-Type" content="...; charset=X">`
`crates/engine/encoding/src/ext_impl.rs:17` **struct** `HeuristicDetector` — Детектор кодировок по умолчанию
`crates/engine/encoding/src/hyphenation_impl.rs:18` **struct** `KnuthLiangHyphenation` — Knuth–Liang hyphenation with per-locale lazy-loaded embedded dictionaries
`crates/engine/encoding/src/hyphenation_impl.rs:24` **fn** `new` — Create a new provider with an empty cache
`crates/engine/encoding/src/lib.rs:41` **enum** `Encoding` — Поддерживаемые в Phase 0 кодировки
`crates/engine/encoding/src/lib.rs:59` **fn** `name` — Стабильное имя кодировки. Используется в API детектора
`crates/engine/encoding/src/lib.rs:79` **fn** `from_label` — Парсит label кодировки (case-insensitive, с алиасами)
`crates/engine/encoding/src/unicode_provider.rs:23` **struct** `Icu4xUnicodeProvider` — ICU4x-провайдер Unicode-операций
`crates/engine/encoding/src/unicode_provider.rs:31` **fn** `new` — Создаёт провайдер с auto-режимом (LSTM/dictionary для CJK/Thai/etc)
`crates/engine/encoding/src/unicode_provider.rs:40` **fn** `new_latin` — Облегчённая версия — только Latin + UAX #14 rules, без LSTM

## lumen-font  (220 symbols)

`crates/engine/font/src/avar.rs:32` **struct** `AxisValueMap` — Одна пара (fromCoord → toCoord) в segment map оси. Координаты в
`crates/engine/font/src/avar.rs:44` **struct** `SegmentMap` — Segment map для одной оси: список пар, отсортированных по `from`
`crates/engine/font/src/avar.rs:55` **fn** `normalize` — Применяет piecewise-linear перенормализацию: ищет сегмент, в
`crates/engine/font/src/avar.rs:89` **struct** `Avar`
`crates/engine/font/src/avar.rs:97` **fn** `parse`
`crates/engine/font/src/avar.rs:131` **fn** `normalize` — Перенормализация для axis под индексом `axis_index`. `coord`
`crates/engine/font/src/binary.rs:8` **struct** `BinaryReader`
`crates/engine/font/src/binary.rs:14` **fn** `new`
`crates/engine/font/src/binary.rs:18` **fn** `position`
`crates/engine/font/src/binary.rs:22` **fn** `seek`
`crates/engine/font/src/binary.rs:26` **fn** `remaining`
`crates/engine/font/src/binary.rs:30` **fn** `skip`
`crates/engine/font/src/binary.rs:39` **fn** `read_bytes`
`crates/engine/font/src/binary.rs:46` **fn** `read_u8`
`crates/engine/font/src/binary.rs:52` **fn** `read_u16`
`crates/engine/font/src/binary.rs:57` **fn** `read_u32`
`crates/engine/font/src/binary.rs:62` **fn** `read_i16`
`crates/engine/font/src/binary.rs:67` **fn** `read_i32`
`crates/engine/font/src/binary.rs:73` **fn** `read_tag` — 4-байтовый ASCII-тег (например, `b"head"`, `b"glyf"`)
`crates/engine/font/src/cff.rs:298` **struct** `Cff` — Parsed `CFF ` table ready to produce glyph outlines
`crates/engine/font/src/cff.rs:306` **fn** `num_glyphs` — Number of glyphs (CharStrings INDEX count)
`crates/engine/font/src/cff.rs:311` **fn** `parse` — Parse a `CFF ` table from its raw bytes
`crates/engine/font/src/cff.rs:390` **fn** `glyph` — Glyph outline for `glyph_id`, or `None` if the glyph is empty (e.g
`crates/engine/font/src/cmap.rs:21` **struct** `Cmap`
`crates/engine/font/src/cmap.rs:31` **fn** `parse`
`crates/engine/font/src/cmap.rs:94` **fn** `glyph_index` — Возвращает glyph index для codepoint, либо `None` если не отображён
`crates/engine/font/src/delta_set_index_map.rs:30` **struct** `DeltaSetIndex` — Распакованный entry: пара индексов для lookup в `ItemVariationStore`
`crates/engine/font/src/delta_set_index_map.rs:36` **struct** `DeltaSetIndexMap`
`crates/engine/font/src/delta_set_index_map.rs:44` **fn** `parse`
`crates/engine/font/src/delta_set_index_map.rs:90` **fn** `get` — Возвращает `(outer, inner)` для glyph_id (или другого входного
`crates/engine/font/src/face.rs:11` **struct** `OffsetTable` — Заголовок TTF/OTF файла. Указывает, сколько таблиц в шрифте
`crates/engine/font/src/face.rs:27` **fn** `read`
`crates/engine/font/src/face.rs:40` **struct** `TableRecord` — Запись в каталоге таблиц: где в файле лежит конкретная таблица
`crates/engine/font/src/face.rs:48` **fn** `read`
`crates/engine/font/src/face.rs:59` **enum** `FontError`
`crates/engine/font/src/face.rs:91` **struct** `Font` — Распарсенный шрифт: каталог таблиц + ссылка на оригинальные байты
`crates/engine/font/src/face.rs:98` **fn** `parse`
`crates/engine/font/src/face.rs:118` **fn** `offset_table`
`crates/engine/font/src/face.rs:122` **fn** `tables`
`crates/engine/font/src/face.rs:128` **fn** `table` — Возвращает байты таблицы по 4-байтовому тегу, либо `None`,
`crates/engine/font/src/face.rs:135` **fn** `head`
`crates/engine/font/src/face.rs:140` **fn** `maxp`
`crates/engine/font/src/face.rs:145` **fn** `cmap`
`crates/engine/font/src/face.rs:150` **fn** `hhea`
`crates/engine/font/src/face.rs:155` **fn** `hmtx`
`crates/engine/font/src/face.rs:162` **fn** `loca`
`crates/engine/font/src/face.rs:169` **fn** `glyf`
`crates/engine/font/src/face.rs:179` **fn** `cff` — `CFF ` — Compact Font Format (PostScript Type 2 outlines). Present in
`crates/engine/font/src/face.rs:186` **fn** `has_cff` — `true` if the font stores outlines in a `CFF ` table (PostScript) rather
`crates/engine/font/src/face.rs:190` **fn** `name`
`crates/engine/font/src/face.rs:195` **fn** `os2`
`crates/engine/font/src/face.rs:207` **fn** `post` — `post` — PostScript Information Table. Содержит italic angle и
`crates/engine/font/src/face.rs:217` **fn** `fvar` — `fvar` (Font Variations) — описание variation axes (wght / wdth / slnt /
`crates/engine/font/src/face.rs:228` **fn** `avar` — `avar` (Axis Variations) — piecewise-linear перенормализация осей из
`crates/engine/font/src/face.rs:242` **fn** `gvar` — `gvar` (Glyph Variations) — per-glyph variation deltas для outline
`crates/engine/font/src/face.rs:254` **fn** `hvar` — `HVAR` (Horizontal Metrics Variations) — variation deltas для
`crates/engine/font/src/face.rs:268` **fn** `advance_width_varied` — Advance width for `glyph_id` with HVAR variation deltas applied
`crates/engine/font/src/face.rs:292` **fn** `vvar` — `VVAR` (Vertical Metrics Variations) — зеркало `HVAR` для
`crates/engine/font/src/face.rs:309` **fn** `mvar` — `MVAR` (Metrics Variations) — variation deltas для глобальных
`crates/engine/font/src/face.rs:318` **fn** `glyph` — Удобная обёртка: glyph_id → outline. `None`, если глиф пустой
`crates/engine/font/src/face.rs:337` **fn** `glyph_resolved` — Возвращает глиф с рекурсивно развёрнутыми composite-компонентами:
`crates/engine/font/src/face.rs:369` **fn** `glyph_resolved_with_coords` — Variable-fonts вариант [`Font::glyph_resolved`]: применяет gvar deltas
`crates/engine/font/src/font_registry.rs:19` **struct** `FontRegistry` — Провайдер шрифтов с поддержкой @font-face: системные шрифты + URL-буферы
`crates/engine/font/src/font_registry.rs:28` **fn** `new`
`crates/engine/font/src/font_registry.rs:38` **fn** `with_dirs` — Registry backed by a custom-dir `SystemFontIndex` — for tests and
`crates/engine/font/src/font_registry.rs:52` **fn** `register_from_bytes` — Регистрирует шрифт из байт-буфера (TrueType / sfnt после декодирования
`crates/engine/font/src/font_registry.rs:88` **fn** `custom_face_count` — Количество зарегистрированных @font-face face-ов. Для тестов
`crates/engine/font/src/font_registry.rs:99` **fn** `resolve_local_bytes` — Resolves a `local()` @font-face source by matching the name against the system
`crates/engine/font/src/font_registry.rs:108` **fn** `face_bytes_for_family` — Возвращает байты первого загруженного face для данной семьи
`crates/engine/font/src/fvar.rs:25` **struct** `VariationAxis` — Одна variation axis. Все значения в native axis units (не CSS-нормализо-
`crates/engine/font/src/fvar.rs:53` **fn** `is_hidden`
`crates/engine/font/src/fvar.rs:60` **fn** `clamp` — Зажать значение в `[min, max]`. Полезно при подаче CSS-уровневого
`crates/engine/font/src/fvar.rs:76` **struct** `NamedInstance` — Одна named instance — фиксированная точка в пространстве variation axes,
`crates/engine/font/src/fvar.rs:95` **struct** `Fvar` — Все axes и instances из `fvar`. Порядок — как в таблице (важно: координаты
`crates/engine/font/src/fvar.rs:101` **fn** `parse`
`crates/engine/font/src/fvar.rs:224` **fn** `axis` — Найти axis по tag-у. Возвращает `None`, если в шрифте нет такой
`crates/engine/font/src/fvar.rs:232` **fn** `is_variable` — `true`, если шрифт имеет хотя бы одну variation axis. Для non-variable
`crates/engine/font/src/fvar.rs:240` **fn** `instance_by_name_id` — Найти named instance с указанным `subfamily_name_id`. Возвращает
`crates/engine/font/src/glyf.rs:25` **struct** `BoundingBox`
`crates/engine/font/src/glyf.rs:33` **struct** `OutlinePoint`
`crates/engine/font/src/glyf.rs:40` **struct** `Contour`
`crates/engine/font/src/glyf.rs:45` **enum** `Outline`
`crates/engine/font/src/glyf.rs:65` **enum** `Anchor` — Как компонент привязывается к parent-у
`crates/engine/font/src/glyf.rs:79` **struct** `CompositeComponent` — Один компонент composite-глифа: ссылка на другой глиф + 2×2 матрица + anchor
`crates/engine/font/src/glyf.rs:86` **struct** `Glyph`
`crates/engine/font/src/glyf.rs:92` **fn** `parse`
`crates/engine/font/src/glyf.rs:286` **struct** `Glyf` — Удобный view над байтами `glyf` для разбора глифа по offset/length из loca
`crates/engine/font/src/glyf.rs:291` **fn** `new`
`crates/engine/font/src/glyf.rs:295` **fn** `glyph_at`
`crates/engine/font/src/gpos.rs:32` **struct** `Gpos` — Parsed `GPOS` table plus the lookup indices activated by the enabled
`crates/engine/font/src/gpos.rs:40` **fn** `parse` — Parse the `GPOS` table bytes and pre-select the lookups for the
`crates/engine/font/src/gpos.rs:48` **fn** `parse_with_features` — Like [`Gpos::parse`], but with CSS `font-feature-settings` overrides
`crates/engine/font/src/gpos.rs:56` **fn** `has_lookups` — Whether any positioning lookups are active
`crates/engine/font/src/gpos.rs:62` **fn** `apply` — Apply all enabled positioning lookups to `glyphs` in order. Advances
`crates/engine/font/src/gsub.rs:43` **struct** `Gsub` — Parsed `GSUB` table plus the lookup indices activated by the enabled
`crates/engine/font/src/gsub.rs:52` **fn** `parse` — Parse the `GSUB` table bytes and pre-select the lookups for the
`crates/engine/font/src/gsub.rs:60` **fn** `parse_with_features` — Like [`Gsub::parse`], but with CSS `font-feature-settings` overrides
`crates/engine/font/src/gsub.rs:68` **fn** `has_lookups` — Whether any substitution lookups are active
`crates/engine/font/src/gsub.rs:73` **fn** `apply` — Apply all enabled substitution lookups to `glyphs` in order
`crates/engine/font/src/gvar.rs:47` **enum** `PointNumbers` — Какие точки glyph-а трогает variation: либо явный список индексов,
`crates/engine/font/src/gvar.rs:59` **struct** `TupleVariation` — Описание одной tuple-variation для glyph-а
`crates/engine/font/src/gvar.rs:79` **struct** `GlyphVariationData` — Полный набор tuple-variations для одного glyph-а
`crates/engine/font/src/gvar.rs:88` **struct** `Gvar` — Распарсенная gvar-таблица. Хранит per-glyph offsets в массив сырых
`crates/engine/font/src/gvar.rs:107` **fn** `parse`
`crates/engine/font/src/gvar.rs:179` **fn** `glyph_variation_data` — Сырой byte-slice glyph-variation-data для одного glyph-а. `None`,
`crates/engine/font/src/gvar.rs:197` **fn** `parse_glyph` — Декодирует `GlyphVariationData` для glyph-а. `None` если у glyph-а
`crates/engine/font/src/gvar.rs:465` **fn** `tuple_axis_scalar` — Per-axis scalar tent-функции для одной оси tuple-variation
`crates/engine/font/src/gvar.rs:512` **fn** `tuple_scalar` — Региональный scalar для всех осей tuple-variation: произведение per-axis
`crates/engine/font/src/head.rs:18` **struct** `Head`
`crates/engine/font/src/head.rs:28` **enum** `IndexToLocFormat`
`crates/engine/font/src/head.rs:36` **fn** `parse`
`crates/engine/font/src/hhea.rs:10` **struct** `Hhea`
`crates/engine/font/src/hhea.rs:19` **fn** `parse`
`crates/engine/font/src/hmtx.rs:12` **struct** `Hmtx`
`crates/engine/font/src/hmtx.rs:19` **fn** `parse`
`crates/engine/font/src/hmtx.rs:35` **fn** `advance_width`
`crates/engine/font/src/hmtx.rs:46` **fn** `left_side_bearing`
`crates/engine/font/src/hvar.rs:26` **struct** `Hvar`
`crates/engine/font/src/hvar.rs:38` **fn** `parse`
`crates/engine/font/src/hvar.rs:72` **fn** `advance_width_index` — `(outer, inner)`-индекс для advance width variations glyph_id
`crates/engine/font/src/hvar.rs:79` **fn** `lsb_index` — Аналогично для LSB. `None`-map → identity-fallback. Caller обычно
`crates/engine/font/src/hvar.rs:83` **fn** `rsb_index`
`crates/engine/font/src/hvar.rs:89` **fn** `has_lsb_variations` — `true`, если HVAR содержит хоть один map для LSB (т.е. шрифт
`crates/engine/font/src/hvar.rs:93` **fn** `has_rsb_variations`
`crates/engine/font/src/item_variation.rs:31` **struct** `RegionAxisCoordinates` — Один axis-сегмент региона: tent-функция со scalar = 1.0 в peak,
`crates/engine/font/src/item_variation.rs:50` **fn** `scalar` — Per-axis scalar для tent-функции в `coord`. Возвращает значение
`crates/engine/font/src/item_variation.rs:92` **struct** `VariationRegion` — Один variation region — кортеж `RegionAxisCoordinates` на каждую ось
`crates/engine/font/src/item_variation.rs:104` **fn** `scalar` — Региональный scalar — произведение per-axis scalars. Region
`crates/engine/font/src/item_variation.rs:120` **struct** `VariationRegionList` — Список всех регионов, на которые могут ссылаться item-variation-data
`crates/engine/font/src/item_variation.rs:134` **struct** `ItemVariationData` — Блок per-item delta-наборов: для `item_count` items, каждый item
`crates/engine/font/src/item_variation.rs:146` **struct** `ItemVariationStore` — Root variation store. `format == 1` для всех современных шрифтов
`crates/engine/font/src/item_variation.rs:155` **fn** `parse` — Parses an `ItemVariationStore` starting at the beginning of `data`
`crates/engine/font/src/item_variation.rs:198` **fn** `evaluate` — Вычисляет суммарный delta для item `(outer, inner)` при текущих
`crates/engine/font/src/item_variation.rs:219` **fn** `is_empty` — `true`, если store не содержит ни регионов, ни data blocks —
`crates/engine/font/src/loca.rs:17` **struct** `Loca`
`crates/engine/font/src/loca.rs:24` **fn** `parse`
`crates/engine/font/src/loca.rs:46` **fn** `glyph_range` — Возвращает `(offset, length)` в байтах внутри `glyf`-таблицы,
`crates/engine/font/src/maxp.rs:9` **struct** `Maxp`
`crates/engine/font/src/maxp.rs:14` **fn** `parse`
`crates/engine/font/src/mvar.rs:29` **struct** `ValueRecord` — Одна запись MVAR: tag метрики + (outer, inner) для lookup в IVS
`crates/engine/font/src/mvar.rs:42` **struct** `Mvar`
`crates/engine/font/src/mvar.rs:50` **fn** `parse`
`crates/engine/font/src/mvar.rs:102` **fn** `lookup` — Lookup `(outer, inner)` для метрики по tag-у. `None`, если запись
`crates/engine/font/src/mvar.rs:114` **fn** `is_sorted_by_tag` — Проверяет, что records отсортированы по tag — инвариант OpenType
`crates/engine/font/src/name.rs:41` **struct** `Name` — Минимальный набор строк, нужных font matcher-у
`crates/engine/font/src/name.rs:55` **fn** `parse`
`crates/engine/font/src/name.rs:85` **fn** `best_family` — «Лучшее» family name: typographic, если есть, иначе обычный family
`crates/engine/font/src/os2.rs:32` **struct** `Os2` — Расширенный набор полей `OS/2`
`crates/engine/font/src/os2.rs:112` **fn** `is_italic` — Italic flag из `fsSelection`
`crates/engine/font/src/os2.rs:117` **fn** `is_oblique` — Oblique flag (OS/2 v4+)
`crates/engine/font/src/os2.rs:123` **fn** `is_bold` — Bold flag из `fsSelection`. Не источник истины для веса —
`crates/engine/font/src/os2.rs:129` **fn** `stretch_percent` — Возвращает stretch в процентах (от 50 до 200)
`crates/engine/font/src/os2.rs:144` **fn** `parse`
`crates/engine/font/src/otlayout.rs:29` **fn** `apply_feature_overrides` — Apply CSS `font-feature-settings` overrides to a default feature-tag set
`crates/engine/font/src/otlayout.rs:63` **struct** `LayoutHeader` — Parsed header of a `GSUB`/`GPOS` table: byte offsets (relative to the
`crates/engine/font/src/otlayout.rs:76` **fn** `parse` — Parse the 10-byte (v1.0) / 14-byte (v1.1) header at the start of a
`crates/engine/font/src/otlayout.rs:97` **struct** `Lookup` — A single lookup: its type, flags and the absolute byte offsets (within
`crates/engine/font/src/otlayout.rs:110` **struct** `LayoutTable` — Borrowed view over a `GSUB`/`GPOS` table providing lookup access and the
`crates/engine/font/src/otlayout.rs:119` **fn** `parse` — Parse the table header; returns `None` for malformed/empty data
`crates/engine/font/src/otlayout.rs:127` **fn** `lookup_count` — Total number of lookups in the LookupList
`crates/engine/font/src/otlayout.rs:134` **fn** `lookup` — Resolve a lookup by its LookupList index: returns its type, flags and
`crates/engine/font/src/otlayout.rs:166` **fn** `enabled_lookups` — Collect the LookupList indices activated by any of the `wanted`
`crates/engine/font/src/otlayout.rs:271` **enum** `Coverage` — A Coverage table: maps a glyph id to a *coverage index* (its ordinal
`crates/engine/font/src/otlayout.rs:282` **struct** `CoverageRange` — One range record of a format-2 Coverage table
`crates/engine/font/src/otlayout.rs:293` **fn** `parse` — Parse a Coverage table located at absolute `offset` within `data`
`crates/engine/font/src/otlayout.rs:322` **fn** `index_of` — Return the coverage index of `glyph`, or `None` if not covered
`crates/engine/font/src/otlayout.rs:351` **enum** `ClassDef` — A Class Definition table: maps a glyph id to a class number (0 for any
`crates/engine/font/src/otlayout.rs:366` **struct** `ClassRange` — One range record of a format-2 ClassDef table
`crates/engine/font/src/otlayout.rs:378` **fn** `parse` — Parse a ClassDef table at absolute `offset`. A NULL (`0`) offset has
`crates/engine/font/src/otlayout.rs:411` **fn** `class_of` — Return the class of `glyph` (0 when not explicitly assigned)
`crates/engine/font/src/otlayout.rs:454` **struct** `ValueRecord` — A GPOS ValueRecord: positional adjustments in font design units. Fields
`crates/engine/font/src/otlayout.rs:466` **fn** `value_record_size` — Number of bytes a ValueRecord with `format` occupies (2 per set bit)
`crates/engine/font/src/otlayout.rs:473` **fn** `read_value_record` — Read a ValueRecord of the given `format` at absolute `offset`, returning
`crates/engine/font/src/otlayout.rs:510` **fn** `resolve_extension` — Resolve an Extension subtable (GSUB Lookup Type 7 / GPOS Lookup Type 9):
`crates/engine/font/src/post.rs:18` **struct** `Post`
`crates/engine/font/src/post.rs:47` **fn** `parse`
`crates/engine/font/src/post.rs:71` **fn** `is_italic` — `true` если italic_angle != 0 (шрифт имеет slant). Удобный
`crates/engine/font/src/rasterizer.rs:20` **struct** `Bitmap`
`crates/engine/font/src/rasterizer.rs:35` **struct** `Rasterizer`
`crates/engine/font/src/rasterizer.rs:41` **fn** `new`
`crates/engine/font/src/rasterizer.rs:49` **fn** `scale`
`crates/engine/font/src/rasterizer.rs:55` **fn** `rasterize` — Растеризует simple-glyph. Возвращает `None` для composite-глифов
`crates/engine/font/src/shape.rs:24` **struct** `ShapedGlyph` — One positioned glyph produced by shaping. All metrics are in font design
`crates/engine/font/src/shape.rs:47` **struct** `Shaper` — Shaping engine bound to one font's `GSUB`/`GPOS` tables
`crates/engine/font/src/shape.rs:55` **fn** `new` — Build a shaper from a parsed font, reading its `GSUB`/`GPOS` tables
`crates/engine/font/src/shape.rs:65` **fn** `with_features` — Like [`Shaper::new`], but with CSS `font-feature-settings` overrides
`crates/engine/font/src/shape.rs:78` **fn** `is_active` — Whether shaping will change anything versus base advances — i.e. the
`crates/engine/font/src/shape.rs:88` **fn** `shape` — Shape a run of glyph ids into positioned glyphs
`crates/engine/font/src/system_fonts.rs:31` **struct** `SystemFontIndex` — Простой ленивый индекс системных шрифтов
`crates/engine/font/src/system_fonts.rs:44` **fn** `new` — Индекс, который при первом lookup просканирует стандартные пути
`crates/engine/font/src/system_fonts.rs:53` **fn** `with_dirs` — Индекс с явно заданным списком директорий — для тестов и
`crates/engine/font/src/system_fonts.rs:66` **fn** `family_count` — Сколько family-имён зарегистрировано. Для тестов и диагностики;
`crates/engine/font/src/unicode_range.rs:12` **struct** `UnicodeRange` — Один диапазон кодепоинтов из `unicode-range:` дескриптора @font-face
`crates/engine/font/src/unicode_range.rs:21` **fn** `contains` — Проверяет, входит ли кодепоинт `cp` в этот диапазон
`crates/engine/font/src/unicode_range.rs:35` **fn** `parse_unicode_ranges` — Парсит CSS `unicode-range` дескриптор в список `UnicodeRange`
`crates/engine/font/src/unicode_range.rs:74` **fn** `codepoint_in_ranges` — Проверяет, покрывается ли кодепоинт хотя бы одним диапазоном из списка
`crates/engine/font/src/variation.rs:80` **fn** `apply_variations_to_simple_outline` — Применяет набор `TupleVariation` к outline-контурам, имитируя
`crates/engine/font/src/variation_coords.rs:28` **struct** `VariationCoords` — Normalized variation coordinates for a font instance. Stores one f32 per axis
`crates/engine/font/src/variation_coords.rs:33` **fn** `empty` — Creates an empty coordinate vector (no variations applied; uses default
`crates/engine/font/src/variation_coords.rs:45` **fn** `from_css_settings` — Builds normalized coordinates from CSS `font-variation-settings` values
`crates/engine/font/src/variation_coords.rs:92` **fn** `as_slice` — Returns the coordinate vector as a slice
`crates/engine/font/src/variation_coords.rs:97` **fn** `as_mut_slice` — Returns the coordinate vector as a mutable slice (for P4 to update optical sizing)
`crates/engine/font/src/variation_coords.rs:102` **fn** `is_empty` — Returns true if no coordinates are set (default instance)
`crates/engine/font/src/variation_coords.rs:107` **fn** `len` — Returns the number of axes
`crates/engine/font/src/variation_coords.rs:114` **fn** `get_axis_by_tag` — Gets coordinate for a specific axis by tag (for debugging / CSS property hookup)
`crates/engine/font/src/variation_coords.rs:126` **fn** `set_axis_by_tag` — Sets a specific axis coordinate by tag
`crates/engine/font/src/vvar.rs:31` **struct** `Vvar`
`crates/engine/font/src/vvar.rs:45` **fn** `parse`
`crates/engine/font/src/vvar.rs:80` **fn** `advance_height_index` — `(outer, inner)`-индекс для advance height variations glyph_id
`crates/engine/font/src/vvar.rs:87` **fn** `tsb_index` — Аналогично для TSB. `None`-map → identity-fallback. Caller обычно
`crates/engine/font/src/vvar.rs:91` **fn** `bsb_index`
`crates/engine/font/src/vvar.rs:95` **fn** `v_org_index`
`crates/engine/font/src/vvar.rs:99` **fn** `has_tsb_variations`
`crates/engine/font/src/vvar.rs:103` **fn** `has_bsb_variations`
`crates/engine/font/src/vvar.rs:107` **fn** `has_v_org_variations`
`crates/engine/font/src/woff2.rs:18` **fn** `is_woff2` — Returns `true` if `data` begins with the WOFF2 magic signature
`crates/engine/font/src/woff2.rs:23` **fn** `is_woff1` — Returns `true` if `data` begins with the WOFF1 magic signature
`crates/engine/font/src/woff2.rs:483` **fn** `decode_woff2` — Decode WOFF2 bytes into a raw sfnt byte vector
`crates/engine/font/src/woff2.rs:699` **fn** `decode_woff1` — Decode WOFF1 bytes into a raw sfnt byte vector
`crates/engine/font/src/woff2.rs:764` **fn** `maybe_decode_font` — If `data` is WOFF2 or WOFF1, decode it and return the raw sfnt bytes

## lumen-html-parser  (47 symbols)

`crates/engine/html-parser/src/picture.rs:56` **struct** `PickedSource` — Финальный URL выбранного источника плюс author-объявленные
`crates/engine/html-parser/src/picture.rs:64` **struct** `PictureParams` — Параметры picker-а
`crates/engine/html-parser/src/picture.rs:90` **fn** `pick_picture_source` — Выбрать источник для `<picture>` элемента. См. модульный заголовок
`crates/engine/html-parser/src/picture.rs:136` **fn** `pick_img_source` — Выбрать источник для одиночного `<img>` элемента (`srcset` + `sizes` +
`crates/engine/html-parser/src/preload_scanner.rs:56` **enum** `PreloadHint` — Один speculative-fetch hint, извлечённый preload-сканером
`crates/engine/html-parser/src/preload_scanner.rs:116` **fn** `scan_preload_hints` — Пробежать по HTML и вернуть все subresource-hint-ы, найденные в
`crates/engine/html-parser/src/preload_scanner.rs:240` **struct** `PreloadScanner` — Инкрементальный preload-сканер (HTML LS §13.2.6.4.7)
`crates/engine/html-parser/src/preload_scanner.rs:246` **fn** `new` — Создаёт новый инкрементальный сканер
`crates/engine/html-parser/src/preload_scanner.rs:255` **fn** `feed_bytes` — Скармливает очередной chunk сырых байт и возвращает все hint-ы,
`crates/engine/html-parser/src/preload_scanner.rs:263` **fn** `end` — Завершает ввод и возвращает hint-ы из буферизованного хвоста
`crates/engine/html-parser/src/push_tokenizer.rs:32` **struct** `PushTokenizer` — Push-режим HTML5 токенизатора. См. module-level docs
`crates/engine/html-parser/src/push_tokenizer.rs:51` **fn** `new` — Создаёт новый `PushTokenizer` в исходном состоянии
`crates/engine/html-parser/src/push_tokenizer.rs:66` **fn** `feed` — Скармливает chunk токенизатору и возвращает токены, ставшие
`crates/engine/html-parser/src/push_tokenizer.rs:87` **fn** `feed_bytes` — Вариант [`PushTokenizer::feed`] для сырых байт из сети
`crates/engine/html-parser/src/push_tokenizer.rs:156` **fn** `end` — Финализирует ввод. Хвост буфера токенизируется как при EOF —
`crates/engine/html-parser/src/push_tokenizer.rs:169` **fn** `pending_len` — Количество ещё не потреблённых байт строкового буфера
`crates/engine/html-parser/src/quirks_mode.rs:18` **fn** `detect_document_mode` — Решение по §13.2.5.1. `public_id`/`system_id` — `None` если в
`crates/engine/html-parser/src/srcset.rs:15` **struct** `SrcsetCandidate` — Один кандидат из `srcset`
`crates/engine/html-parser/src/srcset.rs:23` **enum** `SrcsetDescriptor` — Дескриптор кандидата. По умолчанию `1x` (когда дескриптор
`crates/engine/html-parser/src/srcset.rs:48` **fn** `parse_srcset` — Распарсить значение `srcset` атрибута. Возвращает список кандидатов
`crates/engine/html-parser/src/srcset.rs:172` **fn** `pick_best_for_density` — Выбрать лучший кандидат по DPR для density-descriptors
`crates/engine/html-parser/src/srcset.rs:232` **enum** `SizeLength` — Длина в `sizes`-атрибуте. По HTML5 §4.8.4.4 значение — одиночный
`crates/engine/html-parser/src/srcset.rs:250` **struct** `SizesViewport` — Viewport-параметры для резолва `sizes` в CSS-пиксели. `root_font_size_px`
`crates/engine/html-parser/src/srcset.rs:269` **fn** `resolve` — Резолв длины в CSS-пиксели
`crates/engine/html-parser/src/srcset.rs:287` **enum** `Orientation` — Ориентация viewport-а для media-feature `orientation:`
`crates/engine/html-parser/src/srcset.rs:294` **enum** `ColorScheme` — CSS Media Queries L5 `prefers-color-scheme` значение
`crates/engine/html-parser/src/srcset.rs:306` **enum** `MediaClause` — Одиночный `<media-in-parens>` внутри media-condition (Media Queries L4
`crates/engine/html-parser/src/srcset.rs:360` **enum** `MediaCondition` — Media-condition в `<source media>` / `<img sizes>`-атрибутах
`crates/engine/html-parser/src/srcset.rs:370` **fn** `matches` — Принимает решение, удовлетворяет ли viewport условие
`crates/engine/html-parser/src/srcset.rs:383` **struct** `SourceSize` — Один элемент `sizes`-списка: опциональный media-condition + length
`crates/engine/html-parser/src/srcset.rs:402` **fn** `parse_sizes` — Распарсить значение `sizes`-атрибута. Возвращает список
`crates/engine/html-parser/src/srcset.rs:504` **fn** `parse_media_condition` — Распарсить media-condition. Lenient: `Unsupported` вместо `None` —
`crates/engine/html-parser/src/srcset.rs:697` **fn** `evaluate_sizes` — Вычислить эффективную «source size» в CSS-пикселях по `sizes` и
`crates/engine/html-parser/src/srcset.rs:724` **fn** `pick_best_for_width` — Выбрать лучший кандидат по w-descriptor (HTML5 §4.8.4.3.7)
`crates/engine/html-parser/src/tokenizer.rs:21` **enum** `Token`
`crates/engine/html-parser/src/tokenizer.rs:47` **struct** `Tokenizer`
`crates/engine/html-parser/src/tokenizer.rs:58` **fn** `new`
`crates/engine/html-parser/src/tokenizer.rs:71` **fn** `with_state` — Создаёт tokenizer с заранее заданным `text_only`-состоянием
`crates/engine/html-parser/src/tokenizer.rs:81` **fn** `pos` — Текущая позиция курсора (в байтах от начала `input`). Используется
`crates/engine/html-parser/src/tokenizer.rs:87` **fn** `text_only_state` — Текущее `text_only`-состояние. После исчерпания iterator-а это
`crates/engine/html-parser/src/tree_builder.rs:47` **fn** `parse` — Парсит вход целиком в pull-режиме и возвращает построенный
`crates/engine/html-parser/src/tree_builder.rs:121` **struct** `IncrementalTreeBuilder` — Push-режим tree builder-а: принимает HTML chunk-ами, держит
`crates/engine/html-parser/src/tree_builder.rs:167` **fn** `new` — Создаёт пустой builder в insertion mode `Initial`
`crates/engine/html-parser/src/tree_builder.rs:189` **fn** `feed` — Скармливает chunk push-токенизатору и применяет полученные
`crates/engine/html-parser/src/tree_builder.rs:196` **fn** `feed_bytes` — Вариант [`feed`][Self::feed] для сырых байт
`crates/engine/html-parser/src/tree_builder.rs:203` **fn** `as_doc` — Возвращает ссылку на текущее состояние DOM
`crates/engine/html-parser/src/tree_builder.rs:212` **fn** `finish` — Финализирует ввод. Хвост push-tokenizer-а токенизируется как

## lumen-image  (67 symbols)

`crates/engine/image/src/avif/mod.rs:19` **enum** `AvifError` — Ошибка декодирования AVIF
`crates/engine/image/src/avif/mod.rs:47` **fn** `is_avif` — Проверяет AVIF-сигнатуру по ISOBMFF ftyp-боксу
`crates/engine/image/src/avif/mod.rs:68` **fn** `decode_avif` — Декодирует AVIF-файл в RGBA8 (4 байта на пиксель, row-major)
`crates/engine/image/src/avif/mod.rs:96` **struct** `AvifImageDecoder` — Реализация [`lumen_core::ext::ImageDecoder`] для AVIF
`crates/engine/image/src/decode_cache.rs:17` **type** `ImageHandle` — A thin, reference-counted pointer to a decoded image stored in `ImageDecodeCache`
`crates/engine/image/src/decode_cache.rs:23` **struct** `ImageKey` — Cache key identifying a decoded image
`crates/engine/image/src/decode_cache.rs:27` **fn** `new` — Construct from a URL or hash string
`crates/engine/image/src/decode_cache.rs:52` **struct** `ImageDecodeCache` — LRU decode cache for decoded raster images
`crates/engine/image/src/decode_cache.rs:67` **fn** `new` — Create a new cache with the default 256 MB budget
`crates/engine/image/src/decode_cache.rs:72` **fn** `with_budget` — Create a new cache with a custom memory budget in bytes
`crates/engine/image/src/decode_cache.rs:82` **fn** `used_bytes` — Current memory used by all cached images (bytes)
`crates/engine/image/src/decode_cache.rs:87` **fn** `budget_bytes` — Memory budget (bytes)
`crates/engine/image/src/decode_cache.rs:92` **fn** `len` — Number of cached images
`crates/engine/image/src/decode_cache.rs:97` **fn** `is_empty` — `true` if no images are cached
`crates/engine/image/src/decode_cache.rs:102` **fn** `contains` — `true` if the key is present in the cache
`crates/engine/image/src/decode_cache.rs:109` **fn** `get` — Look up a cached image by key, updating its LRU timestamp
`crates/engine/image/src/decode_cache.rs:125` **fn** `insert` — Insert a decoded image into the cache and return a handle
`crates/engine/image/src/decode_cache.rs:158` **fn** `decode_or_get` — Decode and cache an image, or return the existing cached handle
`crates/engine/image/src/decode_cache.rs:173` **fn** `evict_to_budget` — Evict least-recently-used entries until `used_bytes <= budget_bytes`
`crates/engine/image/src/decode_cache.rs:201` **fn** `remove` — Remove a single cached entry by key
`crates/engine/image/src/decode_cache.rs:211` **fn** `clear` — Evict all cached entries regardless of budget
`crates/engine/image/src/decode_cache.rs:219` **fn** `lru_candidates` — Return LRU candidates sorted from least- to most-recently used
`crates/engine/image/src/decode_cache.rs:234` **fn** `on_memory_pressure` — React to an OS memory pressure event by evicting proportionally
`crates/engine/image/src/gif.rs:12` **enum** `GifError` — Ошибки декодирования GIF
`crates/engine/image/src/gif.rs:37` **fn** `is_gif` — Проверяет, является ли начало `bytes` валидной GIF сигнатурой (GIF87a или GIF89a)
`crates/engine/image/src/gif.rs:46` **struct** `AnimatedFrame` — Один кадр анимированного GIF
`crates/engine/image/src/gif.rs:58` **fn** `delay_ms` — Возвращает задержку в миллисекундах
`crates/engine/image/src/gif.rs:66` **enum** `GifLoopCount` — Количество повторений анимации GIF
`crates/engine/image/src/gif.rs:75` **struct** `AnimatedGif` — Анимированный GIF: кадры + размер + метаданные цикличности
`crates/engine/image/src/gif.rs:93` **fn** `frame_index_at` — Возвращает индекс кадра для `elapsed_ms` миллисекунд от начала анимации
`crates/engine/image/src/gif.rs:126` **fn** `frame_at` — Возвращает кадр для `elapsed_ms` миллисекунд от начала анимации
`crates/engine/image/src/gif.rs:140` **fn** `decode_gif` — Декодирует GIF файл и возвращает первый кадр
`crates/engine/image/src/gif.rs:164` **fn** `decode_gif_animated` — Декодирует все кадры GIF и возвращает [`AnimatedGif`]
`crates/engine/image/src/heic.rs:18` **struct** `HeicError` — Error decoding a HEIC/HEIF image
`crates/engine/image/src/heic.rs:33` **fn** `is_heic` — Detects HEIC/HEIF image format
`crates/engine/image/src/heic.rs:66` **fn** `decode_heic` — Stub HEIC/HEIF decoder (Phase 1)
`crates/engine/image/src/jpeg/mod.rs:94` **fn** `decode_jpeg`
`crates/engine/image/src/jpeg/mod.rs:247` **struct** `JpegError` — Ошибка декодирования JPEG (обёртка над zune-jpeg)
`crates/engine/image/src/jxl.rs:16` **struct** `JxlError` — Error decoding a JPEG XL image
`crates/engine/image/src/jxl.rs:32` **fn** `is_jxl` — Detects JPEG XL image format
`crates/engine/image/src/jxl.rs:70` **fn** `decode_jxl` — Stub JPEG XL decoder (Phase 0)
`crates/engine/image/src/lib.rs:38` **fn** `supported_mime_types` — MIME-типы изображений, которые `decode` умеет декодировать
`crates/engine/image/src/lib.rs:60` **fn** `is_svg` — Checks whether the given bytes look like an SVG document
`crates/engine/image/src/lib.rs:89` **fn** `decode_to` — Декодирует растровое изображение по сигнатуре первых байтов и colour-manages
`crates/engine/image/src/lib.rs:106` **fn** `decode` — Декодирует растровое изображение по сигнатуре первых байтов
`crates/engine/image/src/lib.rs:145` **enum** `ImageError` — Ошибка `decode`
`crates/engine/image/src/lib.rs:211` **enum** `IccGamut` — Идентифицированный цветовой охват ICC профиля
`crates/engine/image/src/lib.rs:226` **struct** `IccProfile` — ICC профиль изображения (опциональный)
`crates/engine/image/src/lib.rs:234` **fn** `is_valid` — Проверяет минимальный размер ICC профиля (128 байт)
`crates/engine/image/src/lib.rs:244` **fn** `detect_gamut` — Определяет цветовой охват по сигнатуре пространства данных (bytes 16-19)
`crates/engine/image/src/lib.rs:309` **fn** `correct_rgba_pixels` — Применяет ICC-коррекцию к RGBA8 пикселям in-place
`crates/engine/image/src/lib.rs:445` **struct** `Image` — Декодированное растровое изображение в плотной row-major упаковке
`crates/engine/image/src/lib.rs:459` **fn** `detect_color_space` — Детектирует цветовое пространство изображения из ICC профиля или сигнатуры изображения
`crates/engine/image/src/lib.rs:469` **fn** `to_rgba8` — Возвращает пиксели в формате RGBA8 (4 байта на пиксель)
`crates/engine/image/src/lib.rs:513` **fn** `to_rgba8_tone_mapped` — Alias for `to_rgba8()`. Tone-mapping is now applied automatically
`crates/engine/image/src/lib.rs:524` **fn** `apply_icc_rgb_transform` — Applies a compiled ICC matrix-shaper transform to RGBA8 pixels in place
`crates/engine/image/src/lib.rs:609` **fn** `apply_tone_mapping` — Apply tone mapping for a detected color space
`crates/engine/image/src/lib.rs:669` **fn** `resize_bilinear` — Масштабирует `src` до `(dst_w × dst_h)` билинейной интерполяцией
`crates/engine/image/src/lib.rs:721` **fn** `resize_area_avg` — Масштабирует `src` до `(dst_w × dst_h)` усреднением по площади (box filter)
`crates/engine/image/src/lib.rs:780` **enum** `PixelFormat` — Формат пикселя декодированного изображения. Все варианты — 8 бит на канал
`crates/engine/image/src/lib.rs:804` **enum** `DecodeError` — Ошибки декодирования PNG
`crates/engine/image/src/png/mod.rs:59` **fn** `decode_png`
`crates/engine/image/src/png/mod.rs:101` **fn** `encode_png_rgba8` — Кодирует RGBA8 изображение в PNG формат
`crates/engine/image/src/webp/mod.rs:24` **struct** `WebpError` — Ошибка декодирования WebP
`crates/engine/image/src/webp/mod.rs:39` **fn** `is_webp` — Проверяет WebP-сигнатуру без полной валидации
`crates/engine/image/src/webp/mod.rs:52` **fn** `decode_webp` — Декодирует WebP-файл в RGBA8 (4 байта на пиксель, row-major)
`crates/engine/image/src/webp/mod.rs:88` **struct** `WebpImageDecoder` — Реализация [`lumen_core::ext::ImageDecoder`] для WebP

## lumen-ipc  (16 symbols)

`crates/ipc/src/lib.rs:36` **type** `TabId` — Identifier for a tab in the shell's `--ipc-server` control channel (TAB-4)
`crates/ipc/src/lib.rs:44` **enum** `IpcRequest` — A request sent over an IPC channel
`crates/ipc/src/lib.rs:77` **enum** `IpcResponse` — A response sent back over an IPC channel
`crates/ipc/src/lib.rs:119` **struct** `FetchRequest` — Parameters for a fetch request (Phase 1: GET-only, no custom headers/body)
`crates/ipc/src/lib.rs:135` **struct** `FetchOk` — Successful HTTP response payload returned by the network service
`crates/ipc/src/lib.rs:148` **struct** `FetchErr` — Error returned when a fetch fails
`crates/ipc/src/lib.rs:161` **struct** `IpcChannel` — Bidirectional framing layer over any `Read + Write` stream
`crates/ipc/src/lib.rs:167` **fn** `new` — Wrap an existing stream
`crates/ipc/src/lib.rs:172` **fn** `send` — Serialize `msg` via bincode and write it with a 4-byte LE length prefix
`crates/ipc/src/lib.rs:190` **fn** `recv` — Read one length-prefixed message and deserialize it
`crates/ipc/src/lib.rs:207` **struct** `IpcServer` — TCP server that the network service uses to accept connections from the shell
`crates/ipc/src/lib.rs:215` **fn** `bind` — Bind on an OS-assigned loopback port. Returns `(server, bound_port)`
`crates/ipc/src/lib.rs:226` **fn** `accept` — Block until the shell connects and return the framing channel
`crates/ipc/src/lib.rs:245` **struct** `IpcClient` — Client used by the shell to communicate with the network service
`crates/ipc/src/lib.rs:251` **fn** `connect` — Connect to the network service listening on `127.0.0.1:port`
`crates/ipc/src/lib.rs:261` **fn** `request` — Send a request and block until the matching response arrives

## lumen-js  (365 symbols)

`crates/js/src/async_context.rs:32` **fn** `install_async_context` — Install the `AsyncContext` global (Variable + Snapshot) into the context
`crates/js/src/attribution_reporting.rs:23` **fn** `install_attribution_reporting_api` — Install Attribution Reporting API bindings into the JS context
`crates/js/src/audio_bindings.rs:37` **fn** `new_session_seed` — Generate a unique per-session noise seed
`crates/js/src/audio_bindings.rs:46` **fn** `install_audio_bindings` — Install the complete Web Audio API Level 2 into the JS context
`crates/js/src/audio_element.rs:56` **fn** `set_audio_playback_provider` — Install the platform audio playback backend
`crates/js/src/audio_element.rs:72` **fn** `install_audio_element_bindings` — Install `HTMLAudioElement` Phase 1 bindings into the JS context
`crates/js/src/background_fetch.rs:22` **fn** `init_background_fetch` — Install the Background Fetch API stub into the JS context
`crates/js/src/background_sync.rs:17` **fn** `init_background_sync` — Install the Background Sync API stub into the JS context
`crates/js/src/badging.rs:12` **fn** `install_badging_bindings` — Install Badging API bindings into the JS context
`crates/js/src/battery_bindings.rs:22` **fn** `install_battery_bindings` — Install Battery Status API disable shim into the JS context
`crates/js/src/bluetooth.rs:5` **fn** `install_bluetooth_bindings`
`crates/js/src/broadcast_channel.rs:61` **struct** `LocalChannel` — A channel instance owned by the current runtime: the receiver half plus its id
`crates/js/src/broadcast_channel.rs:72` **type** `BroadcastRegistry` — All `BroadcastChannel` instances created in this runtime
`crates/js/src/broadcast_channel.rs:80` **fn** `register` — Register a new channel instance for `name` and return its unique id
`crates/js/src/broadcast_channel.rs:100` **fn** `post` — Deliver `json` to every channel named `name` except the sender (`sender_id`)
`crates/js/src/broadcast_channel.rs:119` **fn** `close` — Remove the channel instance `id` from the global hub and this runtime
`crates/js/src/broadcast_channel.rs:135` **fn** `drain` — Drain all pending messages addressed to this runtime's channels
`crates/js/src/broadcast_channel.rs:150` **fn** `install_broadcast_channel_bindings` — Install the `_lumen_bc_*` native bindings and the `BroadcastChannel` JS class
`crates/js/src/canvas2d.rs:253` **fn** `present_rgba` — Present a WebGPU-rendered RGBA8 frame into the `<canvas>` `nid`'s CPU buffer
`crates/js/src/canvas2d.rs:275` **fn** `flush_dirty` — Drain dirty canvases and return their current RGBA buffers
`crates/js/src/canvas2d.rs:302` **fn** `install_canvas2d_bindings` — Register the `_lumen_canvas2d_*` native functions on `globals`
`crates/js/src/clipboard.rs:33` **fn** `set_clipboard_provider` — Install the host clipboard provider backing `navigator.clipboard`
`crates/js/src/close_watcher.rs:19` **fn** `install_close_watcher` — Install `CloseWatcher` class + Escape key handler into the JS context
`crates/js/src/compute_pressure.rs:8` **fn** `install_compute_pressure_bindings` — Install Compute Pressure API bindings into the JS context
`crates/js/src/contacts.rs:15` **fn** `init_contacts_manager` — Install the Contact Picker API stub into the JS context
`crates/js/src/content_index.rs:18` **fn** `install_content_index_api` — Install Content Index API on `ServiceWorkerRegistration.prototype`
`crates/js/src/cookie_banner.rs:30` **fn** `install_cookie_banner_bindings` — Install cookie-banner auto-dismiss shim into the JS context
`crates/js/src/cookie_banner.rs:160` **fn** `install_with_selectors` — Build the `_LUMEN_CONSENT_SELECTORS` global value and inject the shim
`crates/js/src/cookie_store.rs:17` **fn** `init_cookie_store` — Install the Cookie Store API into the JS context
`crates/js/src/credentials.rs:50` **fn** `set_credential_provider` — Install the host credential provider backing `navigator.credentials`
`crates/js/src/credentials.rs:66` **fn** `install_credentials_bindings` — Install the `navigator.credentials` JS shim
`crates/js/src/csp.rs:12` **fn** `install_csp_bindings` — Install CSP JS bindings: `SecurityPolicyViolationEvent` class and
`crates/js/src/css_properties_values_api.rs:14` **struct** `RegisteredPropertiesMap` — Maps property name (e.g. "--my-color") to its definition
`crates/js/src/css_properties_values_api.rs:19` **fn** `new`
`crates/js/src/css_properties_values_api.rs:24` **fn** `register` — Register a custom property definition
`crates/js/src/css_properties_values_api.rs:29` **fn** `get` — Look up a registered property by name
`crates/js/src/css_properties_values_api.rs:34` **fn** `all` — Get all registered properties
`crates/js/src/css_properties_values_api.rs:39` **fn** `clear` — Clear all registrations (for tests)
`crates/js/src/css_properties_values_api.rs:45` **fn** `get_registered_properties` — Get the global registered properties registry, initializing it if necessary
`crates/js/src/css_properties_values_api.rs:51` **struct** `RegisteredProperty` — Definition of a custom CSS property
`crates/js/src/css_properties_values_api.rs:64` **fn** `install_css_properties_values_api` — Install CSS.registerProperty bindings into the JS context
`crates/js/src/decorators.rs:39` **fn** `install_decorator_shim` — Install the decorator transformer shim and well-known symbols into `ctx`
`crates/js/src/decorators.rs:50` **fn** `maybe_transform_decorators` — Pre-process `source` through the JS decorator transformer
`crates/js/src/device_sensors.rs:8` **fn** `install_device_sensors_bindings`
`crates/js/src/digital_credentials.rs:19` **fn** `install_digital_credentials_api` — Install Digital Credentials API stubs into the JS context
`crates/js/src/document_pip.rs:8` **fn** `install_document_pip_api` — Install Document Picture-in-Picture API into the JS context
`crates/js/src/dom.rs:111` **enum** `NavigateRequest` — Navigation request emitted by JS (`location.href =`, `location.assign()`,
`crates/js/src/dom.rs:128` **enum** `HistoryUrlUpdate` — Notification emitted by `history.pushState`/`history.replaceState` so the
`crates/js/src/dom.rs:158` **enum** `NavAction` — Discriminant embedded in `pending_navigation_updates` to tell the shell
`crates/js/src/dom.rs:173` **type** `NavUpdate` — Tuple stored in `pending_navigation_updates`:
`crates/js/src/dom.rs:181` **struct** `PopupRequest` — A popup window request emitted by JS `window.open(url, target, features)`
`crates/js/src/dom.rs:197` **struct** `PrintRequest` — A print request emitted by `window.print()` (W-2 Phase 1)
`crates/js/src/dom.rs:230` **enum** `FullscreenRequest` — A fullscreen API request emitted by JS `element.requestFullscreen()` or
`crates/js/src/dom.rs:270` **fn** `install_dom_api` — Install DOM primitives (`_lumen_*`) and the Web API shim into `ctx`
`crates/js/src/dom_parser.rs:34` **fn** `install_dom_parser` — Install DOMParser and XMLSerializer into the JS context
`crates/js/src/download_bindings.rs:26` **struct** `DownloadRequest` — A single pending download asked for by JS, awaiting the shell to start it
`crates/js/src/download_bindings.rs:45` **fn** `enqueue` — Enqueue a download request. Public so non-JS engine paths (e.g. a future
`crates/js/src/download_bindings.rs:52` **fn** `take_download_requests` — Drain and return all pending download requests
`crates/js/src/download_bindings.rs:61` **fn** `install_download_bindings` — Install the `_lumen_network_download(url, filename)` native binding
`crates/js/src/element_internals.rs:10` **fn** `install_element_internals_bindings` — Install ElementInternals and CustomStateSet bindings into the JS context
`crates/js/src/es2026_proposals.rs:24` **fn** `install_es2026_proposals` — Install all ES2026+ proposal shims into the given QuickJS context
`crates/js/src/esm.rs:27` **type** `SharedPageUrl` — Shared, late-writable page URL used by `LumenResolver` to resolve relative
`crates/js/src/esm.rs:34` **type** `ModuleRegistry` — Shared module source registry: specifier → source code
`crates/js/src/esm.rs:37` **fn** `new_registry` — Creates an empty `ModuleRegistry`
`crates/js/src/esm.rs:46` **struct** `ImportMap` — Import map: specifier mappings for bare specifiers and scoped paths
`crates/js/src/esm.rs:58` **fn** `parse` — Parse an import map from a JSON string
`crates/js/src/esm.rs:94` **fn** `resolve` — Resolve a specifier using this import map
`crates/js/src/esm.rs:137` **struct** `LumenResolver` — URL resolver: normalises module specifiers into canonical keys for the registry
`crates/js/src/esm.rs:147` **fn** `new` — Create a resolver; `page_url` is the initial fallback base (may be empty)
`crates/js/src/esm.rs:156` **fn** `set_import_map` — Set the import map for this resolver
`crates/js/src/esm.rs:170` **fn** `resolve_specifier` — Resolve `name` relative to `base` using simplified URL resolution rules
`crates/js/src/esm.rs:226` **struct** `LumenLoader` — Module loader backed by `ModuleRegistry`
`crates/js/src/esm.rs:235` **fn** `new` — Create a loader backed by `registry` with no declared module types
`crates/js/src/esm.rs:241` **fn** `with_types` — Create a loader that also consults `types` for import-attribute
`crates/js/src/eye_dropper.rs:8` **fn** `install_eye_dropper_bindings`
`crates/js/src/file_input.rs:57` **fn** `register_file_token` — Register a file path and return an opaque token for JS access
`crates/js/src/file_input.rs:64` **fn** `clear_file_registry` — Revoke all tokens — should be called when a browsing context is torn down
`crates/js/src/file_input.rs:139` **fn** `install_file_input_bindings` — Install File / FileList classes, native read bindings, and `_lumen_deliver_file_list`
`crates/js/src/form_validation.rs:9` **fn** `install_form_validation_bindings` — Install Form Constraint Validation API bindings into the JS context
`crates/js/src/gamepad.rs:31` **fn** `install_gamepad_bindings` — Install Gamepad API shim into the JS context
`crates/js/src/gc_policy.rs:12` **enum** `GcLevel` — GC aggressiveness level for [`crate::QuickJsRuntime::run_gc_pass`]
`crates/js/src/generic_sensor.rs:16` **fn** `install_generic_sensor_bindings` — Install Generic Sensor API bindings into the JS context
`crates/js/src/geolocation.rs:25` **struct** `FakeCoords` — Fake geographic coordinates injected into the Geolocation API
`crates/js/src/geolocation.rs:43` **fn** `install_geolocation_bindings` — Install the Geolocation API stub into the JS context
`crates/js/src/heap_snapshot.rs:40` **enum** `HeapSnapshotError` — Error from the heap-snapshot compression layer
`crates/js/src/heap_snapshot.rs:74` **fn** `compress_heap` — Compress a raw heap payload into a [`SuspendedHeap`]
`crates/js/src/heap_snapshot.rs:97` **fn** `decompress_heap` — Inverse of [`compress_heap`]: strip the [`HEAP_MAGIC`] prefix and inflate
`crates/js/src/highlight_api.rs:10` **struct** `HighlightRegistry`
`crates/js/src/highlight_api.rs:15` **fn** `new`
`crates/js/src/highlight_api.rs:19` **fn** `set`
`crates/js/src/highlight_api.rs:23` **fn** `get`
`crates/js/src/highlight_api.rs:27` **fn** `has`
`crates/js/src/highlight_api.rs:31` **fn** `delete`
`crates/js/src/highlight_api.rs:35` **fn** `clear`
`crates/js/src/highlight_api.rs:39` **fn** `all`
`crates/js/src/highlight_api.rs:47` **fn** `get_highlights_registry`
`crates/js/src/highlight_api.rs:52` **struct** `Highlight`
`crates/js/src/highlight_api.rs:58` **fn** `new`
`crates/js/src/highlight_api.rs:66` **fn** `install_highlight_api_bindings`
`crates/js/src/idle_detection.rs:89` **fn** `install_idle_detection_bindings` — Install Idle Detection API bindings into the JS context
`crates/js/src/iframe_element.rs:30` **fn** `install_iframe_element_bindings` — Install HTMLIFrameElement stubs into the JS context
`crates/js/src/img_bitmap_store.rs:27` **fn** `set_img_bitmap` — Store decoded RGBA8 pixels for an `<img>` element identified by its node id
`crates/js/src/img_bitmap_store.rs:37` **fn** `with_img_bitmap` — Call `f` with `(natural_width, natural_height, rgba8_slice)` for `nid`
`crates/js/src/img_bitmap_store.rs:47` **fn** `clear_img_bitmaps` — Remove all registered bitmaps (call at the start of each navigation to
`crates/js/src/import_attributes.rs:29` **enum** `ModuleType` — Module type declared by an import attribute (`with { type: '...' }`)
`crates/js/src/import_attributes.rs:39` **fn** `from_attr` — Map a raw attribute value (`"json"`, `"css"`, ...) to a `ModuleType`
`crates/js/src/import_attributes.rs:53` **type** `ModuleTypeRegistry` — Shared registry: resolved module specifier → declared module type
`crates/js/src/import_attributes.rs:56` **fn** `new_type_registry` — Creates an empty [`ModuleTypeRegistry`]
`crates/js/src/import_attributes.rs:306` **fn** `strip_import_attributes` — Strip `with { ... }` / `assert { ... }` import-attribute clauses from
`crates/js/src/import_meta.rs:23` **fn** `transform_import_meta` — Transform `import.meta` in `source`, binding `url` as `.url`
`crates/js/src/inert.rs:22` **fn** `install_inert_api` — Install `HTMLElement.prototype.inert` getter/setter into the JS context
`crates/js/src/intl_bindings.rs:42` **fn** `install_intl_bindings` — Install the `Intl` shim into the JS context
`crates/js/src/launch_handler.rs:14` **fn** `install_launch_handler_api` — Install Launch Handler API bindings into the JS context
`crates/js/src/lib.rs:155` **fn** `deterministic_seed_from_url` — Compute a deterministic u64 seed from a URL for deterministic render mode (8F)
`crates/js/src/lib.rs:169` **struct** `QuickJsRuntime` — QuickJS-based JS runtime via `rquickjs`
`crates/js/src/lib.rs:445` **fn** `new`
`crates/js/src/lib.rs:555` **fn** `with_sw_worker_store` — Attach a `SwWorkerStore` so that `_lumen_sw_activate_script` can spawn and
`crates/js/src/lib.rs:588` **fn** `register_module_source` — Register an ES module by specifier so it can be `import`-ed by other modules
`crates/js/src/lib.rs:604` **fn** `set_import_map` — Set the import map (HTML LS §8.1.6.2) used by the module resolver
`crates/js/src/lib.rs:614` **fn** `eval_module` — Evaluate `source` as an ES module (HTML LS §8.1.3 `<script type=module>`)
`crates/js/src/lib.rs:687` **fn** `install_dom` — Install DOM Web API globals (`document`, `window`, `console`, etc.) into
`crates/js/src/lib.rs:1495` **fn** `set_cookie_banner_dismiss` — Enable or disable cookie-banner auto-dismiss for subsequent `install_dom` calls
`crates/js/src/lib.rs:1504` **fn** `set_deterministic_mode` — Enable deterministic render mode (8F)
`crates/js/src/lib.rs:1521` **fn** `freeze_fingerprint` — Freeze fingerprint APIs for canvas / audio / font enumeration (8F.3)
`crates/js/src/lib.rs:1565` **fn** `pump_workers` — Deliver messages posted by worker threads to their `Worker` JS instances
`crates/js/src/lib.rs:1591` **fn** `flush_canvas_updates` — Drain dirty Canvas 2D buffers for upload to the renderer
`crates/js/src/lib.rs:1604` **fn** `pump_broadcast_channels` — Deliver messages posted to this page's `BroadcastChannel` instances
`crates/js/src/lib.rs:1630` **fn** `pump_shared_workers` — Deliver messages posted by `SharedWorker` threads to this page's ports
`crates/js/src/lib.rs:1650` **fn** `take_navigate_request` — Consume any navigation request that JS placed via `location.href =` etc
`crates/js/src/lib.rs:1660` **fn** `update_nav_state` — Update the authoritative navigation state from the shell
`crates/js/src/lib.rs:1669` **fn** `take_nav_updates` — Drain all Navigation API update requests queued by `_lumen_navigation_request`
`crates/js/src/lib.rs:1678` **fn** `take_nav_intercept_result` — Drain `NavigateEvent` intercept results queued by `_lumen_navigation_report_intercept`
`crates/js/src/lib.rs:1683` **fn** `push_nav_update` — Push a Navigation API update into the queue (called by `_lumen_navigation_request`)
`crates/js/src/lib.rs:1696` **fn** `take_history_url_updates` — Drain `history.pushState` / `history.replaceState` URL-update notifications
`crates/js/src/lib.rs:1706` **fn** `take_history_traversals` — Drain all `history.go(n)` / `back` / `forward` traversal deltas queued by
`crates/js/src/lib.rs:1717` **fn** `take_fullscreen_requests` — Drain all fullscreen requests queued by `element.requestFullscreen()` and
`crates/js/src/lib.rs:1725` **fn** `take_view_transition_events` — Drain all View Transition events queued by `document.startViewTransition`
`crates/js/src/lib.rs:1733` **fn** `take_dom_dirty` — Returns `true` if JS mutated the DOM since the last call, clearing the flag
`crates/js/src/lib.rs:1742` **fn** `take_raf_pending` — Returns `true` if `requestAnimationFrame` was called since the last call,
`crates/js/src/lib.rs:1750` **fn** `has_raf_pending` — Non-consuming peek: `true` if `requestAnimationFrame` callbacks are queued
`crates/js/src/lib.rs:1759` **fn** `take_timer_wakeup` — Take the next timer wakeup as Unix epoch ms, clearing the stored value
`crates/js/src/lib.rs:1768` **fn** `update_layout_rects` — Replace the layout bounding-rect table with a fresh snapshot
`crates/js/src/lib.rs:1776` **fn** `update_viewport_size` — Update the viewport dimensions
`crates/js/src/lib.rs:1785` **fn** `take_lazy_image_requests` — Drain lazy image load requests queued by `_lumen_request_lazy_image_load` in JS
`crates/js/src/lib.rs:1797` **fn** `update_scroll_states` — Replace the scroll-state table with a fresh snapshot from the layout tree
`crates/js/src/lib.rs:1806` **fn** `take_scroll_requests` — Drain JS-initiated scroll requests queued by `_lumen_request_scroll`
`crates/js/src/lib.rs:1813` **fn** `take_page_scroll_requests` — Drain JS page-level scroll requests from `window.scrollTo/scrollBy/scroll`
`crates/js/src/lib.rs:1819` **fn** `set_page_scroll_y` — Update the page scroll Y exposed to JS `window.scrollY / pageYOffset`
`crates/js/src/lib.rs:1828` **fn** `take_notification_requests` — Drain all OS notification requests queued by `new Notification(...)` in JS
`crates/js/src/lib.rs:1839` **fn** `take_window_open_requests` — Drain all popup window requests queued by JS `window.open(...)`
`crates/js/src/lib.rs:1848` **fn** `take_print_requests` — Drain all print requests queued by JS `window.print()` (W-2)
`crates/js/src/lib.rs:1857` **fn** `pointer_capture_nid` — Returns the DOM node nid that currently holds pointer capture (pointer_id=1)
`crates/js/src/lib.rs:1865` **fn** `take_pointer_capture` — Release the active pointer capture, returning the former capture target nid
`crates/js/src/lib.rs:1874` **fn** `take_console_messages` — Drain all `console.log/warn/error` messages queued since the last call
`crates/js/src/lib.rs:1883` **fn** `take_focus_requests` — Drain JS dialog focus requests queued by `_lumen_request_focus` / `_lumen_request_blur`
`crates/js/src/lib.rs:1892` **fn** `fire_dialog_close` — Close a `<dialog>` as the result of a `<form method="dialog">` submission
`crates/js/src/lib.rs:1910` **fn** `notify_focus_changed` — Notify the JS runtime that the shell moved keyboard focus to a new node
`crates/js/src/lib.rs:1927` **fn** `update_computed_styles` — Push a fresh snapshot of computed CSS styles into the JS runtime
`crates/js/src/lib.rs:1937` **fn** `set_document_visibility` — Update `document.hidden` / `document.visibilityState` and fire
`crates/js/src/lib.rs:1956` **fn** `notify_dom_content_loaded` — Transition `document.readyState` → `'interactive'` and fire
`crates/js/src/lib.rs:1969` **fn** `notify_window_loaded` — Transition `document.readyState` → `'complete'` and fire
`crates/js/src/lib.rs:1983` **fn** `register_img_bitmaps` — Register decoded RGBA8 bitmaps for `<img>` elements, keyed by node id
`crates/js/src/lib.rs:1998` **fn** `deliver_scroll_progress` — Push viewport scroll progress into all active root-viewport `ScrollTimeline` instances
`crates/js/src/lib.rs:2017` **fn** `fire_element_scroll` — Fire a non-bubbling `scroll` Event on the DOM element identified by `nid`
`crates/js/src/lib.rs:2033` **fn** `fire_window_scroll` — Fire a non-bubbling `scroll` Event on the `window` object (page scroll)
`crates/js/src/lib.rs:2055` **fn** `fire_snap_changing` — Fire a CSS Scroll Snap L2 `snapchanging` event on a scroll container
`crates/js/src/lib.rs:2065` **fn** `fire_snap_changed` — Fire a CSS Scroll Snap L2 `snapchanged` event on a scroll container
`crates/js/src/lib.rs:2101` **fn** `deliver_long_animation_frame` — Deliver a Long Animation Frame (LoAF) entry to PerformanceObserver subscribers
`crates/js/src/lib.rs:2139` **fn** `run_gc_pass` — Tune the QuickJS GC based on the tab's lifecycle tier (10L)
`crates/js/src/local_font_access.rs:19` **fn** `install_local_font_access_api` — Install Local Font Access API shim into the JS context
`crates/js/src/long_animation_frames.rs:24` **fn** `install_long_animation_frames_bindings` — Install Long Animation Frames API into the QuickJS context
`crates/js/src/media_capabilities.rs:8` **fn** `install_media_capabilities_bindings` — Install Media Capabilities API bindings into the JS context
`crates/js/src/media_capture.rs:54` **fn** `set_audio_capture_provider` — Install the platform audio capture backend
`crates/js/src/media_capture.rs:85` **fn** `install_media_capture_bindings` — Install `__lumen_*` audio capture natives into the JS context
`crates/js/src/media_devices.rs:33` **fn** `install_media_devices_bindings` — Install MediaDevices API shim into the JS context
`crates/js/src/media_session.rs:36` **fn** `install_media_session_bindings` — Install MediaSession API shim into the JS context
`crates/js/src/media_stream_recording.rs:12` **fn** `init_media_stream_recording` — Install the MediaRecorder API stub into the JS context
`crates/js/src/navigation_api.rs:11` **fn** `install_navigation_api` — Install Navigation API into the JS context
`crates/js/src/navigator_bindings.rs:36` **struct** `NavigatorProfile` — High-entropy `navigator` / `screen` / timezone values exposed to JavaScript
`crates/js/src/navigator_bindings.rs:86` **fn** `set_navigator_profile` — Install a process-wide navigator profile (9F.1). Subsequent calls to the
`crates/js/src/navigator_bindings.rs:93` **fn** `current_navigator_profile` — Return the currently configured profile, or the default if none was set
`crates/js/src/navigator_bindings.rs:111` **fn** `install_navigator_bindings` — Install navigator/screen/timezone normalization shim into the JS context,
`crates/js/src/navigator_bindings.rs:117` **fn** `install_navigator_bindings_with` — Install the navigator shim using an explicit [`NavigatorProfile`], ignoring
`crates/js/src/network_log_bindings.rs:28` **struct** `NetworkLogRecord` — A single network request logged by JS, awaiting the shell's drain
`crates/js/src/network_log_bindings.rs:51` **fn** `enqueue` — Enqueue a network-log record. Public so non-JS engine paths can reuse the
`crates/js/src/network_log_bindings.rs:63` **fn** `take_network_log_records` — Drain and return all pending network-log records
`crates/js/src/network_log_bindings.rs:72` **fn** `install_network_log_bindings` — Install the `_lumen_log_network_request(method, url, status, duration_ms)`
`crates/js/src/notifications_bindings.rs:21` **struct** `NotificationRequest` — A notification request queued by `new Notification(...)` in JS
`crates/js/src/notifications_bindings.rs:34` **type** `NotificationQueue` — Shared queue of pending notification requests
`crates/js/src/notifications_bindings.rs:52` **fn** `install_notifications_bindings` — Install Web Notifications API globals into the JS context
`crates/js/src/notifications_bindings.rs:108` **fn** `drain_notifications` — Drain all pending notification requests from the queue
`crates/js/src/offscreen_canvas.rs:33` **struct** `OffscreenCanvas` — Wrapper class for OffscreenCanvas JS object
`crates/js/src/offscreen_canvas.rs:44` **fn** `new` — Create a new OffscreenCanvas with the given dimensions
`crates/js/src/offscreen_canvas.rs:57` **fn** `id` — Get the canvas ID (internal use only)
`crates/js/src/offscreen_canvas.rs:62` **fn** `width` — Get canvas width in CSS pixels
`crates/js/src/offscreen_canvas.rs:67` **fn** `height` — Get canvas height in CSS pixels
`crates/js/src/offscreen_canvas.rs:72` **fn** `transfer_to_image_bitmap` — Transfer pixel buffer to ImageBitmap and clear the canvas
`crates/js/src/offscreen_canvas.rs:113` **fn** `create_offscreen_from_pixels` — Create a new OffscreenCanvas pre-filled with existing RGBA8 pixel data
`crates/js/src/offscreen_canvas.rs:127` **fn** `flush_dirty` — Drain dirty offscreen canvases and return their RGBA buffers
`crates/js/src/offscreen_canvas.rs:151` **fn** `install_offscreen_canvas_bindings` — Install OffscreenCanvas bindings and JS shim into the QuickJS runtime
`crates/js/src/paint_worklet.rs:13` **struct** `PaintWorkletRegistry` — Maps worklet name (e.g. "my-paint") to its definition
`crates/js/src/paint_worklet.rs:18` **fn** `new`
`crates/js/src/paint_worklet.rs:23` **fn** `register` — Register a paint worklet definition
`crates/js/src/paint_worklet.rs:28` **fn** `get` — Look up a registered worklet by name
`crates/js/src/paint_worklet.rs:33` **fn** `all` — Get all registered worklets
`crates/js/src/paint_worklet.rs:38` **fn** `clear` — Clear all registrations (for tests)
`crates/js/src/paint_worklet.rs:44` **fn** `get_paint_worklet_registry` — Get the global paint worklet registry, initializing it if necessary
`crates/js/src/paint_worklet.rs:50` **struct** `PaintWorkletDef` — Definition of a registered paint worklet
`crates/js/src/paint_worklet.rs:61` **fn** `install_paint_worklet_api` — Install CSS.paintWorklet bindings into the JS context
`crates/js/src/payment_request.rs:18` **fn** `init_payment_request` — Install the Payment Request API stub into the JS context
`crates/js/src/periodic_sync.rs:19` **fn** `init_periodic_sync` — Install the Periodic Background Sync API stub into the JS context
`crates/js/src/permissions_policy.rs:13` **fn** `install_permissions_policy_bindings` — Install Permissions Policy JS bindings: `document.featurePolicy` and the
`crates/js/src/pip_bindings.rs:24` **enum** `PipRequest` — A picture-in-picture request emitted by the JS PiP API, awaiting the shell
`crates/js/src/pip_bindings.rs:49` **fn** `enqueue` — Enqueue a PiP request. Public so non-JS engine paths can reuse the channel
`crates/js/src/pip_bindings.rs:56` **fn** `take_pip_requests` — Drain and return all pending PiP requests
`crates/js/src/pip_bindings.rs:67` **fn** `install_pip_bindings` — Install the `_lumen_pip_enter(nid)` / `_lumen_pip_exit(nid)` native bindings
`crates/js/src/pointer_capture.rs:23` **fn** `install_pointer_capture_bindings` — Install `_lumen_set_capture_state` and `_lumen_release_capture_state` into the
`crates/js/src/pointer_lock.rs:42` **fn** `request_pointer_lock` — Request pointer lock for element with given node ID
`crates/js/src/pointer_lock.rs:50` **fn** `exit_pointer_lock` — Exit pointer lock
`crates/js/src/pointer_lock.rs:58` **fn** `set_movement` — Set relative mouse movement delta (called from shell DeviceEvent::MouseMotion)
`crates/js/src/pointer_lock.rs:67` **fn** `get_lock_state` — Get current pointer lock state: (is_locked, locked_element_nid, movement_x, movement_y)
`crates/js/src/pointer_lock.rs:78` **fn** `is_pointer_locked` — Check if pointer is locked
`crates/js/src/pointer_lock.rs:83` **fn** `get_locked_element_nid` — Get the DOM node ID of the locked element, or None
`crates/js/src/pointer_lock.rs:89` **fn** `take_movement` — Get the current movement delta and reset it to zero
`crates/js/src/pointer_lock.rs:100` **fn** `take_pending_grab` — Take pending OS cursor grab request, resetting it to None
`crates/js/src/presentation_api.rs:19` **fn** `install_presentation_api` — Install the Presentation API bindings into the JS context
`crates/js/src/push_api.rs:18` **fn** `init_push_api` — Install the Push API stub into the JS context
`crates/js/src/reporting_api.rs:13` **fn** `install_reporting_api_bindings` — Install Reporting API bindings into the JS context
`crates/js/src/sanitizer.rs:9` **fn** `install_sanitizer_bindings`
`crates/js/src/scheduler.rs:20` **fn** `install_scheduler_api` — Install the Scheduler API, TaskController, and TaskSignal into the JS context
`crates/js/src/screen_capture.rs:52` **fn** `set_screen_capture_provider` — Install the platform screen capture backend
`crates/js/src/screen_capture.rs:81` **fn** `install_screen_capture_bindings` — Install `__lumen_screen_capture_*` natives into the JS context
`crates/js/src/screen_orientation.rs:19` **fn** `install_screen_orientation_bindings` — Install Screen Orientation API shim into the JS context
`crates/js/src/scroll_snap_events.rs:23` **fn** `install_scroll_snap_events_bindings` — Install CSS Scroll Snap L2 events into the JS context
`crates/js/src/scroll_timeline.rs:27` **fn** `install_scroll_timeline_bindings` — Install CSS Scroll-Driven Animations L1 JS API into the QuickJS context
`crates/js/src/serial.rs:7` **fn** `install_serial_bindings` — Install WebSerial API bindings into the JS context
`crates/js/src/shape_detection.rs:8` **fn** `install_shape_detection_bindings`
`crates/js/src/shared_storage.rs:36` **fn** `install_shared_storage` — Install the Shared Storage API on `globalThis`
`crates/js/src/shared_worker.rs:42` **type** `SharedWorkerOutbox` — Outbound queue owned by a single `QuickJsRuntime` (page / context)
`crates/js/src/shared_worker.rs:86` **fn** `connect_shared_worker` — Connect a new client to the shared worker identified by `key`
`crates/js/src/shared_worker.rs:118` **fn** `post_to_shared_worker` — Forward a client `port.postMessage(data)` to the shared-worker thread
`crates/js/src/shared_worker.rs:128` **fn** `close_shared_worker_port` — Notify the shared worker that a client closed its port
`crates/js/src/shared_worker.rs:137` **fn** `drain_messages` — Drain all messages a runtime's shared-worker ports have received
`crates/js/src/shared_worker.rs:147` **fn** `install_shared_worker_bindings` — Install the `_lumen_sw_connect` / `_lumen_sw_post` / `_lumen_sw_close` native
`crates/js/src/soft_navigation.rs:22` **fn** `install_soft_navigation_api` — Install Soft Navigation Timing API stubs into the JS context
`crates/js/src/speculation_rules.rs:18` **fn** `install_speculation_rules_api` — Install the Speculation Rules API stubs into the JS context
`crates/js/src/speech.rs:84` **fn** `install_speech_bindings` — Install the Web Speech API into `ctx`
`crates/js/src/sri.rs:10` **enum** `SriAlgorithm` — Hash algorithm accepted in the `integrity` attribute
`crates/js/src/sri.rs:17` **struct** `SriToken` — One parsed token from an `integrity` string
`crates/js/src/sri.rs:27` **fn** `parse_integrity_metadata` — Parses a space-separated list of integrity tokens
`crates/js/src/sri.rs:56` **fn** `check_sri` — Returns `true` if `body` passes the SRI check encoded in `integrity`
`crates/js/src/storage_buckets.rs:18` **fn** `init_storage_buckets` — Install the Storage Buckets API into the JS context
`crates/js/src/storage_manager.rs:19` **fn** `install_storage_manager_bindings` — Install StorageManager API bindings into the JS context
`crates/js/src/surface_api.rs:29` **fn** `install_surface_api_protection` — Install Layer 1 surface API protection into the JS context
`crates/js/src/svg.rs:8` **fn** `install_svg_bindings` — Install SVG DOM API bindings into the JS context
`crates/js/src/sw_worker.rs:24` **fn** `spawn_sw_worker` — Spawn a Service Worker execution thread
`crates/js/src/tc39_proposals.rs:31` **fn** `install_tc39_proposals` — Install all TC39 Stage 4 proposal shims into the given QuickJS context
`crates/js/src/temporal_api.rs:36` **fn** `install_temporal_api` — Install the Temporal API shim into the given QuickJS context
`crates/js/src/text_track_store.rs:22` **struct** `CueData` — One WebVTT cue exposed to JS as a `TextTrackCue` / `VTTCue`
`crates/js/src/text_track_store.rs:35` **struct** `TextTrackData` — One `<track>` element exposed to JS as a `TextTrack`
`crates/js/src/text_track_store.rs:56` **struct** `TextTrackStore` — Per-`<video>` text-track snapshot, keyed by DOM node index (`el.__nid__`)
`crates/js/src/text_track_store.rs:66` **fn** `tracks_json` — Serialize the tracks of one `<video>` to a JSON array string
`crates/js/src/text_track_store.rs:110` **fn** `set_text_track_store` — Install the text-track store from the shell
`crates/js/src/text_track_store.rs:115` **fn** `get_text_track_store` — Return a clone of the installed store, or `None` in headless/CI mode
`crates/js/src/topics_api.rs:24` **fn** `install_topics_api` — Install Topics API bindings into the JS context
`crates/js/src/trusted_types.rs:18` **fn** `install_trusted_types_bindings` — Installs `window.trustedTypes`, the three trusted value classes and
`crates/js/src/typed_om_api.rs:20` **fn** `install_typed_om_api` — Install CSS Typed OM API bindings
`crates/js/src/ua_client_hints.rs:11` **fn** `install_ua_client_hints_bindings` — Install User-Agent Client Hints bindings into the JS context
`crates/js/src/url_pattern.rs:14` **fn** `install_url_pattern_api` — Install URL Pattern API into the JS context
`crates/js/src/video_bindings.rs:46` **fn** `install_video_bindings` — Install HTMLVideoElement Phase 1 bindings into the JS context
`crates/js/src/video_gif_store.rs:36` **struct** `VideoPlaybackState` — Per-`<video>` playback timing, stored by the shell after a GIF is decoded
`crates/js/src/video_gif_store.rs:57` **fn** `current_ms` — Playback position in ms at a given real-clock instant
`crates/js/src/video_gif_store.rs:66` **fn** `is_ended` — Whether playback has naturally ended (finite loop count exhausted)
`crates/js/src/video_gif_store.rs:75` **fn** `duration_secs` — Duration in seconds exposed to JS as `video.duration`
`crates/js/src/video_gif_store.rs:84` **fn** `freeze` — Snapshot `position_ms` to the current playback position and clear epoch
`crates/js/src/video_gif_store.rs:96` **struct** `VideoGifStore` — Shared state for all `<video>`-element GIF animations, keyed by DOM node index
`crates/js/src/video_gif_store.rs:116` **fn** `set_video_gif_store` — Install the video GIF store from the shell
`crates/js/src/video_gif_store.rs:121` **fn** `get_video_gif_store` — Return a clone of the installed store, or `None` in headless/CI mode
`crates/js/src/video_pip.rs:23` **fn** `install_video_pip_api` — Install Video Picture-in-Picture API into the JS context
`crates/js/src/view_transitions.rs:19` **enum** `ViewTransitionEvent` — Events emitted by `document.startViewTransition` and drained by the shell
`crates/js/src/view_transitions.rs:90` **fn** `install_view_transition_bindings` — Register `_lumen_vt_begin` / `_lumen_vt_end` / `_lumen_vt_cancel` native functions
`crates/js/src/virtual_keyboard.rs:15` **fn** `install_virtual_keyboard_bindings` — Install Virtual Keyboard API bindings into the JS context
`crates/js/src/wake_lock.rs:43` **fn** `set_wake_lock_provider` — Install the platform wake-lock backend
`crates/js/src/wake_lock.rs:84` **fn** `install_wake_lock_bindings` — Install the Screen Wake Lock API bindings into the JS context
`crates/js/src/wasm/interp.rs:21` **struct** `Trap` — A runtime trap (maps to `WebAssembly.RuntimeError` on the JS side)
`crates/js/src/wasm/interp.rs:32` **trait** `HostImports` — Host import callback surface. The interpreter calls this when WASM invokes
`crates/js/src/wasm/interp.rs:39` **struct** `NullHost` — A no-op host that traps on any imported call. Used when a module declares no
`crates/js/src/wasm/interp.rs:50` **struct** `Instance` — An instantiated module: linear memory, globals, table, and a reference back
`crates/js/src/wasm/interp.rs:86` **fn** `new` — Instantiate a decoded module
`crates/js/src/wasm/interp.rs:184` **fn** `run_start` — Run the module's `start` function, if any
`crates/js/src/wasm/interp.rs:192` **fn** `export_func_index` — Resolve an exported function's index by name
`crates/js/src/wasm/interp.rs:203` **fn** `mem_pages` — Current memory size in pages
`crates/js/src/wasm/interp.rs:209` **fn** `mem_grow` — Grow memory by `delta` pages; return the previous page count, or -1 on
`crates/js/src/wasm/interp.rs:225` **fn** `invoke` — Invoke any function by index (imported → host, defined → interpret)
`crates/js/src/wasm/mod.rs:70` **fn** `validate` — `true` if `bytes` decode as a valid module this engine can run
`crates/js/src/wasm/mod.rs:75` **fn** `compile` — Decode and store a module; returns its registry id
`crates/js/src/wasm/mod.rs:98` **fn** `clear_registry` — Drop all compiled modules and live instances on this thread, releasing the
`crates/js/src/wasm/mod.rs:108` **fn** `module_exports_json` — JSON descriptor of a module's exports (consumed by the JS shim to build the
`crates/js/src/wasm/mod.rs:130` **fn** `module_imports_json` — JSON descriptor of a module's imports (consumed by the JS shim to resolve
`crates/js/src/wasm/mod.rs:156` **fn** `instantiate` — Instantiate a compiled module
`crates/js/src/wasm/mod.rs:247` **fn** `func_signature` — Parameter and result value types of an exported function (by its function
`crates/js/src/wasm/mod.rs:263` **fn** `call_typed` — Call an exported function with already-typed arguments, returning typed
`crates/js/src/wasm/mod.rs:294` **fn** `mem_size` — Current memory size of an instance, in 64 KiB pages
`crates/js/src/wasm/mod.rs:305` **fn** `mem_grow` — Grow an instance's memory by `delta` pages; previous size or -1 on failure
`crates/js/src/wasm/mod.rs:316` **fn** `mem_read` — Copy `len` bytes of an instance's linear memory starting at `offset`
`crates/js/src/wasm/mod.rs:334` **fn** `mem_write` — Write `bytes` into an instance's linear memory at `offset`. Returns `false`
`crates/js/src/wasm/mod.rs:354` **fn** `mem_read_all` — Full linear-memory snapshot of an instance (every page). Returns an empty
`crates/js/src/wasm/mod.rs:367` **fn** `global_value` — Read an exported global's current value (typed). Returns `None` if the
`crates/js/src/wasm/mod.rs:379` **fn** `global_set_value` — Set a mutable exported global from a typed value (coerced to its declared
`crates/js/src/wasm/mod.rs:497` **fn** `func_param_count` — Number of parameters for an exported function index (used by the shim to
`crates/js/src/wasm/parser.rs:17` **type** `DecodeResult` — Result of decoding, with a human-readable error for `CompileError`
`crates/js/src/wasm/parser.rs:21` **enum** `BlockType` — Block signature for `block`/`loop`/`if`
`crates/js/src/wasm/parser.rs:34` **enum** `Instr` — A decoded instruction. Numeric/comparison/conversion ops with no immediate
`crates/js/src/wasm/parser.rs:113` **enum** `ImportKind` — What an import binds to
`crates/js/src/wasm/parser.rs:126` **struct** `Import` — A single import entry
`crates/js/src/wasm/parser.rs:137` **enum** `ExportKind` — The export kind tag
`crates/js/src/wasm/parser.rs:146` **struct** `Export` — A single export entry
`crates/js/src/wasm/parser.rs:157` **struct** `GlobalDef` — A defined global: its type, mutability, and initialiser expression
`crates/js/src/wasm/parser.rs:168` **struct** `FuncBody` — A decoded function body: extra locals plus its instruction stream
`crates/js/src/wasm/parser.rs:178` **struct** `DataSegment` — An active data segment: target memory offset expression + raw bytes
`crates/js/src/wasm/parser.rs:189` **struct** `ElemSegment` — An active element segment for a table: offset expression + function indices
`crates/js/src/wasm/parser.rs:200` **struct** `Module` — A fully decoded module ready for instantiation
`crates/js/src/wasm/parser.rs:235` **fn** `func_type` — Look up the function type for any function index (imported or defined)
`crates/js/src/wasm/parser.rs:370` **fn** `check_header` — Validate the WASM magic + version header without a full decode (used by
`crates/js/src/wasm/parser.rs:375` **fn** `parse_module` — Decode a full module image
`crates/js/src/wasm/simd.rs:107` **fn** `shuffle` — `i8x16.shuffle`: pick 16 lanes from the concatenation of `a` (lanes 0..15)
`crates/js/src/wasm/simd.rs:123` **fn** `lane_op` — `*.extract_lane*` / `*.replace_lane` (`0xFD` sub-opcodes 21..=34)
`crates/js/src/wasm/simd.rs:170` **fn** `exec_simd` — Execute a SIMD op with no immediate beyond the sub-opcode (the `Instr::Simd`
`crates/js/src/wasm/value.rs:11` **enum** `ValType` — A WebAssembly value type
`crates/js/src/wasm/value.rs:32` **fn** `from_byte` — Decode a value type from its binary tag byte. Returns `None` for an
`crates/js/src/wasm/value.rs:46` **fn** `default_value` — The zero/default runtime value for this type (used to initialise locals)
`crates/js/src/wasm/value.rs:64` **enum** `Value` — A runtime WebAssembly value
`crates/js/src/wasm/value.rs:86` **fn** `as_i32` — Interpret this value as `i32`, trapping representation is the caller's
`crates/js/src/wasm/value.rs:94` **fn** `as_i64` — Interpret this value as `i64`
`crates/js/src/wasm/value.rs:102` **fn** `as_f32` — Interpret this value as `f32`
`crates/js/src/wasm/value.rs:110` **fn** `as_f64` — Interpret this value as `f64`
`crates/js/src/wasm/value.rs:120` **fn** `as_v128` — Interpret this value as the raw 16 bytes of a `v128`. Returns all-zero
`crates/js/src/wasm/value.rs:128` **fn** `val_type` — The value type of this runtime value
`crates/js/src/wasm/value.rs:143` **struct** `FuncType` — A function signature: parameter types followed by result types
`crates/js/src/wasm/value.rs:155` **struct** `Limits` — Min/max limits shared by memories and tables (in pages for memory, in
`crates/js/src/web_audio.rs:18` **fn** `install_web_audio_api` — Install the Web Audio API into the JS context
`crates/js/src/web_codecs.rs:16` **fn** `install_webcodecs_bindings` — Install WebCodecs API JS shim
`crates/js/src/web_locks.rs:14` **fn** `install_web_locks_bindings` — Install the Web Locks API bindings into the JS context
`crates/js/src/web_midi.rs:16` **fn** `install_web_midi_api` — Install Web MIDI API bindings into the JS context
`crates/js/src/webassembly.rs:186` **fn** `install_webassembly_bindings` — Install WebAssembly API bindings into the JS context
`crates/js/src/webgl_bindings.rs:25` **fn** `install_webgl_bindings` — Install WebGL fingerprint shim into the JS context
`crates/js/src/webgl_canvas.rs:57` **fn** `install_webgl_canvas` — Install functional WebGL bindings into the JS context
`crates/js/src/webgpu.rs:58` **fn** `install_webgpu_bindings` — Install the WebGPU API bindings into the JS context
`crates/js/src/webhid.rs:5` **fn** `install_webhid_bindings`
`crates/js/src/webrtc_stub.rs:27` **fn** `install_webrtc_bindings` — Install the WebRTC mDNS-only stub into the JS context
`crates/js/src/webtransport.rs:5` **fn** `install_webtransport_bindings`
`crates/js/src/webusb.rs:5` **fn** `install_webusb_bindings`
`crates/js/src/webxr.rs:7` **fn** `install_webxr_bindings` — Install WebXR Device API bindings into the JS context
`crates/js/src/window_management.rs:21` **fn** `install_window_management_api` — Install Window Management API shim into the JS context
`crates/js/src/worker.rs:29` **enum** `WorkerInMsg` — Message sent from the main JS thread to a worker thread
`crates/js/src/worker.rs:39` **struct** `WorkerHandle` — Live handle to a spawned worker thread
`crates/js/src/worker.rs:51` **type** `WorkerRegistry` — All live Worker instances for the current page, keyed by worker ID
`crates/js/src/worker.rs:57` **type** `WorkerMessageQueue` — Outbound message queue: messages posted by worker threads to the main thread
`crates/js/src/worker.rs:64` **type** `WorkerBlobStore` — Shared blob store: blob URL → decoded script text
`crates/js/src/worker.rs:72` **fn** `spawn_worker` — Spawn a new worker thread that evaluates `script` and waits for messages
`crates/js/src/worker.rs:105` **fn** `post_to_worker` — Send a JSON-serialized message to a live worker thread
`crates/js/src/worker.rs:115` **fn** `terminate_worker` — Terminate a worker and remove it from the registry
`crates/js/src/worker.rs:124` **fn** `drain_messages` — Drain all pending messages sent from worker threads to the main thread
`crates/js/src/worker.rs:134` **fn** `install_worker_bindings` — Install native bindings (`_lumen_create_worker`, `_lumen_worker_post`,
`crates/js/src/xhr.rs:38` **fn** `install_xhr_bindings` — Install the XMLHttpRequest API into the QuickJS context

## lumen-knowledge  (59 symbols)

`crates/knowledge/src/fts.rs:28` **struct** `SearchHit` — Результат полнотекстового поиска
`crates/knowledge/src/fts.rs:43` **struct** `HistoryFts` — FTS5-индекс над `(url, title, text)`. Открывается отдельной БД-файлом
`crates/knowledge/src/fts.rs:54` **fn** `open`
`crates/knowledge/src/fts.rs:60` **fn** `open_in_memory`
`crates/knowledge/src/fts.rs:87` **fn** `index` — Добавить или обновить запись в индексе. `rowid` обычно совпадает
`crates/knowledge/src/fts.rs:111` **fn** `unindex` — Удалить запись по rowid
`crates/knowledge/src/fts.rs:129` **fn** `search` — Полнотекстовый поиск по `text` с ранжированием bm25. `query` —
`crates/knowledge/src/fts.rs:167` **fn** `clear` — Полная очистка индекса
`crates/knowledge/src/history.rs:28` **struct** `HistoryWithFts` — История с интегрированным FTS-индексом. Оборачивает
`crates/knowledge/src/history.rs:36` **fn** `open` — Открыть или создать FTS-индекс истории. Обычно открывается
`crates/knowledge/src/history.rs:42` **fn** `open_in_memory` — Открыть in-memory FTS-индекс (для тестов)
`crates/knowledge/src/history.rs:52` **fn** `index_text` — Индексировать запись истории в FTS. Обычно вызывается после
`crates/knowledge/src/history.rs:58` **fn** `unindex` — Удалить запись из FTS-индекса. Обычно вызывается после
`crates/knowledge/src/history.rs:69` **fn** `search` — Полнотекстовый поиск по истории. Возвращает совпадения,
`crates/knowledge/src/history.rs:75` **fn** `clear` — Очистить весь FTS-индекс. Обычно вызывается при
`crates/knowledge/src/history.rs:85` **fn** `record_visit_with_text` — Записать визит в History и автоматически индексировать текст в FTS
`crates/knowledge/src/history.rs:106` **fn** `delete_with_fts` — Удалить запись из History и автоматически удалить из FTS
`crates/knowledge/src/notes.rs:21` **struct** `Note` — Одна заметка пользователя
`crates/knowledge/src/notes.rs:34` **struct** `NoteSearchHit`
`crates/knowledge/src/notes.rs:41` **struct** `Notes`
`crates/knowledge/src/notes.rs:52` **fn** `open`
`crates/knowledge/src/notes.rs:58` **fn** `open_in_memory`
`crates/knowledge/src/notes.rs:110` **fn** `add` — Создать заметку. Возвращает её id
`crates/knowledge/src/notes.rs:132` **fn** `update` — Обновить selection / context / comment по id. created_at не меняется
`crates/knowledge/src/notes.rs:152` **fn** `delete` — Удалить заметку по id
`crates/knowledge/src/notes.rs:163` **fn** `get` — Получить заметку по id
`crates/knowledge/src/notes.rs:182` **fn** `list_for_url` — Все заметки для конкретного URL (для восстановления highlight-
`crates/knowledge/src/notes.rs:204` **fn** `recent` — Последние N заметок (по убыванию created_at)
`crates/knowledge/src/notes.rs:226` **fn** `search` — Полнотекстовый поиск по selection + comment
`crates/knowledge/src/notes.rs:268` **fn** `count` — Общее число заметок
`crates/knowledge/src/notes.rs:280` **fn** `clear` — Удалить все заметки. Триггеры notes_ad чистят FTS индекс
`crates/knowledge/src/open_tabs.rs:36` **struct** `OpenTabHit` — Результат поиска по открытым вкладкам
`crates/knowledge/src/open_tabs.rs:54` **struct** `OpenTabsIndex` — Живой in-memory FTS5-индекс над открытыми вкладками. Не персистится —
`crates/knowledge/src/open_tabs.rs:67` **fn** `new` — Создать пустой in-memory индекс. По дизайну (§12.4) on-disk варианта
`crates/knowledge/src/open_tabs.rs:88` **fn** `index_tab` — Добавить или обновить вкладку в индексе. `tab_id` — живой shell tab id;
`crates/knowledge/src/open_tabs.rs:112` **fn** `remove_tab` — Убрать вкладку из индекса (при её закрытии). No-op, если вкладки нет
`crates/knowledge/src/open_tabs.rs:129` **fn** `search` — Полнотекстовый поиск по `(url, title, text)` среди открытых вкладок,
`crates/knowledge/src/open_tabs.rs:164` **fn** `count` — Текущее число проиндексированных открытых вкладок
`crates/knowledge/src/open_tabs.rs:176` **fn** `clear` — Очистить весь индекс (например, при выходе или сбросе сессии)
`crates/knowledge/src/read_later.rs:23` **enum** `ReadStatus` — Статус read-later записи
`crates/knowledge/src/read_later.rs:53` **struct** `ReadLaterEntry` — Одна сохранённая страница
`crates/knowledge/src/read_later.rs:69` **struct** `ReadLaterSearchHit`
`crates/knowledge/src/read_later.rs:75` **struct** `ReadLater`
`crates/knowledge/src/read_later.rs:86` **fn** `open`
`crates/knowledge/src/read_later.rs:92` **fn** `open_in_memory`
`crates/knowledge/src/read_later.rs:153` **fn** `save` — Сохранить новую страницу или обновить существующую. Возвращает id
`crates/knowledge/src/read_later.rs:206` **fn** `set_status` — Обновить статус записи (mark read / archive)
`crates/knowledge/src/read_later.rs:220` **fn** `touch` — Обновить last_accessed (вызывается при открытии офлайн-копии)
`crates/knowledge/src/read_later.rs:233` **fn** `get`
`crates/knowledge/src/read_later.rs:252` **fn** `get_by_url`
`crates/knowledge/src/read_later.rs:272` **fn** `list_by_status` — Список записей с указанным статусом, сортировка по saved_at DESC
`crates/knowledge/src/read_later.rs:296` **fn** `search` — Полнотекстовый поиск
`crates/knowledge/src/read_later.rs:346` **fn** `delete`
`crates/knowledge/src/read_later.rs:356` **fn** `count`
`crates/knowledge/src/store.rs:33` **struct** `DefaultKnowledgeStore` — SQLite-backed [`KnowledgeStore`]. One instance per browser process
`crates/knowledge/src/store.rs:52` **fn** `open` — Open (or create) a `DefaultKnowledgeStore` in `base_dir`
`crates/knowledge/src/store.rs:65` **fn** `open_in_memory` — Create an in-memory `DefaultKnowledgeStore` (tests only)
`crates/knowledge/src/store.rs:77` **fn** `read_later` — Direct access to the read-later store for status / touch operations
`crates/knowledge/src/store.rs:83` **fn** `notes` — Direct access to the notes store for URL-based note listing and

## lumen-layout  (611 symbols)

`crates/engine/layout/src/anchor.rs:44` **enum** `AnchorSide` — Which edge or point of an anchor element the `anchor()` function references
`crates/engine/layout/src/anchor.rs:73` **enum** `InsetAreaKeyword` — Single-axis `inset-area` keyword, as defined in §5.2 of the spec
`crates/engine/layout/src/anchor.rs:104` **enum** `AnchorScope` — Value of the CSS `anchor-scope` property (CSS Anchor Positioning L1 §2.1)
`crates/engine/layout/src/anchor.rs:121` **enum** `AnchorSizeDimension` — Which dimension the `anchor-size()` function references
`crates/engine/layout/src/anchor.rs:144` **struct** `AnchorSizeFunc` — Parsed `anchor-size(<anchor-el>? <anchor-size>)` value stored in ComputedStyle
`crates/engine/layout/src/anchor.rs:163` **struct** `AnchorRegistry` — Map from CSS `anchor-name` value (e.g. `"--foo"`) to the border-box [`Rect`]
`crates/engine/layout/src/anchor.rs:170` **struct** `AnchorEntry` — One registered anchor element
`crates/engine/layout/src/anchor.rs:188` **fn** `get` — Look up an anchor by CSS name (e.g. `"--tooltip-anchor"`)
`crates/engine/layout/src/anchor.rs:199` **fn** `get_scoped` — Scope-aware lookup: returns the anchor entry only if it is visible to a
`crates/engine/layout/src/anchor.rs:212` **fn** `is_empty` — True when the registry has no anchors
`crates/engine/layout/src/anchor.rs:228` **fn** `collect_anchors`
`crates/engine/layout/src/anchor.rs:253` **fn** `register_anchor` — Register an element as a named anchor (globally visible, no scope restriction)
`crates/engine/layout/src/anchor.rs:261` **fn** `register_anchor_scoped` — Register an element as a named anchor with optional scope restriction
`crates/engine/layout/src/anchor.rs:288` **fn** `resolve_anchor_function`
`crates/engine/layout/src/anchor.rs:330` **fn** `resolve_anchor_size`
`crates/engine/layout/src/anchor.rs:359` **enum** `AxisSize` — The positioned element's used size on one axis, as seen by the position-area
`crates/engine/layout/src/anchor.rs:382` **struct** `AnchoredPosition` — Resolved inset-area position for an anchored element
`crates/engine/layout/src/anchor.rs:407` **fn** `resolve_inset_area`
`crates/engine/layout/src/anchor.rs:429` **fn** `resolve_inset_area_scoped`
`crates/engine/layout/src/animation.rs:36` **struct** `AnimatedStyle` — Sparse animated values for one element — scheduler output per node per frame
`crates/engine/layout/src/animation.rs:49` **struct** `AnimationFrame` — Output of `AnimationScheduler::tick` — per-node animated values for one frame
`crates/engine/layout/src/animation.rs:61` **fn** `merge` — Merge `other` into `self`; `other` values take precedence per property
`crates/engine/layout/src/animation.rs:80` **fn** `merge_from` — Extract only compositor-offloadable properties (opacity, transform)
`crates/engine/layout/src/animation.rs:99` **fn** `to_compositor_frame` — Extract only compositor-offloadable properties (opacity, transform)
`crates/engine/layout/src/animation.rs:128` **struct** `CompositorOverride` — Compositor-offloadable overrides for one element
`crates/engine/layout/src/animation.rs:142` **struct** `CompositorAnimFrame` — Per-frame compositor overrides — output of `AnimationFrame::to_compositor_frame`
`crates/engine/layout/src/animation.rs:148` **fn** `is_empty`
`crates/engine/layout/src/animation.rs:152` **fn** `get`
`crates/engine/layout/src/animation.rs:160` **struct** `KeyframeStyle` — Sparse style extracted from one `@keyframes` frame's declarations
`crates/engine/layout/src/animation.rs:169` **fn** `parse_keyframe_style` — Parse the `declarations` of one `@keyframes` frame into a [`KeyframeStyle`]
`crates/engine/layout/src/animation.rs:207` **enum** `AnimValue` — Анимируемое значение. Phase 0: восемь вариантов — Number / Length / Color /
`crates/engine/layout/src/animation.rs:243` **trait** `AnimationInterpolator` — Trait для интерполяции пары computed values
`crates/engine/layout/src/animation.rs:257` **struct** `NoopInterpolator` — Stub-реализация: step-half для любой пары значений
`crates/engine/layout/src/animation.rs:288` **struct** `LinearInterpolator` — Реальная импл §5.2 — linear для Number / Length (same-unit) / Color
`crates/engine/layout/src/animation.rs:776` **struct** `AnimationScheduler` — CSS Animations L1 §3 — scheduler that maps `@keyframes` to interpolated
`crates/engine/layout/src/animation.rs:782` **fn** `new`
`crates/engine/layout/src/animation.rs:792` **fn** `sync` — Register or refresh animations for `node` based on its computed style
`crates/engine/layout/src/animation.rs:813` **fn** `remove_node` — Remove all animation state for `node` (e.g. when the node is removed from the DOM)
`crates/engine/layout/src/animation.rs:823` **fn** `tick` — Compute per-node animated style overrides for the current frame
`crates/engine/layout/src/animation.rs:1146` **struct** `TransitionScheduler` — CSS Transitions L1 §2 — detects property value changes and interpolates
`crates/engine/layout/src/animation.rs:1160` **fn** `new`
`crates/engine/layout/src/animation.rs:1169` **fn** `set_auto_height` — Store the resolved auto-height for `node` from the last layout pass
`crates/engine/layout/src/animation.rs:1182` **fn** `sync` — Detect value changes between `old` and `new` style for properties listed
`crates/engine/layout/src/animation.rs:1288` **fn** `remove_node` — Remove all transition state for `node` (called when node leaves DOM)
`crates/engine/layout/src/animation.rs:1326` **fn** `tick` — Compute interpolated style overrides for the current frame
`crates/engine/layout/src/box_tree.rs:172` **struct** `ViewBox` — SVG `viewBox="min-x min-y width height"` attribute. Maps SVG user-unit space
`crates/engine/layout/src/box_tree.rs:187` **struct** `PreserveAspectRatio` — SVG `preserveAspectRatio` attribute for aspect-ratio preservation
`crates/engine/layout/src/box_tree.rs:198` **enum** `SvgAlignX` — SVG preserveAspectRatio horizontal alignment
`crates/engine/layout/src/box_tree.rs:209` **enum** `SvgAlignY` — SVG preserveAspectRatio vertical alignment
`crates/engine/layout/src/box_tree.rs:220` **enum** `SvgMeetOrSlice` — SVG preserveAspectRatio meet-or-slice mode
`crates/engine/layout/src/box_tree.rs:230` **enum** `SvgTextAnchor` — SVG `text-anchor` attribute for text horizontal alignment
`crates/engine/layout/src/box_tree.rs:243` **enum** `SvgDominantBaseline` — SVG `dominant-baseline` attribute for text vertical alignment
`crates/engine/layout/src/box_tree.rs:267` **enum** `SvgBaselineShift` — SVG 1.1 §10.9.2 / CSS Inline Layout L3 §5.2 — `baseline-shift`. Vertical shift
`crates/engine/layout/src/box_tree.rs:284` **struct** `SvgTransform` — SVG transformation data from the `transform` presentation attribute
`crates/engine/layout/src/box_tree.rs:292` **fn** `identity` — Creates an identity transform (no transformation)
`crates/engine/layout/src/box_tree.rs:297` **fn** `translate` — Creates a translation transform
`crates/engine/layout/src/box_tree.rs:302` **fn** `compose` — Multiplies this transform by another, composing them
`crates/engine/layout/src/box_tree.rs:317` **fn** `transform_point` — Applies this transform to a point (x, y)
`crates/engine/layout/src/box_tree.rs:326` **enum** `SvgShapeKind` — Geometric primitive for an SVG shape element in SVG user units (before viewBox scaling)
`crates/engine/layout/src/box_tree.rs:343` **enum** `FormControlKind` — Вид form control — используется в `BoxKind::FormControl` для paint-специализаций
`crates/engine/layout/src/box_tree.rs:430` **fn** `collect_selectlist_label` — Collect the selected `<option>` label from a `<selectlist>` element
`crates/engine/layout/src/box_tree.rs:468` **fn** `is_selectlist` — Returns `true` when `node` is a `<selectlist>` element (Customizable Select)
`crates/engine/layout/src/box_tree.rs:560` **fn** `is_open_details` — Returns `true` when `id` is a `<details>` element with the `open` attribute set
`crates/engine/layout/src/box_tree.rs:1446` **struct** `ImageRequest` — Запрос на предзагрузку изображения: URL после picking-а по
`crates/engine/layout/src/box_tree.rs:1464` **fn** `collect_image_requests` — Обходит DOM и возвращает запросы на загрузку для всех `<img>`-элементов
`crates/engine/layout/src/box_tree.rs:1484` **fn** `collect_background_image_requests` — Обходит готовое layout-дерево и возвращает уникальные URL-ы из
`crates/engine/layout/src/box_tree.rs:1599` **struct** `LayoutBox`
`crates/engine/layout/src/box_tree.rs:1630` **struct** `InlineSegment` — Отрезок inline-контента с собственным стилем (до layout)
`crates/engine/layout/src/box_tree.rs:1670` **enum** `PseudoKind` — Marks an inline segment as the target of a CSS structural pseudo-element
`crates/engine/layout/src/box_tree.rs:1688` **struct** `InlineFrag` — Позиционированный текстовый фрагмент в строке (после layout)
`crates/engine/layout/src/box_tree.rs:1722` **enum** `BoxKind`
`crates/engine/layout/src/box_tree.rs:2529` **fn** `layout` — Lay out a document without a text measurer. For tests and headless dump modes
`crates/engine/layout/src/box_tree.rs:2554` **fn** `layout_measured` — Layout without a text measurer. For tests and headless modes; uses `layout_measured_hyp` with `dark_mode=false`
`crates/engine/layout/src/box_tree.rs:2567` **fn** `layout_measured_hyp` — Layout with a real hyphenation provider (for `hyphens: auto`)
`crates/engine/layout/src/box_tree.rs:2615` **fn** `lay_out_incremental` — Incremental re-layout pass: skips clean subtrees, re-lays out only dirty ones
`crates/engine/layout/src/box_tree.rs:2652` **fn** `layout_streaming_incremental` — Streaming incremental layout (PH1-2b)
`crates/engine/layout/src/box_tree.rs:2749` **fn** `build_iframe_document` — Parse inline HTML from an `<iframe srcdoc="...">` attribute (HTML spec §4.8.5)
`crates/engine/layout/src/box_tree.rs:2822` **fn** `canvas_background_color` — CSS Backgrounds §3.11.1 — the canvas background color
`crates/engine/layout/src/box_tree.rs:9036` **fn** `resolve_auto_fill_fit_count` — CSS Grid Layout L3 §9 — Resolve `repeat(auto-fill|auto-fit, <track-list>)` count
`crates/engine/layout/src/box_tree.rs:9219` **fn** `measure_text_w` — Measures text width (letter_spacing applied between each character)
`crates/engine/layout/src/box_tree.rs:9238` **fn** `measure_text_w_families` — Как [`measure_text_w`], но учитывает CSS `font-family` каскад
`crates/engine/layout/src/box_tree.rs:9268` **fn** `measure_text_w_varied` — Как [`measure_text_w_families`], но учитывает CSS `font-variation-settings`
`crates/engine/layout/src/box_tree.rs:10240` **fn** `apply_container_styles` — CSS Container Queries L1: second-pass after layout
`crates/engine/layout/src/color_mix.rs:38` **enum** `MixColorSpace` — CSS Color L5 §10.2 — interpolation color space for `color-mix()`
`crates/engine/layout/src/color_mix.rs:63` **fn** `from_css` — Parse a CSS `color-mix()` interpolation space identifier (case-insensitive)
`crates/engine/layout/src/color_mix.rs:80` **fn** `is_polar` — Returns `true` if this space has a hue (polar) axis
`crates/engine/layout/src/color_mix.rs:96` **fn** `mix_colors` — CSS Color L5 §10.2 — mix two sRGB colors in the given interpolation space
`crates/engine/layout/src/color_mix.rs:630` **fn** `relative_origin_channels` — CSS Color L5 §4.1 — channel values of a relative-color origin color
`crates/engine/layout/src/content_visibility.rs:50` **fn** `set_cv_scroll` — Set the root scroll offset used by the relevance check for the next layout
`crates/engine/layout/src/content_visibility.rs:56` **fn** `set_cv_relevant` — Install the set of nodes the shell considers relevant (ratchet set)
`crates/engine/layout/src/content_visibility.rs:69` **fn** `take_cv_skipped` — Drain the skip records of the last layout pass: `(node, collapsed_top_y)`,
`crates/engine/layout/src/counters.rs:44` **type** `CounterSnapshot` — Per-element counter stacks snapshot
`crates/engine/layout/src/counters.rs:49` **enum** `QuoteSlot` — Generated-content slot of an element that can carry `open-quote` /
`crates/engine/layout/src/counters.rs:63` **struct** `CounterMap` — Document-order snapshot of CSS generated-content state
`crates/engine/layout/src/counters.rs:74` **fn** `counters` — Returns the counter snapshot for `id`, if any
`crates/engine/layout/src/counters.rs:80` **fn** `quote_depths` — Returns the ordered quote-depth indices for the given `(id, slot)`'s
`crates/engine/layout/src/counters.rs:156` **fn** `precompute_counters` — Build a `CounterMap` by walking the DOM in pre-order
`crates/engine/layout/src/counters.rs:270` **fn** `format_counter` — Format a counter integer value according to the given `list-style-type` keyword
`crates/engine/layout/src/counters.rs:337` **enum** `CounterSystem` — Numbering algorithm for a `@counter-style` rule — CSS Counter Styles L3 §4
`crates/engine/layout/src/counters.rs:356` **struct** `RangeBound` — Counter range bound: `None` means ±infinite (CSS Counter Styles L3 §5)
`crates/engine/layout/src/counters.rs:365` **enum** `CounterRange` — Range descriptor value (CSS Counter Styles L3 §5)
`crates/engine/layout/src/counters.rs:374` **struct** `CounterStyleDef` — Parsed `@counter-style` rule — CSS Counter Styles L3 §2
`crates/engine/layout/src/counters.rs:412` **type** `CounterStyleRegistry` — Maps counter style names to their parsed `CounterStyleDef`
`crates/engine/layout/src/counters.rs:415` **fn** `build_counter_style_registry` — Build a `CounterStyleRegistry` from all `@counter-style` rules in a stylesheet
`crates/engine/layout/src/counters.rs:694` **fn** `format_counter_with_registry` — Format a counter value using the registry (custom `@counter-style`) first,
`crates/engine/layout/src/counters.rs:864` **fn** `resolve_counter_value` — CSS Counter Styles L3 §2 — format counter `n` using a resolved `CounterStyleDef`
`crates/engine/layout/src/counters.rs:877` **fn** `build_list_marker_text` — CSS Lists L3 §2.1 — canonical wiring point for `list-style-type` + `@counter-style`
`crates/engine/layout/src/field_sizing.rs:47` **fn** `field_sizing_content_intrinsic` — Computes content-based intrinsic dimensions for an HTML form control under
`crates/engine/layout/src/font_palette.rs:20` **struct** `PaletteColorOverride` — Resolved CPAL color override: `(palette_index, color)`
`crates/engine/layout/src/font_palette.rs:38` **fn** `resolve_font_palette_overrides` — Resolves `@font-palette-values` overrides for a given element
`crates/engine/layout/src/font_palette.rs:70` **struct** `ResolvedFontPalette` — Output of [`resolve_font_palette_overrides`]
`crates/engine/layout/src/font_palette.rs:81` **enum** `FontPaletteSelection` — Renderer-facing `font-palette` selection, copied into `DrawText`
`crates/engine/layout/src/font_palette.rs:101` **fn** `palette_selection` — Maps a computed style to the `DrawText` palette selection
`crates/engine/layout/src/hyphenation.rs:31` **struct** `SoftHyphenPoint` — A potential soft-hyphen break position within a word's *display* string
`crates/engine/layout/src/hyphenation.rs:63` **fn** `collect_hyphen_points` — Collect soft-hyphen break points for `word` under the given `hyphens` policy
`crates/engine/layout/src/image_gating.rs:42` **fn** `gate_image_requests` — Returns the set of [`NodeId`]s for `BoxKind::Image` boxes whose bounding
`crates/engine/layout/src/image_set.rs:32` **struct** `ImageSetOption` — A single parsed candidate inside an `image-set()` expression
`crates/engine/layout/src/image_set.rs:48` **struct** `SupportedTypes` — Describes which MIME types the engine can decode
`crates/engine/layout/src/image_set.rs:58` **fn** `all` — Phase 0 — accept every MIME type unconditionally
`crates/engine/layout/src/image_set.rs:64` **fn** `from_list` — Explicit list of accepted MIME types (case-insensitive comparison)
`crates/engine/layout/src/image_set.rs:70` **fn** `accepts` — Returns `true` if `mime_type` is accepted
`crates/engine/layout/src/image_set.rs:251` **fn** `parse_image_set` — Parses an `image-set()` / `-webkit-image-set()` expression into a list of
`crates/engine/layout/src/image_set.rs:269` **fn** `select_image_set_candidate` — CSS Images L4 §5 — selects the best candidate from a parsed `image-set()`
`crates/engine/layout/src/image_set.rs:298` **fn** `select_image_set_url` — Convenience wrapper: parses `value` and immediately selects the best URL
`crates/engine/layout/src/incremental.rs:38` **struct** `DirtyBits` — Bitflag tracking which aspects of a [`LayoutBox`] need recalculation
`crates/engine/layout/src/incremental.rs:52` **fn** `is_clean` — Returns `true` when no bits are set (layout is up-to-date)
`crates/engine/layout/src/incremental.rs:56` **fn** `is_dirty` — Returns `true` when any bit is set
`crates/engine/layout/src/incremental.rs:60` **fn** `contains` — Returns `true` when all bits in `rhs` are also set in `self`
`crates/engine/layout/src/incremental.rs:79` **fn** `translate_subtree` — Translate every rect in `b`'s subtree by `(dx, dy)` without re-running layout
`crates/engine/layout/src/incremental.rs:95` **fn** `mark_dirty` — Mark `node_id` as needing full re-layout
`crates/engine/layout/src/incremental.rs:117` **fn** `mark_dirty_set` — Mark all nodes in `node_ids` as dirty (one tree walk per node)
`crates/engine/layout/src/incremental.rs:128` **fn** `clear_dirty` — Recursively clear all dirty bits throughout `b`'s entire subtree
`crates/engine/layout/src/incremental.rs:145` **fn** `mark_subtree_dirty` — Mark every box in `b`'s subtree as [`DirtyBits::SELF_SIZE`]
`crates/engine/layout/src/incremental.rs:168` **fn** `graft_geometry` — Reuse laid-out geometry from `prev` for unchanged subtrees of the fresh tree
`crates/engine/layout/src/inert.rs:46` **fn** `is_inert` — Returns `true` if `node` or any of its ancestors carries the `inert`
`crates/engine/layout/src/inert.rs:66` **struct** `InertRegion` — A rectangular region in the layout tree that belongs to an inert subtree
`crates/engine/layout/src/inert.rs:87` **fn** `collect_inert_regions` — Walk the layout tree and return every inert root box as an [`InertRegion`]
`crates/engine/layout/src/lib.rs:159` **struct** `SelectionHighlight` — Computed `::selection` highlight data — passed to the paint layer so it can
`crates/engine/layout/src/lib.rs:175` **trait** `TextMeasurer` — Интерфейс измерения ширины символов для line wrapping
`crates/engine/layout/src/lib.rs:242` **enum** `ClickableKind` — Classification of an interactive element found during layout-tree traversal
`crates/engine/layout/src/lib.rs:263` **struct** `ClickableElement` — An interactive element with its screen-space bounding rect
`crates/engine/layout/src/lib.rs:284` **fn** `collect_clickable_elements` — Collect all interactive elements from the layout tree in document order
`crates/engine/layout/src/lib.rs:517` **struct** `StickyBox` — Snapshot of a `position: sticky` element captured after normal-flow layout
`crates/engine/layout/src/lib.rs:545` **fn** `collect_sticky_boxes` — Collect all `position: sticky` elements from the layout tree in document order
`crates/engine/layout/src/lib.rs:604` **fn** `compute_sticky_offset` — Compute the visual offset `(dx, dy)` in CSS px to apply to a sticky element
`crates/engine/layout/src/lib.rs:677` **struct** `SnapPoint` — A single snap area inside a [`SnapContainer`]
`crates/engine/layout/src/lib.rs:695` **struct** `SnapContainer` — A scroll container that participates in CSS Scroll Snap L1
`crates/engine/layout/src/lib.rs:728` **fn** `collect_snap_containers` — Collect all scroll containers that participate in CSS Scroll Snap L1
`crates/engine/layout/src/lib.rs:907` **fn** `find_snap_target` — Find the nearest snap target for a scroll gesture
`crates/engine/layout/src/lib.rs:1006` **struct** `SnapTargets` — The snap areas a container is currently snapped to, one per axis
`crates/engine/layout/src/lib.rs:1029` **fn** `find_snapped_nodes` — Determine which snap areas a container is snapped to at scroll offset `scroll`
`crates/engine/layout/src/lib.rs:1079` **struct** `ScrollContainer` — A scrollable overflow container collected from the layout tree
`crates/engine/layout/src/lib.rs:1111` **fn** `collect_scroll_containers` — Collect all `overflow: scroll` / `overflow: auto` containers from the layout tree
`crates/engine/layout/src/lib.rs:1167` **fn** `overscroll_should_propagate` — CSS Overscroll Behavior L1 §3 — decide whether a scroll delta a container
`crates/engine/layout/src/lib.rs:1215` **fn** `collect_computed_styles` — Walks the layout tree and returns a map of `NodeId index → CSS property map`
`crates/engine/layout/src/lib.rs:1241` **fn** `set_scroll_position` — Update the scroll position of a node in the layout tree
`crates/engine/layout/src/lib.rs:1274` **fn** `collect_view_transition_names` — Find the innermost scroll container whose `clip_rect` contains `(x, y)`
`crates/engine/layout/src/lib.rs:1311` **fn** `collect_view_transition_groups`
`crates/engine/layout/src/lib.rs:1337` **fn** `find_scroll_container_at` — `x` and `y` are in CSS px, document-relative (same coordinate space as
`crates/engine/layout/src/masonry.rs:33` **fn** `lay_out_masonry` — Greedy waterfall masonry placement algorithm (CSS Grid L3 §14)
`crates/engine/layout/src/masonry.rs:64` **fn** `min_track_idx` — Returns the index of the track with the minimum running height
`crates/engine/layout/src/mathml.rs:28` **enum** `MathStyle` — CSS `math-style` (MathML Core §2.1.1). Inherited. Initial: `Normal`
`crates/engine/layout/src/mathml.rs:44` **fn** `math_depth_scale` — Relative font scale between two `math-depth` levels
`crates/engine/layout/src/mathml.rs:50` **enum** `MathmlElementKind` — Represents the type of MathML element and its visual role
`crates/engine/layout/src/mathml.rs:76` **struct** `MathmlBox` — MathML box: container for mathematical notation
`crates/engine/layout/src/mathml.rs:94` **fn** `new` — Create a new MathML box for a given element type
`crates/engine/layout/src/mathml.rs:106` **fn** `with_denominator` — Set denominator boxes for mfrac elements
`crates/engine/layout/src/mathml.rs:112` **fn** `with_annotation` — Set annotation (exponent/subscript) boxes
`crates/engine/layout/src/mathml.rs:118` **fn** `with_annotation_scale` — Set the scaling factor for annotations (superscript/subscript)
`crates/engine/layout/src/mathml.rs:124` **fn** `with_math_style` — Set the CSS `math-style` (taken from the element's `ComputedStyle`)
`crates/engine/layout/src/mathml.rs:140` **fn** `collect_mathml_structure` — Collect MathML element structure from a DOM node
`crates/engine/layout/src/mathml.rs:174` **fn** `lay_out_mathml` — Layout algorithm for MathML content
`crates/engine/layout/src/motion_path.rs:30` **struct** `MotionTransform` — Result of resolving a motion offset along an `offset-path`
`crates/engine/layout/src/motion_path.rs:53` **fn** `resolve_motion_transform` — Resolve the motion transform for an element with `offset-path: path(...)`
`crates/engine/layout/src/motion_path.rs:559` **fn** `flatten_path_to_polygon` — Flattens an SVG path `d` string into a polygon (CSS Shapes L1 §4 `path()`)
`crates/engine/layout/src/page.rs:22` **struct** `MarginBoxTextFragment` — Text fragment within a margin-box after layout
`crates/engine/layout/src/page.rs:49` **enum** `MarginBoxPosition` — Position of a margin-box relative to the page box
`crates/engine/layout/src/page.rs:72` **fn** `all` — All 16 margin-box positions in layout order
`crates/engine/layout/src/page.rs:88` **fn** `css_name` — CSS property name for this margin-box in @page rules
`crates/engine/layout/src/page.rs:103` **fn** `is_corner` — Is this a corner box?
`crates/engine/layout/src/page.rs:114` **fn** `is_horizontal_edge` — Is this a horizontal edge box (top or bottom)?
`crates/engine/layout/src/page.rs:119` **fn** `is_vertical_edge` — Is this a vertical edge box (left or right)?
`crates/engine/layout/src/page.rs:129` **struct** `PageProperties` — Computed properties for a page from matching @page rules
`crates/engine/layout/src/page.rs:155` **fn** `default_a4` — Create default page properties (A4 size, 2cm margins)
`crates/engine/layout/src/page.rs:172` **fn** `content_width` — Content box width: page width minus left and right margins
`crates/engine/layout/src/page.rs:177` **fn** `content_height` — Content box height: page height minus top and bottom margins
`crates/engine/layout/src/page.rs:182` **fn** `compute_orientation` — Update orientation based on width/height ratio
`crates/engine/layout/src/page.rs:196` **struct** `MarginBox` — Margin-box with layout information
`crates/engine/layout/src/page.rs:223` **fn** `new` — Create a new margin-box at a given position
`crates/engine/layout/src/page.rs:236` **fn** `with_content` — Assign generated content to this margin-box
`crates/engine/layout/src/page.rs:247` **fn** `layout_text` — Layout text content in this margin-box with word-wrapping
`crates/engine/layout/src/page.rs:352` **struct** `PageBox` — Complete page structure with margin-boxes and page properties
`crates/engine/layout/src/page.rs:365` **fn** `new` — Create a new page with computed properties
`crates/engine/layout/src/page.rs:378` **fn** `apply_margin_box_content` — Apply content functions to margin-boxes and generate text
`crates/engine/layout/src/page.rs:407` **fn** `layout_margin_boxes` — Layout all 16 margin-boxes based on page properties
`crates/engine/layout/src/page.rs:524` **fn** `get_margin_box` — Get a margin-box by position
`crates/engine/layout/src/page.rs:529` **fn** `get_margin_box_mut` — Mutably get a margin-box by position
`crates/engine/layout/src/page.rs:544` **fn** `match_page_rules` — Matches @page rules for a given page number and applies properties
`crates/engine/layout/src/page.rs:614` **fn** `compute_page_properties` — Computes page properties from matching @page rules
`crates/engine/layout/src/page.rs:654` **struct** `PageCounters` — Counter value for page numbering and related counters
`crates/engine/layout/src/page.rs:664` **fn** `new` — Create a new counter set with the page counter initialized to 1 (page 1)
`crates/engine/layout/src/page.rs:672` **fn** `get` — Get the value of a named counter
`crates/engine/layout/src/page.rs:677` **fn** `set` — Set the value of a named counter
`crates/engine/layout/src/page.rs:682` **fn** `increment` — Increment a counter by 1
`crates/engine/layout/src/page.rs:689` **fn** `reset` — Reset a counter to a specified value
`crates/engine/layout/src/page.rs:699` **enum** `ContentFunction` — Represents a content function used in margin-box content generation
`crates/engine/layout/src/page.rs:800` **fn** `resolve_content_function` — Resolves a content function to its text representation
`crates/engine/layout/src/page.rs:831` **fn** `create_page_number_footer` — Common margin-box content preset: page number at bottom center
`crates/engine/layout/src/page.rs:846` **fn** `create_page_number_header` — Common margin-box content preset: page number at top center
`crates/engine/layout/src/page.rs:861` **fn** `create_header_footer` — Common margin-box content preset: custom header and footer
`crates/engine/layout/src/pagination.rs:23` **struct** `PaginationContext` — Parameters for print pagination
`crates/engine/layout/src/pagination.rs:47` **fn** `content_width` — Content box width: page width minus left and right margins
`crates/engine/layout/src/pagination.rs:52` **fn** `content_height` — Content box height: page height minus top and bottom margins
`crates/engine/layout/src/pagination.rs:57` **fn** `content_origin` — Top-left corner of content box within page
`crates/engine/layout/src/pagination.rs:67` **struct** `Page` — A single page with positioned content
`crates/engine/layout/src/pagination.rs:88` **struct** `PageFragment` — A fragment of layout tree content positioned on a page
`crates/engine/layout/src/pagination.rs:112` **fn** `paginate` — Pagination algorithm: split LayoutBox tree into pages
`crates/engine/layout/src/property_trees.rs:40` **struct** `PropertyTreeNodeId` — Идентификатор узла в любом из четырёх деревьев. Уникален в пределах своего
`crates/engine/layout/src/property_trees.rs:46` **fn** `raw`
`crates/engine/layout/src/property_trees.rs:55` **struct** `Mat4` — 4×4 матрица в column-major порядке (как принято в OpenGL / WebGPU)
`crates/engine/layout/src/property_trees.rs:66` **fn** `is_identity`
`crates/engine/layout/src/property_trees.rs:71` **fn** `translation_2d` — 2D translation. Z и W колонки остаются identity
`crates/engine/layout/src/property_trees.rs:79` **fn** `scale_2d` — 2D scale. CSS Transforms L1 §13.4
`crates/engine/layout/src/property_trees.rs:89` **fn** `rotate_2d` — 2D rotation вокруг Z (положительный угол — против часовой стрелки в
`crates/engine/layout/src/property_trees.rs:101` **fn** `skew_x` — `skewX(angle)` — сдвигает X пропорционально Y. CSS Transforms L1 §13.7
`crates/engine/layout/src/property_trees.rs:108` **fn** `skew_y` — `skewY(angle)` — сдвигает Y пропорционально X
`crates/engine/layout/src/property_trees.rs:116` **fn** `from_2d_affine` — 2D affine `matrix(a, b, c, d, e, f)` (CSS Transforms L1 §13.10) →
`crates/engine/layout/src/property_trees.rs:129` **fn** `multiply` — Композиция матриц: `lhs * rhs`. Для column-major OpenGL-конвенции
`crates/engine/layout/src/property_trees.rs:155` **fn** `invert_2d_affine` — Инверсия 2D affine-матрицы. Возвращает `None`, если матрица
`crates/engine/layout/src/property_trees.rs:181` **fn** `transform_point_2d` — Применяет 2D affine часть матрицы к точке `(x, y)`. Z/W колонки
`crates/engine/layout/src/property_trees.rs:205` **fn** `perspective` — CSS `perspective(<length>)` — матрица перспективной проекции с фокусным
`crates/engine/layout/src/property_trees.rs:213` **fn** `translate_3d` — 3D translation. CSS `translate3d(tx, ty, tz)` / `translateZ(tz)`
`crates/engine/layout/src/property_trees.rs:223` **fn** `scale_3d` — 3D scale. CSS `scale3d(sx, sy, sz)` / `scaleZ(sz)`
`crates/engine/layout/src/property_trees.rs:234` **fn** `rotate_x` — Поворот вокруг оси X. CSS `rotateX(theta)`, `theta` в радианах
`crates/engine/layout/src/property_trees.rs:248` **fn** `rotate_y` — Поворот вокруг оси Y. CSS `rotateY(theta)`, `theta` в радианах
`crates/engine/layout/src/property_trees.rs:262` **fn** `rotate_z` — Поворот вокруг оси Z. CSS `rotateZ(theta)` ≡ `rotate(theta)`
`crates/engine/layout/src/property_trees.rs:271` **fn** `rotate_3d` — CSS `rotate3d(x, y, z, theta)` — поворот вокруг произвольной оси
`crates/engine/layout/src/property_trees.rs:303` **fn** `from_3d` — CSS `matrix3d(m11, …, m44)` — 16 значений в column-major порядке
`crates/engine/layout/src/property_trees.rs:313` **fn** `project_point` — Применяет полную 4×4 матрицу к точке `(x, y, z)` и выполняет
`crates/engine/layout/src/property_trees.rs:331` **fn** `project_point_z` — Как [`project_point`](Self::project_point), но возвращает и
`crates/engine/layout/src/property_trees.rs:352` **fn** `transform_z` — Возвращает только трансформированную z-координату точки `(x, y, z)`
`crates/engine/layout/src/property_trees.rs:362` **fn** `is_2d_affine` — `true`, если матрица — чистое 2D affine-преобразование (Z/W-строки
`crates/engine/layout/src/property_trees.rs:386` **struct** `TransformNode` — Узел TransformTree. Хранит локальный transform; accumulated transform
`crates/engine/layout/src/property_trees.rs:396` **struct** `TransformTree` — Дерево transform-преобразований. Корень — identity
`crates/engine/layout/src/property_trees.rs:402` **fn** `empty` — Sprint 0 stub: только root с identity
`crates/engine/layout/src/property_trees.rs:412` **fn** `root`
`crates/engine/layout/src/property_trees.rs:419` **struct** `ScrollNode` — Узел ScrollTree. Хранит scrollable rect и текущий scroll offset
`crates/engine/layout/src/property_trees.rs:432` **struct** `ScrollTree`
`crates/engine/layout/src/property_trees.rs:437` **fn** `empty`
`crates/engine/layout/src/property_trees.rs:449` **fn** `root`
`crates/engine/layout/src/property_trees.rs:457` **struct** `EffectNode` — Узел EffectTree. Хранит opacity / filter / blend-mode — всё, что
`crates/engine/layout/src/property_trees.rs:484` **struct** `EffectTree`
`crates/engine/layout/src/property_trees.rs:489` **fn** `empty`
`crates/engine/layout/src/property_trees.rs:495` **fn** `root`
`crates/engine/layout/src/property_trees.rs:503` **struct** `ClipNode` — Узел ClipTree. Хранит clip rectangle в локальных координатах (т.е
`crates/engine/layout/src/property_trees.rs:512` **struct** `ClipTree`
`crates/engine/layout/src/property_trees.rs:517` **fn** `empty`
`crates/engine/layout/src/property_trees.rs:527` **fn** `root`
`crates/engine/layout/src/property_trees.rs:537` **struct** `PropertyTrees` — 4-deep property trees — единая поверхность, которую layout
`crates/engine/layout/src/property_trees.rs:546` **fn** `empty` — Sprint 0 stub: все 4 дерева — empty roots
`crates/engine/layout/src/property_trees.rs:557` **fn** `build_stub` — Совместимость с Sprint 0: пустые root-only деревья. Используется
`crates/engine/layout/src/property_trees.rs:584` **fn** `build` — Построение property trees из layout-дерева (P1 п.2B)
`crates/engine/layout/src/property_trees.rs:615` **fn** `compute_local_transform` — Вычислить локальную transform-матрицу элемента. CSS Transforms L1 §13:
`crates/engine/layout/src/property_trees.rs:680` **fn** `forward_box_transform` — Forward-матрица бокса в viewport-координатах. CSS Transforms L1 §13:
`crates/engine/layout/src/property_trees.rs:773` **fn** `transform_fns_to_matrix` — Build the forward transform matrix from a list of TransformFn with a pivot point
`crates/engine/layout/src/ruby.rs:25` **enum** `RubyPosition` — CSS Ruby L1 §4 — `ruby-position`. Inherited. Initial: `over`
`crates/engine/layout/src/ruby.rs:38` **enum** `RubyAlign` — CSS Ruby L1 §4 — `ruby-align`. Inherited. Initial: `space-around`
`crates/engine/layout/src/ruby.rs:55` **enum** `RubyMerge` — CSS Ruby L1 §4 — `ruby-merge`. Inherited. Initial: `separate`
`crates/engine/layout/src/ruby.rs:70` **struct** `RubyBox` — Ruby box: base text with optional annotation
`crates/engine/layout/src/ruby.rs:87` **fn** `new` — Create a new Ruby box with default Over positioning
`crates/engine/layout/src/ruby.rs:103` **fn** `from_style` — Create a Ruby box taking `ruby-position` / `ruby-align` / `ruby-merge`
`crates/engine/layout/src/ruby.rs:119` **fn** `with_position` — Set the ruby text position
`crates/engine/layout/src/ruby.rs:125` **fn** `with_align` — Set the annotation alignment mode
`crates/engine/layout/src/ruby.rs:131` **fn** `with_merge` — Set the annotation pairing mode
`crates/engine/layout/src/ruby.rs:137` **fn** `with_inter_char_spacing` — Set inter-character spacing in em units
`crates/engine/layout/src/ruby.rs:152` **fn** `lay_out_ruby` — Layout algorithm for ruby annotations
`crates/engine/layout/src/rule_index.rs:21` **struct** `RuleIndex` — Subject-keyed rule index for the top-level `rules` vec of a stylesheet
`crates/engine/layout/src/rule_index.rs:97` **fn** `empty` — Empty index — used as the initial value of the thread-local cache
`crates/engine/layout/src/rule_index.rs:110` **fn** `build` — Builds an index over the top-level rules of `sheet`
`crates/engine/layout/src/rule_index.rs:154` **fn** `candidates` — Returns the deduplicated, sorted candidate rule indices for a node
`crates/engine/layout/src/scroll_timeline.rs:26` **enum** `ScrollAxis` — Selects which scroll axis drives a timeline
`crates/engine/layout/src/scroll_timeline.rs:40` **struct** `Viewport` — Viewport dimensions used during progress resolution
`crates/engine/layout/src/scroll_timeline.rs:53` **struct** `ScrollTimeline` — Scroll progress timeline (CSS `scroll()` function / named `scroll-timeline`)
`crates/engine/layout/src/scroll_timeline.rs:66` **struct** `ViewTimeline` — View progress timeline (CSS `view()` function / named `view-timeline`)
`crates/engine/layout/src/scroll_timeline.rs:79` **struct** `NamedScrollTimeline` — Named scroll timeline resolved from the layout tree
`crates/engine/layout/src/scroll_timeline.rs:94` **struct** `NamedViewTimeline` — Named view timeline resolved from the layout tree
`crates/engine/layout/src/scroll_timeline.rs:161` **fn** `resolve_scroll_progress` — Resolve the scroll progress fraction `[0.0, 1.0]` for a [`ScrollTimeline`]
`crates/engine/layout/src/scroll_timeline.rs:225` **fn** `resolve_view_progress` — Resolve the view progress fraction `[0.0, 1.0]` for a [`ViewTimeline`]
`crates/engine/layout/src/scroll_timeline.rs:270` **fn** `collect_named_scroll_timelines` — Collect all named scroll timelines defined in the layout tree
`crates/engine/layout/src/scroll_timeline.rs:295` **fn** `collect_named_view_timelines` — Collect all named view timelines defined in the layout tree
`crates/engine/layout/src/selection.rs:16` **fn** `caret_at_point` — Find the caret position (DOM node + UTF-8 byte offset) closest to a pixel point
`crates/engine/layout/src/selection.rs:95` **fn** `selection_rects` — Compute pixel rectangles that cover the selected `range` within the layout tree
`crates/engine/layout/src/selector_query.rs:42` **fn** `find_descendant_by_selector` — Finds the first descendant LayoutBox matching the given selector
`crates/engine/layout/src/selector_query.rs:63` **fn** `find_all_descendants_by_selector` — Finds all descendant LayoutBoxes matching the given selector
`crates/engine/layout/src/selector_query.rs:75` **fn** `style_snapshot` — Returns the computed style snapshot for this box
`crates/engine/layout/src/selector_query.rs:88` **struct** `ComputedStyleSnapshot` — Flat snapshot of the most-queried CSS properties for in-process testing
`crates/engine/layout/src/selector_query.rs:220` **fn** `find_box_by_selector` — Returns a reference to the first `LayoutBox` in document order whose
`crates/engine/layout/src/selector_query.rs:278` **fn** `computed_style_by_selector` — Returns the computed style snapshot of the first matching `LayoutBox`
`crates/engine/layout/src/selector_query.rs:294` **fn** `find_all_by_selector` — Returns references to **all** `LayoutBox`es (in document order) whose
`crates/engine/layout/src/selector_query.rs:335` **fn** `query_all` — Returns all [`NodeId`]s in the document that match `sel`
`crates/engine/layout/src/selector_query.rs:372` **fn** `matches_selector` — Returns `true` if `node` matches **any** selector in `sel`
`crates/engine/layout/src/selector_query.rs:544` **fn** `computed_style_to_map` — Serialises a [`ComputedStyle`] to a CSS property → resolved-value map
`crates/engine/layout/src/selector_query.rs:877` **fn** `computed_style_json` — Serialises a [`ComputedStyle`] into a deterministic JSON object string
`crates/engine/layout/src/selector_query.rs:899` **fn** `computed_style_json_by_selector` — Like [`computed_style_by_selector`] but returns the full computed-style JSON
`crates/engine/layout/src/selector_query.rs:914` **struct** `MatchedRule` — One CSS rule that matched a specific DOM node
`crates/engine/layout/src/selector_query.rs:934` **fn** `matched_rules_for_node` — Return all CSS rules from `sheet` whose selectors match `node` in `doc`
`crates/engine/layout/src/snapshot.rs:63` **fn** `serialize_layout_tree` — Корневой entry-point: рекурсивно сериализует всё дерево
`crates/engine/layout/src/stacking.rs:29` **struct** `StackingContextId` — Идентификатор stacking context-а. Монотонно растёт от 0; 0 = root
`crates/engine/layout/src/stacking.rs:35` **fn** `raw`
`crates/engine/layout/src/stacking.rs:48` **enum** `PaintPhase` — CSS 2.1 Appendix E — 7-уровневый порядок отрисовки внутри stacking context
`crates/engine/layout/src/stacking.rs:91` **struct** `StackingContext` — Один stacking context: владелец-box + z-index + ссылки на дочерние
`crates/engine/layout/src/stacking.rs:103` **struct** `StackingTree` — Плоское представление stacking-дерева: вектор `StackingContext` + индексы
`crates/engine/layout/src/stacking.rs:110` **fn** `empty_root` — Дерево с единственным root-контекстом без детей. Используется в
`crates/engine/layout/src/stacking.rs:132` **fn** `build` — Построение stacking-дерева из layout-дерева
`crates/engine/layout/src/stacking.rs:154` **fn** `root`
`crates/engine/layout/src/stacking.rs:186` **fn** `creates_stacking_context` — CSS Positioned Layout L3 §9.10 — создаёт ли элемент собственный
`crates/engine/layout/src/stacking.rs:257` **fn** `box_can_own_stacking_context` — Анонимные / неучаствующие в layout box-ы не имеют DOM-элемента, к
`crates/engine/layout/src/stacking.rs:299` **struct** `PaintOrder` — Painting order — линейная последовательность пар `(StackingContextId,
`crates/engine/layout/src/stacking.rs:319` **fn** `from_tree` — Строит painting order по CSS 2.1 Appendix E + CSS Painting Order L3 §3
`crates/engine/layout/src/stacking.rs:327` **fn** `len`
`crates/engine/layout/src/stacking.rs:331` **fn** `is_empty`
`crates/engine/layout/src/starting_style.rs:56` **struct** `StartingStyleTracker` — Tracks nodes that are "entering" — i.e. have just been inserted into the
`crates/engine/layout/src/starting_style.rs:63` **fn** `new` — Create an empty tracker
`crates/engine/layout/src/starting_style.rs:76` **fn** `mark_entered` — Mark `node` as "just entered" the document (or became visible)
`crates/engine/layout/src/starting_style.rs:82` **fn** `is_entered` — Returns `true` when `node` was marked via [`Self::mark_entered`] and
`crates/engine/layout/src/starting_style.rs:91` **fn** `consume` — Remove `node` from the "entered" set
`crates/engine/layout/src/starting_style.rs:99` **fn** `remove` — Remove all state for `node` — called when the node leaves the DOM
`crates/engine/layout/src/starting_style.rs:128` **fn** `resolve_starting_style` — Look up `@starting-style` declarations that match `node` in `sheet`
`crates/engine/layout/src/style.rs:53` **fn** `invalidate_rule_idx_cache` — Invalidate the thread-local rule-index cache
`crates/engine/layout/src/style.rs:81` **fn** `set_shadow_sheets` — Install the per-shadow-host author stylesheets for the current layout pass
`crates/engine/layout/src/style.rs:87` **fn** `clear_shadow_sheets` — Drop all installed shadow-tree stylesheets (used by tests to avoid leaking
`crates/engine/layout/src/style.rs:92` **enum** `Display`
`crates/engine/layout/src/style.rs:133` **enum** `TextAlign`
`crates/engine/layout/src/style.rs:149` **enum** `TextAlignLast` — CSS Text L3 §7.2 — `text-align-last`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:174` **enum** `Direction` — CSS Writing Modes L3 §2.1 — `direction: ltr | rtl`. Inherited
`crates/engine/layout/src/style.rs:186` **struct** `BoxShadow` — CSS Backgrounds L3 §4.6 — спецификация одной тени бокса
`crates/engine/layout/src/style.rs:200` **struct** `TextShadow` — CSS Text Decoration L3 §4 — спецификация одной тени текста
`crates/engine/layout/src/style.rs:213` **enum** `Cursor` — CSS UI L4 §8.1 — `cursor`. Inherited
`crates/engine/layout/src/style.rs:260` **enum** `TextOverflow` — CSS UI L4 §10.1 — `text-overflow`. Не наследуется
`crates/engine/layout/src/style.rs:275` **enum** `Overflow` — CSS Overflow L3 — `overflow`. Не наследуется
`crates/engine/layout/src/style.rs:292` **enum** `Visibility` — CSS Display L3 §4 — `visibility`. Inherited
`crates/engine/layout/src/style.rs:308` **enum** `WhiteSpace` — CSS Text Module L3 §3.1 / L4 §2.1 — `white-space`. Inherited
`crates/engine/layout/src/style.rs:327` **fn** `preserves_whitespace` — True when whitespace (tabs, newlines) is preserved rather than collapsed
`crates/engine/layout/src/style.rs:332` **fn** `is_nowrap` — True when line wrapping is disabled (lines only break at forced breaks)
`crates/engine/layout/src/style.rs:339` **fn** `preserves_newlines` — True when segment breaks (`\n`) in the source are preserved as forced
`crates/engine/layout/src/style.rs:349` **fn** `combine` — CSS Text L4 §2.1 — recombine the two longhand components into the
`crates/engine/layout/src/style.rs:370` **fn** `collapse_component` — Decompose the legacy `white-space` value into its L4 collapse component
`crates/engine/layout/src/style.rs:381` **fn** `wrap_component` — Decompose the legacy `white-space` value into its L4 wrap component
`crates/engine/layout/src/style.rs:392` **enum** `WhiteSpaceCollapse` — CSS Text Module L4 §3.1 — `white-space-collapse`. Inherited
`crates/engine/layout/src/style.rs:409` **fn** `parse`
`crates/engine/layout/src/style.rs:429` **enum** `TextTransform` — CSS Text Module L3 §3.4 — `text-transform`. Inherited
`crates/engine/layout/src/style.rs:442` **fn** `apply` — Применяет преобразование к строке. Не аллоцирует, если transform = None
`crates/engine/layout/src/style.rs:475` **enum** `FontStyle` — CSS Fonts Module L4: `font-style: normal | italic | oblique`. Inherited
`crates/engine/layout/src/style.rs:490` **enum** `FontVariant` — CSS Fonts L4 §6 — `font-variant` (упрощённый Phase 0). Inherited
`crates/engine/layout/src/style.rs:501` **enum** `FontOpticalSizing` — CSS Fonts L4 §7.12 — `font-optical-sizing`. Inherited
`crates/engine/layout/src/style.rs:524` **struct** `FontStretch` — CSS Fonts Module L4 §2.5 — `font-stretch`. Inherited
`crates/engine/layout/src/style.rs:561` **struct** `FontWeight` — CSS Fonts Module L4 §2.4 — `font-weight`. Inherited
`crates/engine/layout/src/style.rs:567` **fn** `is_bold`
`crates/engine/layout/src/style.rs:583` **struct** `FontVariationSetting` — CSS Fonts L4 §7 — одна запись `font-variation-settings`
`crates/engine/layout/src/style.rs:596` **struct** `FontFeatureSetting` — CSS Fonts L3 §6 — одна запись `font-feature-settings`
`crates/engine/layout/src/style.rs:614` **struct** `TextDecorationLine` — Набор активных линий `text-decoration` для элемента
`crates/engine/layout/src/style.rs:636` **enum** `TextDecorationStyle` — CSS Text Decoration L3 §2.2 — `text-decoration-style`. Стиль штриха
`crates/engine/layout/src/style.rs:649` **fn** `parse` — Парсит одиночный keyword. Возвращает `None` для невалидных и для
`crates/engine/layout/src/style.rs:679` **enum** `TextDecorationThickness` — CSS Text Decoration L3 §2.3 — `text-decoration-thickness`. Толщина
`crates/engine/layout/src/style.rs:700` **enum** `TextDecorationSkipInk` — CSS Text Decoration L4 §3.5 — `text-decoration-skip-ink`. Controls whether
`crates/engine/layout/src/style.rs:721` **enum** `TextEmphasisStyle` — CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Форма emphasis-marks
`crates/engine/layout/src/style.rs:736` **enum** `TextEmphasisShape`
`crates/engine/layout/src/style.rs:753` **enum** `TextEmphasisPosition` — CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Сторона
`crates/engine/layout/src/style.rs:762` **fn** `is_over`
`crates/engine/layout/src/style.rs:772` **enum** `TextUnderlinePosition` — CSS Text Decoration L3 §6.1 / L4 §5.1 — `text-underline-position`
`crates/engine/layout/src/style.rs:791` **enum** `ForcedColorAdjust` — CSS Color Adjustment L1 §4 — `forced-color-adjust`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:807` **enum** `ColorScheme` — CSS Color Adjustment L1 §3 — `color-scheme`. Inherited. Initial: `Normal`
`crates/engine/layout/src/style.rs:841` **fn** `used_dark` — CSS Color Adjustment L1 §2.3 — резолвит «used color scheme» элемента
`crates/engine/layout/src/style.rs:851` **struct** `Color`
`crates/engine/layout/src/style.rs:883` **struct** `ColorFloat` — CSS Color L4 §10 — цветовое пространство для wide-gamut значений
`crates/engine/layout/src/style.rs:894` **fn** `to_srgb_color` — Конвертирует в sRGB u8, применяя матрицу цветового пространства и гамму
`crates/engine/layout/src/style.rs:926` **fn** `to_linear_srgb` — Линейные sRGB-каналы [0..1] для прямой передачи в GPU без квантизации
`crates/engine/layout/src/style.rs:959` **fn** `to_display` — Конвертирует `ColorFloat` в линейные каналы заданного `target` цветового
`crates/engine/layout/src/style.rs:1114` **enum** `SystemColor` — CSS Color Level 4 §6.2 — system color keywords. Stored as a `Copy` enum to
`crates/engine/layout/src/style.rs:1166` **fn** `parse` — Parse a CSS system color keyword (case-insensitive). Returns `None` for
`crates/engine/layout/src/style.rs:1226` **fn** `resolve_color` — Resolve to a concrete sRGB `Color` for the given used color scheme
`crates/engine/layout/src/style.rs:1240` **enum** `CssColor` — CSS Color L4 §4.2 — типизированное цветовое значение каскада
`crates/engine/layout/src/style.rs:1252` **fn** `resolve` — Разрешает значение в sRGB u8 Color. `Wide` конвертируется через матрицу
`crates/engine/layout/src/style.rs:1263` **fn** `to_color_opt` — Конвертирует в `Color`, минуя `current_color`. `CurrentColor` → `None`
`crates/engine/layout/src/style.rs:1273` **fn** `resolve_linear` — Линейные sRGB-каналы для прямой передачи в GPU
`crates/engine/layout/src/style.rs:1307` **enum** `SvgPaint` — SVG Presentation §11.2 — `fill` / `stroke` paint value (`<paint>` type)
`crates/engine/layout/src/style.rs:1326` **fn** `resolve` — Resolves the paint value to a concrete `Color`. Returns `None` if paint is `none`
`crates/engine/layout/src/style.rs:1338` **enum** `BorderCollapse` — CSS Tables L2 §17.6 — `border-collapse`. Inherited. Initial: `Separate`
`crates/engine/layout/src/style.rs:1348` **fn** `parse` — Parse CSS keyword; returns `None` for unrecognised values
`crates/engine/layout/src/style.rs:1362` **enum** `EmptyCells` — CSS Tables L2 §17.6.1.1 — `empty-cells`. Inherited. Initial: `Show`
`crates/engine/layout/src/style.rs:1372` **fn** `parse` — Parse CSS keyword; returns `None` for unrecognised values
`crates/engine/layout/src/style.rs:1384` **enum** `FillRule` — SVG §11.3 — `fill-rule`. Inherited. Initial: `NonZero`
`crates/engine/layout/src/style.rs:1395` **enum** `StrokeLinecap` — SVG §11.4 — `stroke-linecap`. Inherited. Initial: `Butt`
`crates/engine/layout/src/style.rs:1408` **enum** `StrokeLinejoin` — SVG §11.4 — `stroke-linejoin`. Inherited. Initial: `Miter`
`crates/engine/layout/src/style.rs:1421` **enum** `PaintOrderSlot` — CSS Fill & Stroke L3 §6 / SVG 2 §13.7 — one component of `paint-order`
`crates/engine/layout/src/style.rs:1436` **struct** `SvgPaintOrder` — CSS Fill & Stroke L3 §6 / SVG 2 §13.7 — `paint-order`. Inherited
`crates/engine/layout/src/style.rs:1449` **fn** `parse` — Parses `normal | [ fill || stroke || markers ]` (CSS Fill & Stroke L3 §6)
`crates/engine/layout/src/style.rs:1485` **fn** `fill_before_stroke` — True when fill is painted before stroke (so the stroke is drawn on top)
`crates/engine/layout/src/style.rs:1497` **enum** `BorderStyle` — Стиль линии CSS border. None = рамка не отображается (как `display: none`)
`crates/engine/layout/src/style.rs:1507` **fn** `is_visible`
`crates/engine/layout/src/style.rs:1520` **enum** `OutlineStyle` — CSS Basic UI L4 §5.3 — `outline-style`. Включает все `<border-style>`
`crates/engine/layout/src/style.rs:1530` **fn** `is_visible`
`crates/engine/layout/src/style.rs:1543` **enum** `OutlineColor` — CSS Basic UI L4 §5.4 — `outline-color`. Помимо явного цвета поддерживает
`crates/engine/layout/src/style.rs:1554` **enum** `BreakValue` — CSS Fragmentation L3 §3.1 — break-before / break-after / break-inside
`crates/engine/layout/src/style.rs:1577` **enum** `BoxSizing` — CSS `box-sizing`. Определяет, что именно задаёт `width` / `height`:
`crates/engine/layout/src/style.rs:1589` **enum** `Position` — CSS Positioned Layout L3 §3 — `position`. Не наследуется
`crates/engine/layout/src/style.rs:1599` **fn** `parse`
`crates/engine/layout/src/style.rs:1615` **enum** `FloatSide` — CSS 2.1 §9.5.1 — `float`. Не наследуется. `Left`/`Right` выводят
`crates/engine/layout/src/style.rs:1624` **fn** `parse` — Parses `float` keyword value
`crates/engine/layout/src/style.rs:1636` **fn** `is_none` — Returns `true` for `float: none`
`crates/engine/layout/src/style.rs:1644` **enum** `ClearSide` — CSS 2.1 §9.5.2 — `clear`. Не наследуется. Указывает, мимо
`crates/engine/layout/src/style.rs:1654` **fn** `parse` — Parses `clear` keyword value
`crates/engine/layout/src/style.rs:1670` **enum** `Isolation` — CSS Compositing & Blending L1 §2.1 — `isolation`. Не наследуется
`crates/engine/layout/src/style.rs:1677` **fn** `parse`
`crates/engine/layout/src/style.rs:1691` **enum** `MixBlendMode` — CSS Compositing & Blending L1 §3.1 — `mix-blend-mode`. Не наследуется
`crates/engine/layout/src/style.rs:1713` **fn** `parse`
`crates/engine/layout/src/style.rs:1751` **enum** `VerticalAlign` — CSS Inline Layout / CSS 2.1 §10.8.1 — `vertical-align`. Не наследуется
`crates/engine/layout/src/style.rs:1772` **fn** `parse_keyword` — Парсит keyword-формы vertical-align. Не покрывает `<length>` /
`crates/engine/layout/src/style.rs:1797` **enum** `TimingFunction` — CSS Easing L1 §2 — easing function для CSS Transitions и CSS Animations
`crates/engine/layout/src/style.rs:1835` **struct** `LinearEasingPoint` — CSS Easing L2 §2.4 — одна control-точка функции `linear(...)`
`crates/engine/layout/src/style.rs:1854` **fn** `parse` — Парсит keyword (`linear` / `ease` / `ease-in` / `ease-out` /
`crates/engine/layout/src/style.rs:1921` **fn** `parse_list` — CSS Transitions/Animations L1 — comma-list of timing functions
`crates/engine/layout/src/style.rs:1940` **fn** `progress` — CSS Easing L1 §2 — компьютация eased progress
`crates/engine/layout/src/style.rs:2196` **enum** `StepPosition` — CSS Easing L1 §3 — позиция шага в `steps()`. Default по spec — `jump-end`
`crates/engine/layout/src/style.rs:2214` **enum** `IterationCount` — CSS Animations L1 §3.5 — `animation-iteration-count`. Либо число
`crates/engine/layout/src/style.rs:2226` **fn** `parse`
`crates/engine/layout/src/style.rs:2239` **fn** `parse_list`
`crates/engine/layout/src/style.rs:2249` **enum** `AnimationDirection` — CSS Animations L1 §3.6 — `animation-direction`. Default = `Normal`
`crates/engine/layout/src/style.rs:2262` **fn** `parse`
`crates/engine/layout/src/style.rs:2272` **fn** `parse_list`
`crates/engine/layout/src/style.rs:2284` **enum** `AnimationFillMode` — CSS Animations L1 §3.7 — `animation-fill-mode`. Default = `None`
`crates/engine/layout/src/style.rs:2297` **fn** `parse`
`crates/engine/layout/src/style.rs:2307` **fn** `parse_list`
`crates/engine/layout/src/style.rs:2317` **enum** `AnimationPlayState` — CSS Animations L1 §3.8 — `animation-play-state`. Default = `Running`
`crates/engine/layout/src/style.rs:2326` **fn** `parse`
`crates/engine/layout/src/style.rs:2334` **fn** `parse_list`
`crates/engine/layout/src/style.rs:2348` **enum** `AnimationTimeline` — CSS Scroll-Driven Animations L1 §3.3 — `animation-timeline` CSS value
`crates/engine/layout/src/style.rs:2372` **enum** `CssWideKeyword` — CSS-wide keywords (CSS Cascade L4 §7) — применимы к любому свойству
`crates/engine/layout/src/style.rs:2382` **fn** `parse_css_wide_keyword` — ASCII case-insensitive проверка значения декларации на CSS-wide keyword
`crates/engine/layout/src/style.rs:2398` **struct** `ComputedStyle`
`crates/engine/layout/src/style.rs:3225` **enum** `Content` — CSS Content L3 — value свойства `content`
`crates/engine/layout/src/style.rs:3238` **enum** `ContentItem`
`crates/engine/layout/src/style.rs:3272` **enum** `Quotes` — CSS Generated Content L3 §3.2 — `quotes`. Inherited. Initial: `auto`
`crates/engine/layout/src/style.rs:3291` **fn** `pair_for_depth` — Returns the `(open, close)` glyph strings for the given nesting `depth`
`crates/engine/layout/src/style.rs:3313` **enum** `ScrollbarWidth` — CSS Scrollbars 1 — `scrollbar-width`. Inherited
`crates/engine/layout/src/style.rs:3324` **fn** `parse`
`crates/engine/layout/src/style.rs:3336` **enum** `ScrollbarGutter` — CSS Overflow L3 — `scrollbar-gutter`
`crates/engine/layout/src/style.rs:3347` **fn** `parse`
`crates/engine/layout/src/style.rs:3366` **enum** `ListStyleType` — CSS Lists L3 §2.1 — markers для list items
`crates/engine/layout/src/style.rs:3395` **fn** `parse`
`crates/engine/layout/src/style.rs:3417` **enum** `ListStylePosition` — CSS Lists L3 §2.3 — `list-style-position`
`crates/engine/layout/src/style.rs:3426` **fn** `parse`
`crates/engine/layout/src/style.rs:3437` **enum** `OverflowWrap` — CSS Text L3 §5.2 — `overflow-wrap`
`crates/engine/layout/src/style.rs:3448` **fn** `parse`
`crates/engine/layout/src/style.rs:3462` **enum** `LineBreak` — CSS Text L3 §5.2 — `line-break`. Inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:3473` **enum** `WordBreak` — CSS Text L3 §5.1 — `word-break`
`crates/engine/layout/src/style.rs:3485` **fn** `parse`
`crates/engine/layout/src/style.rs:3498` **enum** `Hyphens` — CSS Text L3 §6 — `hyphens`
`crates/engine/layout/src/style.rs:3511` **fn** `parse`
`crates/engine/layout/src/style.rs:3525` **enum** `TouchAction` — CSS Pointer Events L3 / Touch Events — `touch-action`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:3543` **enum** `Appearance` — CSS Basic UI L4 §5 — `appearance`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:3556` **enum** `FieldSizing` — CSS Basic UI L4 §4.4 — `field-sizing`. NOT inherited. Initial: `Fixed`
`crates/engine/layout/src/style.rs:3566` **enum** `PointerEvents` — CSS Pointer Events L1. Default `auto`
`crates/engine/layout/src/style.rs:3580` **fn** `parse`
`crates/engine/layout/src/style.rs:3600` **enum** `Resize` — CSS Basic UI L4 §6 — `resize`. NOT inherited. Initial: `None`
`crates/engine/layout/src/style.rs:3614` **struct** `ContainFlags` — CSS Containment L3 §3 — `contain` property
`crates/engine/layout/src/style.rs:3631` **enum** `ContentVisibility` — CSS Containment L3 §4 — `content-visibility`. NOT inherited. Initial: `Visible`
`crates/engine/layout/src/style.rs:3652` **enum** `InterpolateSizeMode` — CSS Sizing L4 §4.5 — `interpolate-size` property value
`crates/engine/layout/src/style.rs:3664` **enum** `ContainerType` — CSS Container Queries L1 §3.1 — `container-type`. NOT inherited. Initial: `Normal`
`crates/engine/layout/src/style.rs:3674` **struct** `ContainerContext` — Resolved container dimensions, passed during style re-computation for container queries
`crates/engine/layout/src/style.rs:3696` **fn** `evaluate_container_condition` — Evaluates a raw @container condition string against a `ContainerContext`
`crates/engine/layout/src/style.rs:3798` **fn** `apply_container_rules` — Applies matching `@container` rules from `sheet` to `style`
`crates/engine/layout/src/style.rs:3852` **enum** `ShapeOutside` — CSS Shapes L1 §3 — `shape-outside` value. NOT inherited. Initial: `None`
`crates/engine/layout/src/style.rs:3861` **enum** `OffsetRotate` — CSS Motion Path L1 §3 — `offset-rotate`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:3872` **enum** `PrintColorAdjust` — CSS Color Adjustment L1 §5 — `print-color-adjust`. NOT inherited. Initial: `Economy`
`crates/engine/layout/src/style.rs:3880` **enum** `FontSizeAdjust` — CSS Fonts L5 §4 — `font-size-adjust`. Inherited. Initial: `None`
`crates/engine/layout/src/style.rs:3889` **enum** `WritingMode` — CSS Writing Modes L3 §2.1 — `writing-mode`. Inherited. Initial: `HorizontalTb`
`crates/engine/layout/src/style.rs:3906` **enum** `TextOrientation` — CSS Writing Modes L3 §6.5 — `text-orientation`. Inherited. Initial: `Mixed`
`crates/engine/layout/src/style.rs:3918` **enum** `UserSelect` — CSS UI L4 §6.2 — `user-select`. Inherited
`crates/engine/layout/src/style.rs:3928` **fn** `parse`
`crates/engine/layout/src/style.rs:3942` **enum** `ScrollBehavior` — CSS Overflow L3 — `scroll-behavior`. Inherited
`crates/engine/layout/src/style.rs:3950` **struct** `ScrollSnapType` — CSS Scroll Snap L1 §3.1 — `scroll-snap-type: none | <axis> [mandatory | proximity]`
`crates/engine/layout/src/style.rs:3956` **enum** `ScrollSnapAxis`
`crates/engine/layout/src/style.rs:3967` **enum** `ScrollSnapStrictness`
`crates/engine/layout/src/style.rs:3975` **struct** `ScrollSnapAlign` — CSS Scroll Snap L1 §6.1 — `scroll-snap-align: none | <axis-keyword>{1,2}`
`crates/engine/layout/src/style.rs:3981` **enum** `ScrollSnapAlignKeyword`
`crates/engine/layout/src/style.rs:3990` **enum** `ScrollSnapStop`
`crates/engine/layout/src/style.rs:3998` **enum** `OverscrollBehavior` — CSS Overscroll Behavior L1 §2 — `overscroll-behavior: auto | contain | none`
`crates/engine/layout/src/style.rs:4006` **fn** `parse`
`crates/engine/layout/src/style.rs:4021` **enum** `ParsedGradient` — CSS Images L3/L4 §3.3/§3.7 — parsed linear / radial / conic gradient
`crates/engine/layout/src/style.rs:4066` **enum** `RadialShape` — CSS Images L3 §3.5 — ending-shape of a `radial-gradient`
`crates/engine/layout/src/style.rs:4078` **enum** `RadialSize` — CSS Images L3 §3.5 — sizing keyword controlling the radii of a
`crates/engine/layout/src/style.rs:4096` **fn** `radial_gradient_radii` — CSS Images L3 §3.5.1 — resolves a `radial-gradient` ending shape to concrete
`crates/engine/layout/src/style.rs:4133` **enum** `BackgroundImage` — CSS Backgrounds L3 §3.1 / CSS Images L4 §4 — `background-image` value
`crates/engine/layout/src/style.rs:4161` **enum** `BackgroundRepeat` — CSS Backgrounds L3 §3.4 — `background-repeat`
`crates/engine/layout/src/style.rs:4172` **fn** `parse`
`crates/engine/layout/src/style.rs:4191` **enum** `BgSizeAxis` — CSS Backgrounds L3 §3.5 — one axis of an explicit `background-size` value
`crates/engine/layout/src/style.rs:4205` **fn** `resolve` — Resolve to a concrete px extent against `area` (the positioning-area
`crates/engine/layout/src/style.rs:4216` **enum** `BackgroundSize` — CSS Backgrounds L3 §3.5 — `background-size`
`crates/engine/layout/src/style.rs:4228` **enum** `BackgroundAttachment` — CSS Backgrounds L3 §3.6 — `background-attachment`
`crates/engine/layout/src/style.rs:4236` **fn** `parse`
`crates/engine/layout/src/style.rs:4257` **enum** `BackgroundOrigin` — CSS Backgrounds L3 §3.7 — `background-origin`. Non-inherited
`crates/engine/layout/src/style.rs:4268` **fn** `parse`
`crates/engine/layout/src/style.rs:4291` **enum** `BackgroundClip` — CSS Backgrounds L3 §3.8 — `background-clip`. Non-inherited
`crates/engine/layout/src/style.rs:4305` **fn** `parse`
`crates/engine/layout/src/style.rs:4321` **struct** `BackgroundLayer` — CSS Backgrounds L3 §3 — один фоновый слой. Первый в Vec = верхний (рисуется последним)
`crates/engine/layout/src/style.rs:4361` **enum** `ObjectFit` — CSS Images L3 §5.5 — `object-fit`. Применяется к replaced elements
`crates/engine/layout/src/style.rs:4382` **fn** `parse`
`crates/engine/layout/src/style.rs:4402` **enum** `ImageRendering` — CSS Images L3 §6.1 — `image-rendering`. Hint для движка о том, как
`crates/engine/layout/src/style.rs:4422` **fn** `parse`
`crates/engine/layout/src/style.rs:4446` **enum** `TextWrapMode` — CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited
`crates/engine/layout/src/style.rs:4455` **fn** `parse`
`crates/engine/layout/src/style.rs:4473` **enum** `TextWrapStyle` — CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited
`crates/engine/layout/src/style.rs:4486` **fn** `parse`
`crates/engine/layout/src/style.rs:4502` **enum** `FlexDirection` — CSS Flexbox L1 §5.1 — `flex-direction`. Non-inherited
`crates/engine/layout/src/style.rs:4515` **fn** `parse`
`crates/engine/layout/src/style.rs:4531` **enum** `FlexWrap` — CSS Flexbox L1 §5.2 — `flex-wrap`. Non-inherited
`crates/engine/layout/src/style.rs:4542` **fn** `parse`
`crates/engine/layout/src/style.rs:4557` **enum** `FlexBasis` — CSS Flexbox L1 §7.3 — `flex-basis`. Non-inherited
`crates/engine/layout/src/style.rs:4568` **fn** `parse`
`crates/engine/layout/src/style.rs:4582` **struct** `GridRepeat` — CSS Grid Layout L3 §9 — `repeat(auto-fill | auto-fit | <count>, <track-list>)`
`crates/engine/layout/src/style.rs:4591` **enum** `RepeatCount` — Count type for grid-template-columns/rows `repeat()`
`crates/engine/layout/src/style.rs:4604` **enum** `GridTrackSize` — CSS Grid Layout L1 §7.2 — sizing function for a grid track
`crates/engine/layout/src/style.rs:4638` **fn** `resolve_fixed` — Resolve to a concrete pixel size given container width, em, viewport
`crates/engine/layout/src/style.rs:4647` **fn** `is_fr` — True for fractional tracks
`crates/engine/layout/src/style.rs:4652` **fn** `fr` — Extract fr value
`crates/engine/layout/src/style.rs:4657` **fn** `is_subgrid` — True when this track inherits its size from the parent grid (subgrid axis)
`crates/engine/layout/src/style.rs:4662` **fn** `is_masonry` — True when this axis uses masonry placement (CSS Grid L3 §14)
`crates/engine/layout/src/style.rs:4708` **fn** `parse_track_list` — Parse a track-list value string into a Vec of GridTrackSize
`crates/engine/layout/src/style.rs:4839` **enum** `GridAutoFlow` — CSS Grid Layout L1 §8.5 — `grid-auto-flow`. Non-inherited
`crates/engine/layout/src/style.rs:4852` **fn** `parse`
`crates/engine/layout/src/style.rs:4866` **enum** `MasonryAutoFlow` — CSS Masonry Layout §9 — `masonry-auto-flow`. Controls the placement order
`crates/engine/layout/src/style.rs:4879` **fn** `parse` — Parse a CSS `masonry-auto-flow` value string
`crates/engine/layout/src/style.rs:4892` **enum** `GridLine` — CSS Grid Layout L1 §8.3 — a grid-line reference for grid-column-start,
`crates/engine/layout/src/style.rs:4906` **fn** `parse`
`crates/engine/layout/src/style.rs:4941` **enum** `PositionComponent` — Одна компонента `object-position`. Length-варианты резолвятся в px
`crates/engine/layout/src/style.rs:4954` **fn** `resolve` — Резолв в финальный px-offset относительно левого/верхнего края
`crates/engine/layout/src/style.rs:4965` **struct** `ObjectPosition` — CSS Images L3 §5.5 — `object-position` (две компоненты, x + y)
`crates/engine/layout/src/style.rs:5002` **fn** `parse` — CSS Values L4 §9.4 — `<position>` для object-position. Phase 0
`crates/engine/layout/src/style.rs:5104` **enum** `AlignValue` — CSS Box Alignment L3 §6.1 — значения для align-/justify- свойств
`crates/engine/layout/src/style.rs:5131` **fn** `parse`
`crates/engine/layout/src/style.rs:5155` **enum** `ShapeValue` — CSS Masking L1 §3.5 — `<length-percentage>` значение координаты/размера
`crates/engine/layout/src/style.rs:5165` **fn** `resolve` — Резолвит значение в px. `basis` — размер reference box по
`crates/engine/layout/src/style.rs:5180` **enum** `ClipPath` — CSS Masking L1 §3.5 — basic-shapes для `clip-path`. Phase 0
`crates/engine/layout/src/style.rs:5221` **enum** `TransformStyle` — CSS Transforms L1 §11 — функции `transform`. Phase 0 поддерживает
`crates/engine/layout/src/style.rs:5231` **enum** `BackfaceVisibility` — CSS Transforms L2 §5.1 — `backface-visibility: visible | hidden`
`crates/engine/layout/src/style.rs:5242` **enum** `TransformFn` — CSS transform functions — translate/scale/rotate/skew/skewX/skewY/matrix
`crates/engine/layout/src/style.rs:5280` **enum** `FilterFn` — CSS Filter Effects L1 §3 — функции `filter`. Phase 0 поддерживает
`crates/engine/layout/src/style.rs:5313` **struct** `GradientStop` — CSS Images L3 §3.4 — единичный `<color-stop>` градиента
`crates/engine/layout/src/style.rs:5326` **enum** `MaskMode` — CSS Masking L1 §6.4 — `mask-mode`. Selects which channel of the mask image
`crates/engine/layout/src/style.rs:5340` **enum** `MaskComposite` — CSS Masking L1 §4.7 — `mask-composite`. Controls how multiple mask layers
`crates/engine/layout/src/style.rs:5349` **fn** `parse`
`crates/engine/layout/src/style.rs:5365` **fn** `outline_used_width` — CSS 2.1 §17.6.1 / Basic UI L4 §5.2 — **used** value `outline-width`
`crates/engine/layout/src/style.rs:5376` **fn** `text_rendering_eq` — Два стиля рендерят текст одинаково (цвет, размер, интерлиньяж, начертание,
`crates/engine/layout/src/style.rs:5394` **fn** `root` — Стартовые значения для корня документа
`crates/engine/layout/src/style.rs:5703` **fn** `compute_style` — Computes the `ComputedStyle` for `node` by running the CSS cascade
`crates/engine/layout/src/style.rs:6976` **fn** `compute_style_from_declarations` — Build a `ComputedStyle` from a flat list of declarations with neutral context
`crates/engine/layout/src/style.rs:6993` **fn** `compute_pseudo_element_style` — Вычисляет стиль для псевдоэлемента `::before` или `::after` элемента `node`
`crates/engine/layout/src/style.rs:7212` **fn** `compute_selection_style` — Computes the `::selection` override style for a DOM element
`crates/engine/layout/src/style.rs:7270` **fn** `validate_against_syntax` — CSS Properties and Values L1 §2 — упрощённая валидация значения
`crates/engine/layout/src/style.rs:9758` **fn** `ua_form_element_colors` — UA stylesheet для HTML form controls (HTML5 §15.5 «Rendering»)
`crates/engine/layout/src/style.rs:9968` **fn** `parse_font_family` — Парсит `font-family: a, "b c", d` в Vec<String>. Запятые разделяют
`crates/engine/layout/src/style.rs:10031` **fn** `parse_font_variation_settings` — Парсит CSS `font-variation-settings` (CSS Fonts L4 §7)
`crates/engine/layout/src/style.rs:10075` **fn** `parse_font_feature_settings` — Парсит CSS `font-feature-settings` (CSS Fonts L3 §6)
`crates/engine/layout/src/style.rs:10117` **enum** `FontPalette` — CSS Fonts L4 §11.3 — computed value of `font-palette`
`crates/engine/layout/src/style.rs:10134` **fn** `parse_font_palette` — Парсит CSS `font-palette`: `normal | light | dark | <dashed-ident>`
`crates/engine/layout/src/style.rs:10209` **fn** `set_cq_context` — Sets the nearest-container size for `cq*` unit resolution during the container re-layout pass
`crates/engine/layout/src/style.rs:10214` **fn** `clear_cq_context` — Clears the `cq*` context after the container re-layout pass completes
`crates/engine/layout/src/style.rs:10238` **fn** `set_interactive_state` — Sets the interactive hover/focus/active state for the next layout pass
`crates/engine/layout/src/style.rs:10249` **fn** `clear_interactive_state` — Clears hover/focus/active state after layout
`crates/engine/layout/src/style.rs:10268` **fn** `set_forced_colors` — Enables/disables Forced Colors Mode (CSS Color Adjustment L1 §3) for all
`crates/engine/layout/src/style.rs:10273` **fn** `forced_colors_active` — True when Forced Colors Mode is active on the current thread
`crates/engine/layout/src/style.rs:10292` **fn** `set_print_media` — Selects the `print` (`true`) or `screen` (`false`) `@media` type for all
`crates/engine/layout/src/style.rs:10297` **fn** `print_media_active` — True when the current layout pass renders for `print` media
`crates/engine/layout/src/style.rs:10342` **enum** `LengthOrAuto` — CSS `<length> | auto` — для margin и offset-свойств, где `auto` имеет
`crates/engine/layout/src/style.rs:10350` **fn** `is_auto`
`crates/engine/layout/src/style.rs:10357` **fn** `to_px_opt` — Returns the raw pixel value for `Length::Px` variants; `Auto` and all
`crates/engine/layout/src/style.rs:10367` **fn** `resolve` — Резолвит в пиксели. `Auto` → `None`; нерезолвируемый `%` → `None`
`crates/engine/layout/src/style.rs:10375` **fn** `resolve_or_zero` — Резолвит в пиксели; для `Auto` и нерезолвируемых значений → 0.0
`crates/engine/layout/src/style.rs:10386` **enum** `Length` — Типизированная длина CSS до резолва в пиксели
`crates/engine/layout/src/style.rs:10451` **enum** `CalcNode` — CSS Values L4 §10 — AST `calc()`-выражения. Хранится как двоичное дерево
`crates/engine/layout/src/style.rs:10480` **enum** `MathFn` — CSS Values L4 §10.7-10.9 — научные math-функции. Имена case-insensitive
`crates/engine/layout/src/style.rs:10509` **enum** `RoundStrategy` — CSS Values L4 §10.5.1 — стратегия округления для `round()`
`crates/engine/layout/src/style.rs:10533` **fn** `resolve` — Резолвит выражение в `f32`-пиксели по тем же правилам, что
`crates/engine/layout/src/style.rs:10731` **fn** `resolve` — Возвращает длину в пикселях. `em_basis` — fs, относительно которого
`crates/engine/layout/src/style.rs:10771` **fn** `is_intrinsic` — Returns `true` if this is an intrinsic sizing keyword (min-content,
`crates/engine/layout/src/style.rs:10777` **fn** `resolve_or_zero` — Резолвит с `cb_width` как percent_basis; возвращает 0.0 при неудаче
`crates/engine/layout/src/style.rs:10783` **fn** `px` — Извлекает пиксельное значение для уже-разрешённых `Px`-значений
`crates/engine/layout/src/style.rs:10938` **fn** `parse_length`
`crates/engine/layout/src/style.rs:14680` **fn** `resolve_logical_property` — Resolve CSS Logical Properties based on writing-mode
`crates/engine/layout/src/style.rs:16765` **fn** `parse_transform_list` — Парсит `<transform-list>` — последовательность `func(args)` через
`crates/engine/layout/src/style.rs:17999` **fn** `parse_grid_template_areas` — CSS Grid L1 §7.3 — parse `grid-template-areas` value
`crates/engine/layout/src/style.rs:18079` **fn** `parse_background_gradient` — CSS Images L3/L4 §3.3/§3.7 — parses color stops from a CSS gradient string
`crates/engine/layout/src/style.rs:18477` **fn** `parse_gradient_stops` — The leading direction / angle / shape argument (e.g. `to right`,
`crates/engine/layout/src/style.rs:19348` **fn** `parse_color`
`crates/engine/layout/src/style.rs:19511` **fn** `system_color` — CSS Color Module Level 4 §6.2 — резолв системных цветовых ключевых слов
`crates/engine/layout/src/subgrid.rs:24` **struct** `SubgridContext` — Resolved track sizes and cumulative offsets for one grid axis (columns or rows)
`crates/engine/layout/src/subgrid.rs:35` **fn** `from_parent_tracks` — Build from a slice of parent track sizes and the gap value used between them
`crates/engine/layout/src/subgrid.rs:46` **fn** `total_size` — Total span width/height occupied by all inherited tracks (including inter-track gaps)
`crates/engine/layout/src/subgrid.rs:96` **struct** `SubgridItem` — A grid item that is itself a subgrid container for at least one axis
`crates/engine/layout/src/subgrid.rs:113` **fn** `collect_subgrid_items` — Collect all layout boxes in the tree that are subgrid containers
`crates/engine/layout/src/table.rs:17` **enum** `BorderPrecedence` — CSS Tables L2 §17.6.2 — precedence level used when two borders compete in collapsed mode
`crates/engine/layout/src/table.rs:38` **struct** `CollapsedBorder` — Resolved border description for the collapsed border model (CSS Tables L2 §17.6.2)
`crates/engine/layout/src/table.rs:50` **fn** `resolve_conflict` — Resolves conflict between two competing borders per CSS Tables L2 §17.6.2:
`crates/engine/layout/src/table.rs:67` **struct** `TableContext` — Table layout algorithm context
`crates/engine/layout/src/table.rs:109` **fn** `new` — Create a new empty table context with CSS-initial values
`crates/engine/layout/src/table.rs:124` **fn** `collect_table_structure` — Scan table structure and infer column count, explicit widths, and rowspan occupancy
`crates/engine/layout/src/table.rs:239` **fn** `compute_table_col_widths` — Compute table column widths using the table-layout algorithm
`crates/engine/layout/src/table.rs:274` **fn** `lay_out_table` — Lay out table rows and cells
`crates/engine/layout/src/text_iter.rs:17` **struct** `TextFragment` — A visible text fragment with its absolute screen rectangle
`crates/engine/layout/src/text_iter.rs:37` **fn** `collect_visible_text` — Walk the layout tree and collect all visible text fragments with screen coordinates

## lumen-mcp  (25 symbols)

`crates/mcp/src/live.rs:23` **fn** `spawn` — Spawn the live-window MCP server on `127.0.0.1:port`. Non-blocking — runs
`crates/mcp/src/protocol.rs:8` **struct** `McpResource` — MCP resource describing a read-only data snapshot
`crates/mcp/src/protocol.rs:21` **struct** `McpTool` — MCP tool describing a callable action
`crates/mcp/src/protocol.rs:32` **struct** `McpRequest` — MCP JSON-RPC запрос
`crates/mcp/src/protocol.rs:47` **fn** `new` — Создать новый MCP запрос
`crates/mcp/src/protocol.rs:57` **fn** `with_id` — Создать запрос с ID для отслеживания ответа
`crates/mcp/src/protocol.rs:65` **struct** `McpResponse` — MCP JSON-RPC ответ
`crates/mcp/src/protocol.rs:80` **fn** `ok` — Создать успешный ответ
`crates/mcp/src/protocol.rs:90` **fn** `err` — Создать ошибку
`crates/mcp/src/protocol.rs:106` **struct** `McpError` — JSON-RPC ошибка
`crates/mcp/src/protocol.rs:118` **enum** `McpMessage` — Размеченное MCP сообщение (запрос или ответ)
`crates/mcp/src/protocol.rs:129` **fn** `from_json` — Распарсить JSON в MCP сообщение
`crates/mcp/src/protocol.rs:137` **fn** `to_json` — Сериализовать MCP сообщение в JSON
`crates/mcp/src/server.rs:15` **struct** `McpServer` — MCP сервер для Lumen браузера
`crates/mcp/src/server.rs:24` **fn** `new` — Создать новый MCP сервер
`crates/mcp/src/server.rs:29` **fn** `run` — Основной цикл сервера: читать запросы и писать ответы
`crates/mcp/src/transport.rs:10` **trait** `Transport` — Абстракция транспорта для MCP сообщений
`crates/mcp/src/transport.rs:22` **struct** `StdioTransport` — Stdio-транспорт (stdin/stdout)
`crates/mcp/src/transport.rs:29` **fn** `new` — Создать новый stdio-транспорт
`crates/mcp/src/transport.rs:69` **struct** `TcpTransport` — TCP-транспорт для `--mcp-port N` режима
`crates/mcp/src/transport.rs:76` **fn** `from_stream` — Создать транспорт поверх уже принятого `TcpStream`
`crates/mcp/src/transport.rs:113` **struct** `VecTransport` — In-memory транспорт для unit-тестов
`crates/mcp/src/transport.rs:122` **fn** `new` — Создать пустой транспорт
`crates/mcp/src/transport.rs:127` **fn** `push_incoming` — Поставить в очередь входящее JSON сообщение
`crates/mcp/src/transport.rs:132` **fn** `take_outgoing` — Забрать все исходящие сообщения (очищает буфер)

## lumen-network  (489 symbols)

`crates/network/src/auth.rs:52` **fn** `get`
`crates/network/src/auth.rs:619` **struct** `StaticCredentialProvider` — Простой credential-провайдер с фиксированной табличкой `(origin, realm) →
`crates/network/src/auth.rs:624` **fn** `new`
`crates/network/src/auth.rs:632` **fn** `with` — Точное совпадение `(origin, realm)`
`crates/network/src/auth.rs:640` **fn** `add` — Зарегистрировать creds после конструирования. `&self` (не `&mut`) —
`crates/network/src/brotli.rs:24` **struct** `BrotliContentDecoder` — `ContentDecoder` для `Content-Encoding: br`. Stateless: один экземпляр
`crates/network/src/coop.rs:37` **enum** `CrossOriginOpenerPolicy` — Value of the `Cross-Origin-Opener-Policy` header
`crates/network/src/coop.rs:59` **fn** `parse` — Parse the value of a `Cross-Origin-Opener-Policy` header
`crates/network/src/coop.rs:70` **fn** `severs_opener` — Whether this policy causes cross-origin documents to lose `window.opener`
`crates/network/src/coop.rs:76` **fn** `allows_cross_origin_isolation` — Whether this policy is compatible with cross-origin isolation
`crates/network/src/coop.rs:87` **enum** `CrossOriginEmbedderPolicy` — Value of the `Cross-Origin-Embedder-Policy` header
`crates/network/src/coop.rs:100` **fn** `parse` — Parse the value of a `Cross-Origin-Embedder-Policy` header
`crates/network/src/coop.rs:109` **fn** `enables_cross_origin_isolation` — Whether this policy enables cross-origin isolation (together with COOP)
`crates/network/src/coop.rs:118` **enum** `CrossOriginResourcePolicy` — Value of the `Cross-Origin-Resource-Policy` header
`crates/network/src/coop.rs:130` **fn** `parse` — Parse the value of a `Cross-Origin-Resource-Policy` header
`crates/network/src/coop.rs:148` **struct** `CrossOriginIsolationState` — The derived cross-origin isolation state of a browsing context
`crates/network/src/coop.rs:159` **fn** `from_headers` — Compute isolation state from COOP and COEP headers present on an HTTP response
`crates/network/src/coop.rs:170` **fn** `is_cross_origin_isolated` — Whether this document is cross-origin isolated
`crates/network/src/coop.rs:188` **fn** `check_corp_allowed` — Check whether a cross-origin resource fetch is allowed under CORP rules
`crates/network/src/cors.rs:35` **enum** `CredentialsMode` — Credentials mode по Fetch §3.1 — определяет, прикладывать ли cookies /
`crates/network/src/cors.rs:50` **fn** `cross_origin_credentials` — Применяются ли credentials для cross-origin запроса в этом режиме?
`crates/network/src/cors.rs:62` **struct** `CorsRequest` — Cross-origin запрос — описание для решения о preflight и сборки CORS-заголовков
`crates/network/src/cors.rs:74` **fn** `is_cors_safelisted_method` — «CORS-safelisted method» (Fetch §4.4.1): GET / HEAD / POST
`crates/network/src/cors.rs:83` **fn** `is_forbidden_request_header` — «forbidden request-header name» (Fetch §4.4.4). UA-controlled заголовки,
`crates/network/src/cors.rs:123` **fn** `is_cors_safelisted_request_header` — «CORS-safelisted request-header» (Fetch §4.4.2). Возвращает true, если
`crates/network/src/cors.rs:151` **fn** `is_cors_safelisted_content_type` — «CORS-safelisted Content-Type» (Fetch §4.4.2): одна из трёх MIME-форм
`crates/network/src/cors.rs:204` **fn** `needs_preflight` — Возвращает true, если запрос требует preflight перед actual request
`crates/network/src/cors.rs:221` **fn** `unsafe_request_header_names` — Имена «unsafe» author-заголовков (lowercased + sorted lexicographically)
`crates/network/src/cors.rs:249` **fn** `build_preflight_headers` — Заголовки OPTIONS preflight-запроса
`crates/network/src/cors.rs:271` **struct** `PreflightResult` — Результат успешного preflight-а. Кешируется по (origin, target_origin,
`crates/network/src/cors.rs:291` **fn** `method_allowed` — Покрывает ли результат preflight-а метод `method` (case-insensitive)?
`crates/network/src/cors.rs:310` **fn** `unmatched_header` — Покрывает ли результат preflight-а все unsafe-заголовки запроса?
`crates/network/src/cors.rs:331` **enum** `CorsError` — Ошибки CORS-валидации (preflight или actual response)
`crates/network/src/cors.rs:393` **fn** `evaluate_preflight_response` — Полный разбор preflight-ответа. Возвращает [`PreflightResult`] для
`crates/network/src/cors.rs:436` **fn** `check_cors_response_headers` — Валидация ACAO + ACAC на **actual response** (не preflight) — Fetch §4.10
`crates/network/src/cors.rs:543` **struct** `PreflightCache` — Кеш preflight-результатов по `(requestor_origin, target_origin,
`crates/network/src/cors.rs:561` **fn** `new`
`crates/network/src/cors.rs:570` **fn** `insert_at` — Добавить результат preflight-а в кеш. `now` — текущее время от UNIX
`crates/network/src/cors.rs:592` **fn** `insert` — То же что [`Self::insert_at`], но с `now = SystemTime::now()`. Для
`crates/network/src/cors.rs:604` **fn** `lookup_at` — Достать НЕИСТЁКШЕЕ entry. Истёкшие удаляются lazy (next-access
`crates/network/src/cors.rs:625` **fn** `lookup`
`crates/network/src/cors.rs:637` **fn** `allows_at` — Возвращает true, если кеш содержит подходящее entry для `req` (метод
`crates/network/src/cors.rs:652` **fn** `allows`
`crates/network/src/cors.rs:657` **fn** `clear` — Полная очистка (для тестов / Profile switching)
`crates/network/src/csp.rs:14` **enum** `HashAlgorithm` — Hash algorithm used in a CSP hash source expression
`crates/network/src/csp.rs:28` **enum** `CspSource` — A single source expression from a CSP directive source list
`crates/network/src/csp.rs:60` **enum** `CspDirective` — A CSP fetch / navigation directive name
`crates/network/src/csp.rs:111` **struct** `CspPolicy` — A parsed Content Security Policy
`crates/network/src/csp.rs:128` **fn** `is_empty` — Returns `true` if no directives or flags are set
`crates/network/src/csp.rs:140` **fn** `effective_sources` — Returns the effective source list for `directive`, falling back to
`crates/network/src/csp.rs:159` **fn** `parse_csp_header` — Parse a `Content-Security-Policy` header value into a [`CspPolicy`]
`crates/network/src/csp.rs:166` **fn** `parse_csp_report_only_header` — Parse a report-only variant of the CSP header
`crates/network/src/ctap2.rs:70` **enum** `Ctap2Error` — Error produced by the CTAP2 HID transport layer
`crates/network/src/ctap2.rs:104` **trait** `HidDevice` — Platform-agnostic USB HID device I/O
`crates/network/src/ctap2.rs:124` **struct** `CtapHidChannel` — An established CTAPHID channel with a specific device
`crates/network/src/ctap2.rs:133` **fn** `init` — Perform the CTAPHID_INIT handshake and return a channel with the
`crates/network/src/ctap2.rs:160` **fn** `send_cbor` — Send a CTAP2 CBOR command and return the CBOR response payload (status
`crates/network/src/ctap2.rs:633` **fn** `extract_credential_id` — Extract the credential ID from the `authenticatorData` byte string
`crates/network/src/ctap2.rs:716` **fn** `probe_usb_fido_devices` — Enumerate connected FIDO2 USB HID devices using the platform HID backend
`crates/network/src/ctap2.rs:729` **fn** `platform_enumerate_ctap2_devices` — Platform-native FIDO2 USB HID device enumeration
`crates/network/src/ctap2.rs:873` **struct** `WinHidDevice` — A real USB HID device opened via Win32 `CreateFile`
`crates/network/src/ctap2.rs:933` **fn** `enumerate` — Enumerate USB HID FIDO2 devices via Win32 SetupDi + HidD APIs
`crates/network/src/ctap2.rs:1108` **struct** `LinuxHidDevice` — A FIDO2 device exposed as a Linux `/dev/hidrawN` character device
`crates/network/src/ctap2.rs:1203` **fn** `enumerate` — Scan `/dev/hidraw0`..`/dev/hidraw31` and return FIDO2 devices
`crates/network/src/ctap2.rs:1242` **struct** `CtapRoamingTransport` — [`CredentialProvider`] that uses a connected FIDO2 USB security key
`crates/network/src/ctap2.rs:1246` **fn** `new` — Create a new roaming transport
`crates/network/src/ctap2.rs:1338` **struct** `CompositeCredentialProvider` — A [`CredentialProvider`] that delegates to a priority-ordered list
`crates/network/src/ctap2.rs:1344` **fn** `new` — Create a composite from an ordered list of providers
`crates/network/src/ctap2.rs:1383` **struct** `MockHidDevice` — A scripted in-memory [`HidDevice`] for unit tests
`crates/network/src/ctap2.rs:1393` **fn** `new` — Create a blank mock with no queued responses
`crates/network/src/ctap2.rs:1402` **fn** `push_response` — Push a raw 65-byte HID report to the response queue
`crates/network/src/ctap2.rs:1407` **fn** `queue_init_response` — Build and queue a CTAPHID_INIT response for the given nonce + CID
`crates/network/src/ctap2.rs:1424` **fn** `queue_cbor_response` — Build and queue a successful CTAPHID_CBOR response with the given payload
`crates/network/src/ctap2.rs:1454` **fn** `written_reports` — Return all written reports (as slices) for inspection
`crates/network/src/ctap2.rs:1485` **fn** `seal` — Reverse the internal response queue so items are served FIFO
`crates/network/src/dns.rs:22` **struct** `SystemDnsResolver` — DNS-резолвер на основе системного getaddrinfo (через std::net)
`crates/network/src/doh.rs:46` **fn** `encode_query` — Закодировать стандартный DNS query — header + одна question. RD=1
`crates/network/src/doh.rs:100` **fn** `decode_answer_ips` — Распакованный DNS-ответ — без CNAME-цепочек, только IP-адреса из
`crates/network/src/doh.rs:249` **fn** `base64url_encode` — Закодировать байты в base64url **без padding** — RFC 8484 §4.1 явно
`crates/network/src/doh.rs:302` **struct** `DohResolver` — DNS-over-HTTPS резолвер
`crates/network/src/doh.rs:310` **fn** `new` — `endpoint` — URL DoH сервера со схемой `https://`. `transport` —
`crates/network/src/doh.rs:405` **struct** `CachedDnsResolver` — Used to reduce DoH / system DNS lookups when resolving frequently-used hosts
`crates/network/src/doh.rs:413` **fn** `new` — Create a new cached resolver wrapping `inner`
`crates/network/src/dot.rs:62` **fn** `frame_query` — Обернуть DNS message в two-octet length prefix: `[u16 BE len][msg]`
`crates/network/src/dot.rs:77` **fn** `read_framed_message` — Прочитать ОДНО framed DNS message из stream-а: 2 байта BE length,
`crates/network/src/dot.rs:107` **fn** `query_over_stream` — Послать ОДИН DNS query (AAAA или A — определяется `qtype`) по уже
`crates/network/src/dot.rs:140` **struct** `DotResolver` — DNS-over-TLS резолвер
`crates/network/src/dot.rs:149` **fn** `new` — Базовый конструктор. `server_name` — TLS SNI/cert host;
`crates/network/src/dot.rs:159` **fn** `cloudflare` — Cloudflare `1.1.1.1:853` с SNI `one.one.one.one`
`crates/network/src/dot.rs:167` **fn** `google` — Google Public DNS `8.8.8.8:853` с SNI `dns.google`
`crates/network/src/dot.rs:175` **fn** `quad9` — Quad9 `9.9.9.9:853` с SNI `dns.quad9.net`
`crates/network/src/filter/default_list.rs:25` **struct** `DefaultFilterList` — Bundled EasyList-format ruleset shipped inside the Lumen binary
`crates/network/src/filter/easylist.rs:236` **struct** `EasyListFilter` — EasyList-format `RequestFilter` implementation
`crates/network/src/filter/easylist.rs:254` **fn** `parse` — Parse an EasyList-format text and return a filter
`crates/network/src/filter/easylist.rs:263` **fn** `rule_count` — Number of block rules loaded
`crates/network/src/filter/hosts.rs:28` **struct** `HostsFilter` — Hosts-file `RequestFilter`
`crates/network/src/filter/hosts.rs:34` **fn** `parse` — Parse a hosts-file text and return a filter
`crates/network/src/filter/hosts.rs:73` **fn** `len` — Number of blocked hostnames
`crates/network/src/filter/hosts.rs:78` **fn** `is_empty` — Returns `true` if the block list is empty
`crates/network/src/filter/mod.rs:45` **struct** `CompositeFilter` — Chains multiple [`RequestFilter`] implementations
`crates/network/src/filter/mod.rs:51` **fn** `new` — Create a composite filter from a list of inner filters
`crates/network/src/flate.rs:28` **struct** `GzipContentDecoder` — `ContentDecoder` для `Content-Encoding: gzip`. Stateless: один экземпляр
`crates/network/src/flate.rs:60` **struct** `DeflateContentDecoder` — `ContentDecoder` для `Content-Encoding: deflate`. Stateless
`crates/network/src/h2/conn.rs:54` **type** `H2Response` — Decoded HTTP response from an H2 fetch: `(status, headers, body)`
`crates/network/src/h2/conn.rs:103` **struct** `H2Conn` — Stateful HTTP/2 client connection
`crates/network/src/h2/conn.rs:134` **fn** `connect` — Establish an HTTP/2 connection with Chrome-matching SETTINGS
`crates/network/src/h2/conn.rs:143` **fn** `connect_with_profile` — Establish an HTTP/2 connection over `stream` with SETTINGS matching the given profile
`crates/network/src/h2/conn.rs:320` **fn** `fetch` — Perform a single HTTP/2 request and collect the response
`crates/network/src/h2/conn.rs:488` **fn** `send_request` — Send a single HTTP/2 request without waiting for the response
`crates/network/src/h2/conn.rs:531` **fn** `read_response_for_stream` — Read and assemble the complete response for a specific stream ID
`crates/network/src/h2/frame.rs:107` **enum** `FrameError` — Codec-level error. The codec produces only two RFC 9113 §7 error codes on
`crates/network/src/h2/frame.rs:150` **struct** `Priority` — Stream priority block — used by the PRIORITY frame and by HEADERS when the
`crates/network/src/h2/frame.rs:162` **enum** `Frame` — Parsed/encodable HTTP/2 frame (RFC 9113 §6). For padded frames the carried
`crates/network/src/h2/frame.rs:286` **fn** `parse` — Parse one frame from `buf`
`crates/network/src/h2/frame.rs:337` **fn** `encode` — Serialize the frame: append the 9-byte header and payload to `out`
`crates/network/src/h2/hpack.rs:17` **enum** `HpackError` — HPACK codec error. All variants map to `COMPRESSION_ERROR` (0x09) at the
`crates/network/src/h2/hpack.rs:393` **fn** `decode_int` — Decode a variable-length integer with an `n`-bit prefix from `src`
`crates/network/src/h2/hpack.rs:430` **fn** `encode_int` — Encode an integer with an `n`-bit prefix. The `prefix_byte` holds the
`crates/network/src/h2/hpack.rs:450` **fn** `huffman_encode` — Huffman-encode `input`. The result is padded to a byte boundary with
`crates/network/src/h2/hpack.rs:480` **fn** `huffman_decode` — Huffman-decode `input`. Padding bits (EOS prefix, all-ones) are accepted
`crates/network/src/h2/hpack.rs:523` **fn** `decode_string` — Decode a header string (literal or Huffman) from `src`
`crates/network/src/h2/hpack.rs:545` **fn** `encode_string` — Encode a header string. When `use_huffman` is true, the string is
`crates/network/src/h2/hpack.rs:569` **struct** `DynamicTable` — The dynamic table. Entries are added at the front (lowest dynamic index)
`crates/network/src/h2/hpack.rs:581` **fn** `new`
`crates/network/src/h2/hpack.rs:591` **fn** `set_max_size` — Update the maximum size (from a dynamic table size update instruction
`crates/network/src/h2/hpack.rs:597` **fn** `add` — Add a new entry, evicting old ones as needed
`crates/network/src/h2/hpack.rs:611` **fn** `get` — Return `(name, value)` for a 1-based dynamic index (1 = most recent)
`crates/network/src/h2/hpack.rs:617` **fn** `len`
`crates/network/src/h2/hpack.rs:621` **fn** `is_empty`
`crates/network/src/h2/hpack.rs:666` **struct** `HeaderField` — A decoded header field
`crates/network/src/h2/hpack.rs:675` **fn** `new`
`crates/network/src/h2/hpack.rs:683` **fn** `sensitive`
`crates/network/src/h2/hpack.rs:692` **fn** `name_str` — Returns `name` as a `&str` (UTF-8 best-effort; non-UTF-8 returns `""`)
`crates/network/src/h2/hpack.rs:697` **fn** `value_str` — Returns `value` as a `&str` (UTF-8 best-effort; non-UTF-8 returns `""`)
`crates/network/src/h2/hpack.rs:705` **struct** `Decoder` — Stateful HPACK decoder. One instance per HTTP/2 connection direction
`crates/network/src/h2/hpack.rs:712` **fn** `new`
`crates/network/src/h2/hpack.rs:721` **fn** `set_proto_max` — Update the protocol-level maximum table size (call when the remote
`crates/network/src/h2/hpack.rs:729` **fn** `decode` — Decode a complete header block fragment into a list of header fields
`crates/network/src/h2/hpack.rs:812` **struct** `Encoder` — Stateful HPACK encoder. One instance per HTTP/2 connection direction
`crates/network/src/h2/hpack.rs:819` **fn** `new`
`crates/network/src/h2/hpack.rs:826` **fn** `with_huffman`
`crates/network/src/h2/hpack.rs:833` **fn** `set_max_size` — Update the maximum dynamic table size. Emits a dynamic table size
`crates/network/src/h2/hpack.rs:844` **fn** `encode` — Encode a list of `(name, value)` pairs into a header block fragment
`crates/network/src/h2/pool.rs:35` **struct** `H2Pool` — A shared pool of HTTP/2 connections, one per origin
`crates/network/src/h2/pool.rs:40` **fn** `new`
`crates/network/src/h3/alt_svc.rs:54` **struct** `AltSvcEntry` — One parsed alternative service entry (RFC 7838 §3) for the `h3` protocol
`crates/network/src/h3/alt_svc.rs:75` **fn** `parse` — Parse an `Alt-Svc` header value into the HTTP/3 alternatives it advertises
`crates/network/src/h3/alt_svc.rs:191` **struct** `AltSvcCache` — Per-origin memory of advertised HTTP/3 alternatives (RFC 7838 §3)
`crates/network/src/h3/alt_svc.rs:198` **fn** `new` — Create an empty cache
`crates/network/src/h3/alt_svc.rs:207` **fn** `insert` — Record the first `h3` alternative from a parsed `Alt-Svc` header for
`crates/network/src/h3/alt_svc.rs:224` **fn** `get` — Look up a still-fresh HTTP/3 alternative for `origin` as of `now`
`crates/network/src/h3/alt_svc.rs:236` **fn** `remove` — Clear any cached alternative for `origin` (RFC 7838 §2.4 "broken":
`crates/network/src/h3/alt_svc.rs:241` **fn** `insert_now` — [`insert`](Self::insert) using the current wall clock
`crates/network/src/h3/alt_svc.rs:246` **fn** `get_now` — [`get`](Self::get) using the current wall clock
`crates/network/src/h3/conn_flow.rs:46` **enum** `StreamDir` — A stream's directionality, the axis `MAX_STREAMS` accounts on separately
`crates/network/src/h3/conn_flow.rs:77` **enum** `ConnError` — A connection-level protocol violation. Each variant maps to a single QUIC
`crates/network/src/h3/conn_flow.rs:131` **struct** `SendConnFlow` — Our view of the peer's connection-level flow-control limit: the sum of stream
`crates/network/src/h3/conn_flow.rs:143` **fn** `new` — Creates the send-side accounting bounded by `initial_max_data` (the peer's
`crates/network/src/h3/conn_flow.rs:149` **fn** `max_data` — The peer's current connection-level limit (RFC 9000 §4.1)
`crates/network/src/h3/conn_flow.rs:155` **fn** `sent` — The connection-wide total of stream bytes sent so far
`crates/network/src/h3/conn_flow.rs:162` **fn** `available` — How many more stream bytes the connection limit currently permits
`crates/network/src/h3/conn_flow.rs:169` **fn** `allowed` — The largest number of bytes, up to `want`, that may be sent right now
`crates/network/src/h3/conn_flow.rs:175` **fn** `on_sent` — Records that `n` stream bytes were sent across the connection
`crates/network/src/h3/conn_flow.rs:181` **fn** `update_max_data` — Raises the peer's connection limit from a received `MAX_DATA` frame
`crates/network/src/h3/conn_flow.rs:189` **fn** `is_blocked` — Whether the connection limit is exhausted (no bytes may be sent). The
`crates/network/src/h3/conn_flow.rs:196` **fn** `blocked_at` — The offset at which the sender is connection-flow-control blocked, if
`crates/network/src/h3/conn_flow.rs:208` **struct** `RecvConnFlow` — The connection-level flow-control limit we advertise and police: it caps the
`crates/network/src/h3/conn_flow.rs:223` **fn** `new` — Creates the receive-side accounting advertising `initial_max_data` bytes
`crates/network/src/h3/conn_flow.rs:229` **fn** `limit` — The connection limit currently advertised (RFC 9000 §4.1)
`crates/network/src/h3/conn_flow.rs:235` **fn** `received` — The connection-wide total of the highest offsets received so far
`crates/network/src/h3/conn_flow.rs:241` **fn** `read` — The connection-wide total of bytes the application has consumed so far
`crates/network/src/h3/conn_flow.rs:249` **fn** `record_received` — Records that a stream's highest received offset advanced by `delta` bytes
`crates/network/src/h3/conn_flow.rs:260` **fn** `record_read` — Records that the application consumed `delta` more stream bytes across the
`crates/network/src/h3/conn_flow.rs:267` **fn** `window_update` — Re-advertises the connection limit as `read + window` and returns the new
`crates/network/src/h3/conn_flow.rs:281` **struct** `SendStreamLimit` — How many streams of one direction the peer lets *us* open, and the IDs of the
`crates/network/src/h3/conn_flow.rs:295` **fn** `new` — Creates the send-side count bounded by `initial_max_streams` (the peer's
`crates/network/src/h3/conn_flow.rs:301` **fn** `dir` — The stream-count axis this tracks (RFC 9000 §4.6)
`crates/network/src/h3/conn_flow.rs:307` **fn** `opened` — How many streams of this direction we have opened so far
`crates/network/src/h3/conn_flow.rs:313` **fn** `max_streams` — The peer's current limit on streams of this direction (RFC 9000 §4.6)
`crates/network/src/h3/conn_flow.rs:320` **fn** `can_open` — Whether another stream of this direction may be opened right now
`crates/network/src/h3/conn_flow.rs:329` **fn** `open` — Opens the next client-initiated stream of this direction and returns its
`crates/network/src/h3/conn_flow.rs:344` **fn** `update_max_streams` — Raises the peer's limit from a received `MAX_STREAMS` frame
`crates/network/src/h3/conn_flow.rs:352` **fn** `is_blocked` — Whether we want to open a stream but the peer's limit forbids it — the
`crates/network/src/h3/conn_flow.rs:359` **fn** `blocked_at` — The stream-count limit at which we are blocked, if blocked
`crates/network/src/h3/conn_flow.rs:369` **struct** `RecvStreamLimit` — How many streams of one direction we let the peer open, the enforcement of
`crates/network/src/h3/conn_flow.rs:385` **fn** `new` — Creates the receive-side count advertising `initial_max_streams` (our
`crates/network/src/h3/conn_flow.rs:391` **fn** `dir` — The stream-count axis this tracks (RFC 9000 §4.6)
`crates/network/src/h3/conn_flow.rs:397` **fn** `opened` — The highest stream count of this direction the peer has opened
`crates/network/src/h3/conn_flow.rs:403` **fn** `closed` — The number of streams of this direction that have finished
`crates/network/src/h3/conn_flow.rs:409` **fn** `max_streams` — The limit currently advertised to the peer (RFC 9000 §4.6)
`crates/network/src/h3/conn_flow.rs:417` **fn** `record_open` — Records that the peer opened a stream whose cumulative count is `count`
`crates/network/src/h3/conn_flow.rs:427` **fn** `record_closed` — Records that `delta` more streams of this direction have finished
`crates/network/src/h3/conn_flow.rs:435` **fn** `window_update` — Re-advertises the limit as `closed + concurrency` and returns the new
`crates/network/src/h3/frame.rs:89` **enum** `FrameError` — Codec-level error. Each variant maps to exactly one RFC 9114 §8.1 wire error
`crates/network/src/h3/frame.rs:132` **enum** `Frame` — A parsed HTTP/3 frame (RFC 9114 §7.2). Field sections in `Headers` /
`crates/network/src/h3/frame.rs:196` **fn** `parse` — Parse one frame from the front of `buf`
`crates/network/src/h3/frame.rs:254` **fn** `encode` — Serialize this frame (type · length · payload) onto `out`
`crates/network/src/h3/loss.rs:89` **enum** `PacketNumberSpace` — One of QUIC's three packet-number spaces (RFC 9000 §12.3). Loss detection is
`crates/network/src/h3/loss.rs:104` **fn** `uses_ack_delay` — Whether acknowledgements in this space may carry a non-zero `ack_delay`
`crates/network/src/h3/loss.rs:112` **struct** `SentPacket` — A packet recorded in a [`SentPacketRegistry`] (RFC 9002 §A.1
`crates/network/src/h3/loss.rs:133` **struct** `AckOutcome` — The result of processing one ACK frame (RFC 9002 §A.7)
`crates/network/src/h3/loss.rs:149` **struct** `SentPacketRegistry` — One packet-number space's sent-packet registry and loss-detection state
`crates/network/src/h3/loss.rs:171` **fn** `new` — Creates an empty registry for the given packet-number space
`crates/network/src/h3/loss.rs:183` **fn** `space` — The packet-number space this registry tracks
`crates/network/src/h3/loss.rs:189` **fn** `largest_acked` — The largest packet number acknowledged so far, or `None` before the first
`crates/network/src/h3/loss.rs:195` **fn** `loss_time` — The earliest time a pending packet will be declared lost by the time
`crates/network/src/h3/loss.rs:201` **fn** `time_of_last_ack_eliciting_packet` — The send time of the most recent ack-eliciting packet, the PTO anchor
`crates/network/src/h3/loss.rs:207` **fn** `ack_eliciting_in_flight` — Whether any ack-eliciting packet is still outstanding (RFC 9002 §6.2.1);
`crates/network/src/h3/loss.rs:212` **fn** `outstanding` — The number of packets currently tracked (sent, not yet acked or lost)
`crates/network/src/h3/loss.rs:221` **fn** `on_packet_sent` — Records that a packet was sent (RFC 9002 §A.5 `OnPacketSent`)
`crates/network/src/h3/loss.rs:240` **fn** `on_ack_received` — Processes an ACK frame's acknowledged ranges (RFC 9002 §A.7)
`crates/network/src/h3/loss.rs:302` **fn** `detect_and_remove_lost_packets` — Declares and removes lost packets (RFC 9002 §A.10
`crates/network/src/h3/loss.rs:413` **fn** `establishes_persistent_congestion` — Whether a run of lost packets establishes persistent congestion (RFC 9002
`crates/network/src/h3/packet.rs:90` **enum** `PacketError` — Packet-header codec error. The connection layer maps these to the
`crates/network/src/h3/packet.rs:141` **enum** `Packet` — A parsed QUIC packet header plus its opaque protected region (RFC 9000 §17)
`crates/network/src/h3/packet.rs:261` **fn** `parse` — Parse one QUIC packet header (and its protected remainder) from the
`crates/network/src/h3/packet.rs:368` **fn** `encode` — Serialize this packet header and its protected region onto `out`
`crates/network/src/h3/pto.rs:68` **enum** `LossTimer` — The state the single loss-detection timer should be left in after
`crates/network/src/h3/pto.rs:80` **fn** `deadline` — The instant the timer is armed for, or `None` when [`LossTimer::Disarmed`]
`crates/network/src/h3/pto.rs:88` **fn** `is_armed` — Whether the timer is armed
`crates/network/src/h3/pto.rs:96` **enum** `TimeoutAction` — What the caller must do when the loss-detection timer fires
`crates/network/src/h3/pto.rs:124` **struct** `LossDetection` — The RFC 9002 Appendix A loss-detection and PTO state machine, tying the three
`crates/network/src/h3/pto.rs:157` **fn** `new` — Creates a fresh loss-detection state machine with the peer's advertised
`crates/network/src/h3/pto.rs:172` **fn** `registry` — The registry for `space`, borrowed immutably
`crates/network/src/h3/pto.rs:183` **fn** `registry_mut` — The registry for `space`, borrowed mutably so the caller can record sent
`crates/network/src/h3/pto.rs:192` **fn** `rtt` — The RTT estimator, borrowed immutably
`crates/network/src/h3/pto.rs:199` **fn** `rtt_mut` — The RTT estimator, borrowed mutably so the caller can fold in a sample
`crates/network/src/h3/pto.rs:204` **fn** `pto_count` — The current PTO backoff exponent (`pto_count`, RFC 9002 §6.2.1)
`crates/network/src/h3/pto.rs:211` **fn** `reset_pto_count` — Resets the PTO backoff to zero (RFC 9002 §6.2.1): done whenever a fresh RTT
`crates/network/src/h3/pto.rs:217` **fn** `set_has_handshake_keys` — Records that Handshake keys are now available (RFC 9002 §6.2.2.1); switches
`crates/network/src/h3/pto.rs:223` **fn** `set_handshake_confirmed` — Records whether the TLS handshake is confirmed (RFC 9001 §4.1.2). The
`crates/network/src/h3/pto.rs:229` **fn** `set_peer_completed_address_validation` — Records whether the peer has validated this endpoint's address (RFC 9000
`crates/network/src/h3/pto.rs:239` **fn** `discard_space` — Discards a packet-number space (RFC 9002 §A.4 `OnPacketNumberSpaceDiscarded`)
`crates/network/src/h3/pto.rs:247` **fn** `any_ack_eliciting_in_flight` — Whether any packet-number space still has an ack-eliciting packet in flight
`crates/network/src/h3/pto.rs:256` **fn** `get_loss_time_and_space` — The earliest pending time-threshold loss time and its space, or `None` when
`crates/network/src/h3/pto.rs:298` **fn** `get_pto_time_and_space` — The instant the PTO should fire and the space it belongs to, or `None` when
`crates/network/src/h3/pto.rs:343` **fn** `set_loss_detection_timer` — Computes the state the single loss-detection timer should be left in
`crates/network/src/h3/pto.rs:368` **fn** `on_timeout` — Handles the loss-detection timer firing (RFC 9002 §A.9
`crates/network/src/h3/qpack.rs:61` **enum** `QpackError` — Field-section codec error. Every variant is a decompression failure at the
`crates/network/src/h3/qpack.rs:262` **struct** `HeaderField` — A decoded header field. `sensitive` reflects the QPACK "never index" (`N`)
`crates/network/src/h3/qpack.rs:274` **fn** `new` — Build a non-sensitive field from `name`/`value`
`crates/network/src/h3/qpack.rs:280` **fn** `sensitive` — Build a field with the "never index" (`N`) bit set
`crates/network/src/h3/qpack.rs:286` **fn** `name_str` — The name as UTF-8 (best-effort; non-UTF-8 yields `""`)
`crates/network/src/h3/qpack.rs:292` **fn** `value_str` — The value as UTF-8 (best-effort; non-UTF-8 yields `""`)
`crates/network/src/h3/qpack.rs:393` **fn** `encode_field_section` — Encode a list of header fields into a QPACK field section (RFC 9204 §4.5),
`crates/network/src/h3/qpack.rs:432` **fn** `decode_field_section` — Decode a QPACK field section (RFC 9204 §4.5) that references only the static
`crates/network/src/h3/qpack_stream.rs:62` **enum** `QpackStreamError` — An error decoding an instruction stream or mutating the dynamic table
`crates/network/src/h3/qpack_stream.rs:163` **struct** `DynamicTable` — The QPACK dynamic table: a FIFO of `(name, value)` entries with a
`crates/network/src/h3/qpack_stream.rs:181` **fn** `new` — Create an empty table whose capacity starts at 0 and may be raised up to
`crates/network/src/h3/qpack_stream.rs:193` **fn** `entry_size` — The size in bytes an entry occupies (RFC 9204 §3.2.1)
`crates/network/src/h3/qpack_stream.rs:199` **fn** `capacity` — The current capacity in bytes
`crates/network/src/h3/qpack_stream.rs:205` **fn** `size` — The total size in bytes of all live entries
`crates/network/src/h3/qpack_stream.rs:211` **fn** `len` — The number of entries currently in the table
`crates/network/src/h3/qpack_stream.rs:217` **fn** `is_empty` — Whether the table currently holds no entries
`crates/network/src/h3/qpack_stream.rs:225` **fn** `insert_count` — The Insert Count — the number of insertions performed so far, equal to
`crates/network/src/h3/qpack_stream.rs:238` **fn** `get_absolute` — Look up an entry by its absolute index (RFC 9204 §3.2.4). Returns `None`
`crates/network/src/h3/qpack_stream.rs:265` **fn** `set_capacity` — Set the table capacity (RFC 9204 §3.2.3 / the Set Dynamic Table Capacity
`crates/network/src/h3/qpack_stream.rs:294` **fn** `insert` — Insert a `(name, value)` entry, evicting older entries to make room
`crates/network/src/h3/qpack_stream.rs:317` **fn** `apply` — Apply a parsed [`EncoderInstruction`] to the table, resolving name and
`crates/network/src/h3/qpack_stream.rs:357` **enum** `EncoderInstruction` — An instruction on the QPACK encoder stream (RFC 9204 §4.3)
`crates/network/src/h3/qpack_stream.rs:385` **fn** `encode` — Serialize this instruction onto `out`. `use_huffman` enables Huffman
`crates/network/src/h3/qpack_stream.rs:416` **fn** `decode` — Parse a single instruction from the front of `src`, returning it and the
`crates/network/src/h3/qpack_stream.rs:446` **fn** `decode_encoder_stream` — Decode a full encoder-stream buffer into a list of instructions
`crates/network/src/h3/qpack_stream.rs:463` **enum** `DecoderInstruction` — An instruction on the QPACK decoder stream (RFC 9204 §4.4)
`crates/network/src/h3/qpack_stream.rs:474` **fn** `encode` — Serialize this instruction onto `out`
`crates/network/src/h3/qpack_stream.rs:492` **fn** `decode` — Parse a single instruction from the front of `src`, returning it and the
`crates/network/src/h3/qpack_stream.rs:512` **fn** `decode_decoder_stream` — Decode a full decoder-stream buffer into a list of instructions
`crates/network/src/h3/quic_frame.rs:122` **enum** `QuicFrameError` — Frame-codec error. Every variant is a `FRAME_ENCODING_ERROR` at the QUIC
`crates/network/src/h3/quic_frame.rs:174` **struct** `AckRange` — A single additional ACK range in an ACK frame (RFC 9000 §19.3.1). The first
`crates/network/src/h3/quic_frame.rs:185` **struct** `EcnCounts` — ECN counts carried by an ACK frame of type `0x03` (RFC 9000 §19.3.2)
`crates/network/src/h3/quic_frame.rs:200` **enum** `Frame` — A parsed QUIC transport frame (RFC 9000 §19). Variable-length payloads
`crates/network/src/h3/quic_frame.rs:343` **fn** `parse` — Parse exactly one frame from the front of `input`
`crates/network/src/h3/quic_frame.rs:443` **fn** `encode` — Serialize this frame onto `out`. STREAM frames always emit an explicit
`crates/network/src/h3/quic_frame.rs:596` **fn** `parse_all` — Parse a full packet payload — a sequence of frames back to back — into a
`crates/network/src/h3/quic_frame.rs:611` **fn** `encode_all` — Serialize a sequence of frames back to back onto `out` (RFC 9000 §12.4)
`crates/network/src/h3/recovery.rs:87` **struct** `RttEstimator` — The round-trip-time estimator of RFC 9002 §5
`crates/network/src/h3/recovery.rs:110` **fn** `new` — Creates an estimator seeded with [`K_INITIAL_RTT`] (RFC 9002 §6.2.2):
`crates/network/src/h3/recovery.rs:121` **fn** `latest_rtt` — The most recent RTT sample (RFC 9002 §5.1)
`crates/network/src/h3/recovery.rs:127` **fn** `min_rtt` — The minimum RTT seen so far (RFC 9002 §5.2); [`Duration::ZERO`] before
`crates/network/src/h3/recovery.rs:132` **fn** `smoothed_rtt` — The smoothed RTT estimate (RFC 9002 §5.3)
`crates/network/src/h3/recovery.rs:137` **fn** `rttvar` — The RTT variance estimate (RFC 9002 §5.3)
`crates/network/src/h3/recovery.rs:143` **fn** `has_sample` — Whether a measured RTT sample has been folded in (as opposed to the
`crates/network/src/h3/recovery.rs:153` **fn** `update_rtt` — Folds a new RTT sample into the estimate (RFC 9002 §5.3)
`crates/network/src/h3/recovery.rs:198` **fn** `pto` — The probe timeout (RFC 9002 §6.2.1):
`crates/network/src/h3/recovery.rs:205` **struct** `LostPacket` — A packet reported lost to [`CongestionController::on_packets_lost`]
`crates/network/src/h3/recovery.rs:219` **struct** `CongestionController` — The NewReno congestion controller of RFC 9002 §7 (pseudocode Appendix B)
`crates/network/src/h3/recovery.rs:241` **fn** `new` — Creates a controller for the given `max_datagram_size` (bytes), with the
`crates/network/src/h3/recovery.rs:259` **fn** `minimum_window` — The minimum congestion window (RFC 9002 §7.2): `2 * max_datagram_size`
`crates/network/src/h3/recovery.rs:264` **fn** `congestion_window` — The current congestion window in bytes (RFC 9002 §7)
`crates/network/src/h3/recovery.rs:269` **fn** `bytes_in_flight` — The bytes currently in flight (sent but unacknowledged)
`crates/network/src/h3/recovery.rs:275` **fn** `ssthresh` — The slow-start threshold, or `None` while it is still infinite (before
`crates/network/src/h3/recovery.rs:281` **fn** `in_slow_start` — Whether the controller is in slow start (`congestion_window < ssthresh`,
`crates/network/src/h3/recovery.rs:287` **fn** `available_window` — The number of additional bytes that may be sent right now
`crates/network/src/h3/recovery.rs:293` **fn** `can_send` — Whether at least one more datagram may be sent under the current window
`crates/network/src/h3/recovery.rs:299` **fn** `on_packet_sent` — Records that `sent_bytes` of ack-eliciting, in-flight data left the host
`crates/network/src/h3/recovery.rs:319` **fn** `on_packet_acked` — Processes one newly acknowledged packet (RFC 9002 Appendix B.5)
`crates/network/src/h3/recovery.rs:343` **fn** `on_congestion_event` — Enters (or stays in) a recovery period in response to a congestion signal
`crates/network/src/h3/recovery.rs:359` **fn** `on_packets_lost` — Processes a batch of lost packets (RFC 9002 Appendix B.7
`crates/network/src/h3/recovery.rs:380` **fn** `on_persistent_congestion` — Collapses the window to the minimum on established persistent congestion
`crates/network/src/h3/recovery.rs:393` **fn** `persistent_congestion_duration` — The persistent-congestion period (RFC 9002 §7.6.1):
`crates/network/src/h3/stream.rs:71` **enum** `StreamError` — A stream-layer protocol violation. Each variant maps to a single QUIC
`crates/network/src/h3/stream.rs:144` **enum** `RecvState` — The state of the receiving part of a stream (RFC 9000 §3.2)
`crates/network/src/h3/stream.rs:163` **struct** `RecvStream` — The receiving half of a QUIC stream: reassembly buffer, receive flow-control
`crates/network/src/h3/stream.rs:189` **fn** `new` — Creates a receive stream advertising `initial_max_data` bytes of receive
`crates/network/src/h3/stream.rs:203` **fn** `state` — The current receive state (RFC 9000 §3.2)
`crates/network/src/h3/stream.rs:209` **fn** `read_offset` — The next offset the application will read (bytes below are delivered)
`crates/network/src/h3/stream.rs:215` **fn** `max_data` — The currently advertised receive flow-control limit (RFC 9000 §4.1)
`crates/network/src/h3/stream.rs:221` **fn** `is_readable` — Whether contiguous data is available to [`Self::read`]
`crates/network/src/h3/stream.rs:227` **fn** `reset_error` — The application error code if the stream was reset (RFC 9000 §19.4)
`crates/network/src/h3/stream.rs:233` **fn** `is_finished` — Whether the application has consumed the whole stream (`DataRead`)
`crates/network/src/h3/stream.rs:245` **fn** `recv` — Processes a received STREAM frame (RFC 9000 §19.8): `offset`/`data` is the
`crates/network/src/h3/stream.rs:295` **fn** `recv_reset` — Processes a received RESET_STREAM frame (RFC 9000 §19.4): the peer aborts
`crates/network/src/h3/stream.rs:318` **fn** `read` — Pops and returns the contiguous readable prefix, advancing the read
`crates/network/src/h3/stream.rs:334` **fn** `window_update` — Re-advertises the receive flow-control limit as `read_offset + window`
`crates/network/src/h3/stream.rs:430` **enum** `SendState` — The state of the sending part of a stream (RFC 9000 §3.1)
`crates/network/src/h3/stream.rs:447` **struct** `StreamChunk` — A STREAM frame to transmit, produced by [`SendStream::poll_transmit`]
`crates/network/src/h3/stream.rs:459` **struct** `SendStream` — The sending half of a QUIC stream: outgoing buffer, send flow-control
`crates/network/src/h3/stream.rs:485` **fn** `new` — Creates a send stream bounded by `initial_max_data` (the peer's
`crates/network/src/h3/stream.rs:501` **fn** `state` — The current send state (RFC 9000 §3.1)
`crates/network/src/h3/stream.rs:507` **fn** `max_data` — The peer's current flow-control limit for this stream (RFC 9000 §4.1)
`crates/network/src/h3/stream.rs:513` **fn** `write_offset` — The total number of bytes written by the application so far
`crates/network/src/h3/stream.rs:519` **fn** `write` — Queues application `data` for transmission (RFC 9000 §3.1). Ignored once
`crates/network/src/h3/stream.rs:535` **fn** `finish` — Marks the end of the stream (RFC 9000 §3.1). No further [`Self::write`]
`crates/network/src/h3/stream.rs:547` **fn** `update_max_data` — Raises the peer's flow-control limit from a received MAX_STREAM_DATA
`crates/network/src/h3/stream.rs:561` **fn** `is_blocked` — Whether the stream has unsent data but is blocked by flow control
`crates/network/src/h3/stream.rs:568` **fn** `blocked_at` — The offset at which the sender is flow-control blocked, if blocked
`crates/network/src/h3/stream.rs:579` **fn** `poll_transmit` — Produces the next STREAM frame to transmit, at most `max_len` data bytes
`crates/network/src/h3/stream.rs:611` **fn** `on_ack` — Records that the byte range `[offset, offset + len)` was acknowledged,
`crates/network/src/h3/stream.rs:630` **fn** `reset` — Abruptly terminates the sending half with `error_code`, discarding any
`crates/network/src/h3/stream.rs:640` **fn** `on_reset_ack` — Acknowledges the RESET_STREAM, moving to `ResetRecvd` (RFC 9000 §3.1)
`crates/network/src/h3/stream.rs:648` **fn** `reset_error` — The application error code if the stream was reset (RFC 9000 §19.4)
`crates/network/src/h3/varint.rs:22` **struct** `VarIntTooLarge` — Error returned by [`encode`] when a value does not fit in a QUIC varint
`crates/network/src/h3/varint.rs:35` **fn** `encoded_len` — Number of bytes the varint encoding of `value` occupies (1, 2, 4, or 8), or
`crates/network/src/h3/varint.rs:51` **fn** `encode` — Append the shortest QUIC varint encoding of `value` to `out`
`crates/network/src/h3/varint.rs:69` **fn** `decode` — Decode one QUIC varint from the front of `buf`
`crates/network/src/hsts_preload.rs:23` **struct** `HstsPreloadList` — HSTS Preload List: быстрый поиск по eTLD+1
`crates/network/src/hsts_preload.rs:36` **fn** `load` — Создать preload list из встроенного JSON (Chromium формат)
`crates/network/src/hsts_preload.rs:100` **fn** `is_preloaded` — Проверить, есть ли хост в preload list
`crates/network/src/hsts_preload.rs:128` **fn** `get_preload_list` — Получить глобальный preload list
`crates/network/src/http/client_hints.rs:14` **enum** `ClientHintsProfile` — Client Hints profile — determines which hints to send
`crates/network/src/http/client_hints.rs:23` **fn** `for_http_profile` — Create ClientHintsProfile for the given HTTP profile
`crates/network/src/http/client_hints.rs:40` **fn** `should_send_client_hints` — Determine whether to send Client Hints headers for the given HTTP profile
`crates/network/src/http/client_hints.rs:56` **fn** `client_hints_headers` — Build Client Hints headers for the given UA string (Lumen)
`crates/network/src/http/h2_settings.rs:11` **struct** `H2Settings` — HTTP/2 SETTINGS frame values matching Chrome's configuration
`crates/network/src/http/h2_settings.rs:33` **fn** `for_profile` — Create HTTP/2 SETTINGS for the given profile
`crates/network/src/http/h2_settings.rs:108` **fn** `to_wire_format` — Convert SETTINGS to HTTP/2 wire format: list of (id, value) pairs
`crates/network/src/http/h2_settings.rs:145` **struct** `H2StreamPriority` — HTTP/2 stream priority information for matching Chrome's priority tree
`crates/network/src/http/h2_settings.rs:158` **fn** `default_for_profile` — Create default HTTP/2 stream priority for the root stream
`crates/network/src/http/h2_settings.rs:169` **fn** `to_wire_format` — Convert priority to HTTP/2 wire format (PRIORITY frame payload)
`crates/network/src/http/headers.rs:14` **enum** `HttpProfile` — HTTP profile — determines header order, casing, and HTTP/2 SETTINGS configuration
`crates/network/src/http/headers.rs:53` **struct** `HeaderOrder` — Chrome HTTP/1.1 header order (in request)
`crates/network/src/http/headers.rs:59` **fn** `new` — Create a new header order builder for the given profile
`crates/network/src/http/headers.rs:69` **fn** `add` — Add a header (key, value) to the ordered list
`crates/network/src/http/headers.rs:83` **fn** `to_http_block` — Build the HTTP/1.1 header block string for the request line
`crates/network/src/http/headers.rs:96` **fn** `as_tuples` — Return headers as a list of tuples
`crates/network/src/http/headers.rs:101` **fn** `clear` — Clear all headers
`crates/network/src/http/headers.rs:117` **fn** `build_request_headers` — Build HTTP/1.1 request headers for the given profile
`crates/network/src/http/headers.rs:290` **fn** `h2_fingerprint_headers` — Build the browser-fingerprint request headers for the HTTP/2 path as
`crates/network/src/http_cache.rs:27` **struct** `CacheControl` — Parsed subset of `Cache-Control` response directives
`crates/network/src/http_cache.rs:42` **fn** `parse` — Parse `Cache-Control` response header value
`crates/network/src/http_cache.rs:62` **fn** `max_age_secs` — Effective freshness lifetime. s-maxage takes precedence over max-age
`crates/network/src/http_cache.rs:89` **struct** `CacheEntry` — A single stored HTTP response (in-memory representation)
`crates/network/src/http_cache.rs:109` **fn** `is_fresh` — True if the entry is fresh and can be served without revalidation
`crates/network/src/http_cache.rs:118` **fn** `conditional_headers` — Build conditional GET headers to revalidate this entry
`crates/network/src/http_cache.rs:137` **struct** `CacheEntrySnapshot` — Owned snapshot of a cache entry returned by `HttpCacheBackend::get`
`crates/network/src/http_cache.rs:160` **trait** `HttpCacheBackend` — Shared interface for HTTP cache backends (in-memory and disk)
`crates/network/src/http_cache.rs:195` **struct** `HttpCache`
`crates/network/src/http_cache.rs:202` **fn** `new` — Create an empty cache with LRU eviction and 50 MB limit
`crates/network/src/http_cache.rs:211` **fn** `len` — Number of entries currently stored
`crates/network/src/http_cache.rs:216` **fn** `is_empty`
`crates/network/src/http_cache.rs:350` **enum** `CacheLookup` — `CacheLookup` is unused externally; we use `get()` which returns `Option<CacheEntrySnapshot>`
`crates/network/src/http_cache.rs:360` **enum** `DiskCacheError` — Error type for [`DiskHttpCache`] operations
`crates/network/src/http_cache.rs:390` **struct** `DiskHttpCache` — SQLite-backed HTTP cache that survives browser restarts (RFC 7234 Phase 1)
`crates/network/src/http_cache.rs:399` **fn** `new` — Open or create a cache database at `path`
`crates/network/src/http_cache.rs:423` **fn** `open_default` — Open or create the default cache database at [`lumen_cache_dir`]`/http_cache.db`
`crates/network/src/http_cache.rs:567` **fn** `lumen_cache_dir` — Returns the Lumen cache directory for the current user
`crates/network/src/lib.rs:100` **fn** `set_global_adblock_enabled` — Enable or disable the process-global ad-block filter
`crates/network/src/lib.rs:106` **fn** `global_adblock_enabled` — Whether the process-global ad-block filter is currently enabled
`crates/network/src/lib.rs:115` **fn** `install_global_adblock_filter` — Install (or replace) the process-global ad-block filter
`crates/network/src/lib.rs:2217` **struct** `HttpProxy` — HTTP proxy configuration (RFC 7230 proxy behavior)
`crates/network/src/lib.rs:2229` **fn** `new` — Создать новый прокси без аутентификации
`crates/network/src/lib.rs:2238` **fn** `with_basic_auth` — Создать прокси с базовой аутентификацией (username:password)
`crates/network/src/lib.rs:2281` **struct** `HttpClient` — HTTP/1.1 + HTTPS клиент
`crates/network/src/lib.rs:2320` **fn** `new`
`crates/network/src/lib.rs:2346` **fn** `with_sink` — Подключить EventSink. По умолчанию sink-а нет (события не эмитятся)
`crates/network/src/lib.rs:2357` **fn** `with_filter` — Подключить RequestFilter. По умолчанию фильтра нет — `fetch` всегда
`crates/network/src/lib.rs:2369` **fn** `with_interceptor` — Подключить Service Worker перехватчик fetch-запросов. Проверяется
`crates/network/src/lib.rs:2378` **fn** `with_pool` — Подключить shared `ConnectionPool`. По умолчанию у каждого `HttpClient`
`crates/network/src/lib.rs:2388` **fn** `with_h2_pool` — Подключить shared `H2Pool` (RFC 9113 §9.1.1). По умолчанию HTTP/2
`crates/network/src/lib.rs:2397` **fn** `with_dns_resolver` — Подключить DNS-резолвер. По умолчанию — `SystemDnsResolver` (через
`crates/network/src/lib.rs:2414` **fn** `with_hsts` — Подключить HSTS-store (RFC 6797). По умолчанию — нет:
`crates/network/src/lib.rs:2430` **fn** `with_credentials` — Подключить credential-провайдер для HTTP authentication (RFC 7235 /
`crates/network/src/lib.rs:2441` **fn** `with_tab` — Указать `TabId`, который попадёт в каждое emit-ое событие. В Phase 0
`crates/network/src/lib.rs:2461` **fn** `with_mixed_content_policy` — Подключить mixed-content policy (W3C Mixed Content §5). По умолчанию
`crates/network/src/lib.rs:2485` **fn** `with_content_decoder` — Зарегистрировать `ContentDecoder` для одного encoding. Декодер попадает
`crates/network/src/lib.rs:2531` **fn** `with_cors_cache` — Запросить только диапазон байт ресурса (RFC 7233). Если сервер
`crates/network/src/lib.rs:2543` **fn** `with_cookie_jar` — Attach a cookie store. The provider receives `Cookie:` injection
`crates/network/src/lib.rs:2567` **fn** `with_http_cache` — Подключить HTTP response cache (RFC 7234)
`crates/network/src/lib.rs:2578` **fn** `with_proxy` — Подключить HTTP прокси (RFC 7230). По умолчанию прокси не подключён — запросы
`crates/network/src/lib.rs:2591` **fn** `with_socks5_proxy` — Подключить SOCKS5 прокси (RFC 1928) для туннелирования всех TCP-соединений
`crates/network/src/lib.rs:2602` **fn** `with_fingerprint_profile` — Установить HTTP fingerprinting profile (Standard/Strict/Tor) для Chrome-matching
`crates/network/src/lib.rs:2610` **fn** `fingerprint_profile` — Получить текущий HTTP fingerprinting profile
`crates/network/src/lib.rs:2621` **fn** `with_tls_profile` — Override the TLS fingerprint profile independently of the HTTP profile
`crates/network/src/lib.rs:2627` **fn** `tls_profile` — Получить текущий TLS fingerprinting profile
`crates/network/src/lib.rs:2661` **fn** `fetch_cors` — CORS-enabled fetch для cross-origin subresource (Fetch §3-§4)
`crates/network/src/lib.rs:2710` **fn** `fetch_range`
`crates/network/src/lib.rs:2778` **fn** `fetch_multi_range` — Multi-range запрос (RFC 7233 §4.1). Один request на несколько
`crates/network/src/lib.rs:2865` **fn** `fetch_subresource` — Загрузить подресурс с проверкой mixed-content по подключённой
`crates/network/src/lib.rs:2965` **fn** `fetch_conditional` — Perform a **conditional GET** (RFC 7232) and report whether the resource
`crates/network/src/lib.rs:3021` **enum** `ConditionalFetch` — Outcome of [`HttpClient::fetch_conditional`]
`crates/network/src/lib.rs:3041` **fn** `fetch_page` — Fetch a top-level page and return the response body together with all
`crates/network/src/lib.rs:3102` **fn** `fetch_page_streaming` — Как [`HttpClient::fetch_page`], но тело финального 2xx-ответа стримится
`crates/network/src/lib.rs:3702` **struct** `InMemoryFetchInterceptor` — In-memory реализация `FetchInterceptor` для тестов без SQLite
`crates/network/src/lib.rs:3708` **fn** `new`
`crates/network/src/lib.rs:3715` **fn** `insert` — Добавить запись: ответ для (origin, url) берётся из кэша без сети
`crates/network/src/mixed_content.rs:33` **enum** `RequestDestination` — Назначение подресурса по Fetch spec §3.2.7 «request destination» —
`crates/network/src/mixed_content.rs:59` **enum** `MixedContentLevel` — Mixed-content уровень для запроса в secure-контексте
`crates/network/src/mixed_content.rs:75` **fn** `is_strict_blocked` — Должны ли мы блокировать запрос по строгому режиму. По умолчанию
`crates/network/src/mixed_content.rs:82` **fn** `is_spec_default_blocked` — Должны ли мы блокировать запрос по spec-default режиму
`crates/network/src/mixed_content.rs:110` **fn** `classify_subresource_request` — Классификация подресурса для secure top-level контекста
`crates/network/src/mixed_content.rs:146` **enum** `MixedContentMode` — Режим enforcement-а для mixed-content в `HttpClient`. Классификатор
`crates/network/src/mixed_content.rs:167` **struct** `MixedContentPolicy` — Связка top-level origin + режим, передаваемая в `HttpClient` через
`crates/network/src/mixed_content.rs:173` **fn** `new`
`crates/network/src/mixed_content.rs:177` **fn** `top_level`
`crates/network/src/mixed_content.rs:181` **fn** `mode`
`crates/network/src/mixed_content.rs:188` **fn** `evaluate` — Возвращает `Some(level)`, если запрос подресурса должен быть
`crates/network/src/mixed_content.rs:209` **fn** `block_reason` — Текстовая причина для `Event::RequestBlocked.reason` — стабильный формат
`crates/network/src/mock.rs:33` **struct** `MockTransport` — Mock HTTP транспорт — перехватывает запросы и возвращает fixture-данные
`crates/network/src/mock.rs:39` **fn** `new` — Создать пустой mock транспорт без зарегистрированных фиксатур
`crates/network/src/mock.rs:53` **fn** `add_fixture` — Зарегистрировать fixture-данные для URL
`crates/network/src/mock.rs:63` **fn** `fixture_count` — Получить текущее количество зарегистрированных фиксатур
`crates/network/src/origin.rs:28` **struct** `Origin` — «Tuple origin» = `(scheme, host, port)`. Сравнение — компонент-к-компоненту,
`crates/network/src/origin.rs:36` **enum** `OriginError` — Ошибки извлечения origin из URL
`crates/network/src/origin.rs:61` **fn** `from_url` — Извлечь tuple origin из `Url`. Возвращает `Err(OriginError::Opaque)`
`crates/network/src/origin.rs:90` **fn** `new` — Конструктор из готовых компонентов (для тестов и внутренних случаев,
`crates/network/src/origin.rs:98` **fn** `scheme`
`crates/network/src/origin.rs:102` **fn** `host`
`crates/network/src/origin.rs:106` **fn** `port`
`crates/network/src/origin.rs:117` **fn** `same_origin` — Same-origin сравнение по HTML LS §7.5 «same origin» для tuple-origin-ов:
`crates/network/src/origin.rs:130` **fn** `is_potentially_trustworthy` — «Potentially trustworthy origin» по W3C Secure Contexts §3.1:
`crates/network/src/origin.rs:145` **fn** `serialize` — Сериализация origin в каноническую форму для заголовков HTTP (`Origin:`,
`crates/network/src/permissions_policy.rs:14` **enum** `PermissionsAllowlist` — The allowlist for a single feature in a [`PermissionsPolicy`]
`crates/network/src/permissions_policy.rs:28` **struct** `PermissionsPolicy` — Parsed representation of a `Permissions-Policy` (or `Feature-Policy`) header
`crates/network/src/permissions_policy.rs:38` **fn** `allows_feature` — Returns `true` if `feature` is allowed for the given `origin`
`crates/network/src/permissions_policy.rs:51` **fn** `features` — Returns all feature names listed in this policy
`crates/network/src/permissions_policy.rs:56` **fn** `allowed_features` — Returns feature names for which the current document origin (`"self"`) is allowed
`crates/network/src/permissions_policy.rs:76` **fn** `parse_permissions_policy_header` — Parse the value of a `Permissions-Policy` header
`crates/network/src/permissions_policy.rs:96` **fn** `parse_feature_policy_header` — Parse the legacy `Feature-Policy` header (space-separated, semicolon-delimited)
`crates/network/src/pool.rs:60` **struct** `ConnectionPool` — Потокобезопасный пул keep-alive соединений. По умолчанию пуст; заполняется
`crates/network/src/pool.rs:65` **fn** `new`
`crates/network/src/pool.rs:109` **fn** `idle_count` — Сколько idle-соединений сейчас в пуле для данного origin-а. Удобно
`crates/network/src/range.rs:32` **enum** `RangeSpec` — Спецификация запрашиваемого диапазона байт (inclusive по обоим концам
`crates/network/src/range.rs:49` **fn** `closed` — Закрытый диапазон `[start; end]` inclusive по обоим концам
`crates/network/src/range.rs:54` **fn** `from` — Открытый диапазон от `start` до конца ресурса
`crates/network/src/range.rs:61` **fn** `suffix` — Suffix-range: последние `length` байт ресурса. RFC 7233 §2.1
`crates/network/src/range.rs:86` **enum** `RangeRequest` — Запрос range-байт, single- или multi-. `Multi(vec)` сериализуется в
`crates/network/src/range.rs:133` **enum** `RangeValidator` — Validator для `If-Range` header (RFC 7233 §3.2). Либо ETag (`"abc"`,
`crates/network/src/range.rs:158` **struct** `ContentRange` — Разобранный `Content-Range: bytes START-END/TOTAL` (RFC 7233 §4.2)
`crates/network/src/range.rs:168` **fn** `parse_content_range` — Парсер `Content-Range: bytes START-END/TOTAL`. Поддерживает обе формы
`crates/network/src/range.rs:189` **struct** `RangeResponse` — Ответ на range-запрос. `status = 206` — Range honored (Content-Range
`crates/network/src/range.rs:199` **struct** `RangePart` — Один part в multipart/byteranges-ответе (или единственный part в случае
`crates/network/src/range.rs:209` **struct** `MultiRangeResponse` — Ответ на multi-range запрос. Caller получает единый список parts,
`crates/network/src/range.rs:223` **fn** `parse_boundary_from_content_type` — Извлечь boundary-токен из значения `Content-Type` (RFC 7231 §3.1.1.1 +
`crates/network/src/range.rs:265` **fn** `parse_multipart_byteranges` — Парсер multipart/byteranges body (RFC 7233 §A + RFC 2046 §5.1.1)
`crates/network/src/remote.rs:23` **struct** `RemoteNetworkTransport` — Реализация `NetworkTransport`, делегирующая HTTP-запросы в отдельный процесс
`crates/network/src/remote.rs:30` **fn** `connect` — Подключиться к сетевому сервису, слушающему на `127.0.0.1:port`
`crates/network/src/socks5.rs:22` **struct** `Socks5Proxy` — SOCKS5 proxy server address and optional credentials
`crates/network/src/socks5.rs:33` **fn** `new` — Create a new SOCKS5 proxy without authentication
`crates/network/src/socks5.rs:42` **fn** `with_auth` — Attach username / password credentials (RFC 1929)
`crates/network/src/socks5.rs:56` **fn** `socks5_connect` — Perform a SOCKS5 handshake on `stream` and request a `CONNECT` to
`crates/network/src/sse.rs:36` **struct** `SseParser` — Incremental `text/event-stream` parser
`crates/network/src/sse.rs:47` **fn** `new`
`crates/network/src/sse.rs:53` **fn** `push_bytes` — Feed a chunk of bytes from the stream; returns any events that
`crates/network/src/sse.rs:175` **fn** `last_event_id` — Current last-event-id (persists across dispatched events, needed for
`crates/network/src/tls/fingerprint.rs:116` **struct** `CertInfo` — X.509 certificate information extracted after a TLS handshake
`crates/network/src/tls/fingerprint.rs:140` **fn** `is_populated` — Return `true` when the cert info was populated (subject_cn is non-empty)
`crates/network/src/tls/fingerprint.rs:147` **fn** `stub_for` — Build a stub `CertInfo` for a given hostname (Phase 0 placeholder)
`crates/network/src/tls/fingerprint.rs:170` **struct** `TlsHandshakeInfo` — TLS handshake parameters extracted from a ClientHello for fingerprinting
`crates/network/src/tls/fingerprint.rs:208` **fn** `ja3_raw_string` — JA3 raw string (pre-MD5 input)
`crates/network/src/tls/fingerprint.rs:240` **fn** `ja4_raw_string` — JA4_r (raw JA4) string — human-readable without SHA256 hashing
`crates/network/src/tls/fingerprint.rs:328` **fn** `is_grease` — Returns `true` if `v` is a GREASE value (RFC 8701)
`crates/network/src/tls/fingerprint.rs:340` **struct** `ChromeJa3Snapshot` — Reference Chrome 130 TLS ClientHello parameters for JA3 snapshot testing
`crates/network/src/tls/fingerprint.rs:404` **struct** `JA4ChromeSnapshot` — Reference Chrome 130 JA4_r parameters for snapshot testing
`crates/network/src/tls/mod.rs:30` **enum** `TlsProfile` — TLS fingerprint profile — controls cipher suites, kx_groups, ALPN, and
`crates/network/src/tls/mod.rs:47` **fn** `http_to_tls_profile` — Map an `HttpProfile` to the corresponding `TlsProfile`
`crates/network/src/tls/mod.rs:64` **fn** `build_client_config` — Build a `ClientConfig` for the given `TlsProfile`
`crates/network/src/webauthn.rs:62` **struct** `VirtualAuthenticator` — In-memory software authenticator: generates and stores ES256 passkeys and
`crates/network/src/webauthn.rs:69` **fn** `new` — Create an empty authenticator with no registered credentials
`crates/network/src/webauthn.rs:74` **fn** `credential_count` — Number of credentials currently registered (test / introspection helper)

## lumen-paint  (355 symbols)

`crates/engine/paint/src/atlas.rs:35` **struct** `AtlasKey` — Композитный ключ glyph-кэша. См. module-level docs
`crates/engine/paint/src/atlas.rs:43` **fn** `new`
`crates/engine/paint/src/atlas.rs:53` **fn** `hash_coords` — Стабильный 64-битный хэш normalized variation coords для cache key
`crates/engine/paint/src/atlas.rs:67` **struct** `GlyphEntry`
`crates/engine/paint/src/atlas.rs:78` **struct** `GlyphAtlas`
`crates/engine/paint/src/atlas.rs:97` **fn** `new`
`crates/engine/paint/src/atlas.rs:112` **fn** `width`
`crates/engine/paint/src/atlas.rs:115` **fn** `height`
`crates/engine/paint/src/atlas.rs:118` **fn** `pixels`
`crates/engine/paint/src/atlas.rs:122` **fn** `dirty`
`crates/engine/paint/src/atlas.rs:125` **fn** `mark_clean`
`crates/engine/paint/src/atlas.rs:129` **fn** `get`
`crates/engine/paint/src/atlas.rs:134` **fn** `access` — Обновляет timestamp доступа для существующей записи
`crates/engine/paint/src/atlas.rs:144` **fn** `get_lru_candidates` — Возвращает список ключей отсортированных по last_accessed (от самого старого к новому)
`crates/engine/paint/src/atlas.rs:154` **fn** `remove_keys` — Удаляет записи с указанными ключами из кэша
`crates/engine/paint/src/atlas.rs:168` **fn** `insert` — Кладёт растеризованный глиф в атлас. Возвращает `None` если место
`crates/engine/paint/src/atlas.rs:232` **fn** `on_memory_pressure` — React to an OS memory pressure event by evicting glyphs from the cache
`crates/engine/paint/src/backdrop_cache.rs:49` **struct** `BackdropCache` — Tracks freshness of cached `backdrop-filter` textures
`crates/engine/paint/src/backdrop_cache.rs:64` **fn** `new` — Creates an enabled cache with [`DEFAULT_BUDGET_BYTES`]
`crates/engine/paint/src/backdrop_cache.rs:70` **fn** `with_budget` — Creates an enabled cache with a custom GPU memory budget (bytes)
`crates/engine/paint/src/backdrop_cache.rs:82` **fn** `set_enabled` — Enables or disables the cache. Disabling clears all entries so the
`crates/engine/paint/src/backdrop_cache.rs:91` **fn** `is_enabled` — Whether the cache is currently active
`crates/engine/paint/src/backdrop_cache.rs:101` **fn** `lookup` — Returns `true` (cache HIT) if an entry for `ordinal` exists with a
`crates/engine/paint/src/backdrop_cache.rs:122` **fn** `store` — Records that `ordinal` now holds freshly produced content for
`crates/engine/paint/src/backdrop_cache.rs:142` **fn** `invalidate` — Drops the metadata entry for `ordinal`, if any. Returns `true` if an
`crates/engine/paint/src/backdrop_cache.rs:152` **fn** `clear` — Removes all entries. The renderer drops every backing texture in lockstep
`crates/engine/paint/src/backdrop_cache.rs:163` **fn** `on_memory_pressure` — Responds to a memory-pressure signal. Returns the ordinals whose textures
`crates/engine/paint/src/backdrop_cache.rs:178` **fn** `len` — Number of live cache entries
`crates/engine/paint/src/backdrop_cache.rs:184` **fn** `is_empty` — Whether the cache holds no entries
`crates/engine/paint/src/backdrop_cache.rs:190` **fn** `used_bytes` — Total GPU memory tracked by live entries, in bytes
`crates/engine/paint/src/backdrop_cache.rs:196` **fn** `budget_bytes` — Configured eviction budget, in bytes
`crates/engine/paint/src/backend.rs:39` **enum** `RenderError` — Ошибка рендера — возвращается из [`RenderBackend::render`]
`crates/engine/paint/src/backend.rs:79` **trait** `RenderBackend` — Стабильный интерфейс GPU-рендера для Lumen
`crates/engine/paint/src/backends/compare_backend.rs:35` **struct** `DiffResult` — Результат pixel-diff сравнения двух бэкендов
`crates/engine/paint/src/backends/compare_backend.rs:53` **fn** `diff_percent` — Доля отличающихся пикселей в процентах (0.0 – 100.0)
`crates/engine/paint/src/backends/compare_backend.rs:61` **fn** `is_identical` — `true` если бэкенды дали побитово идентичные результаты
`crates/engine/paint/src/backends/compare_backend.rs:68` **fn** `format` — Форматирует результат в строку для логов
`crates/engine/paint/src/backends/compare_backend.rs:80` **fn** `compute` — Вычисляет DiffResult из двух RGBA8-буферов одинакового размера
`crates/engine/paint/src/backends/compare_backend.rs:129` **struct** `CompareBackend` — Тестовый бэкенд: рендерит двумя бэкендами + вычисляет pixel-diff
`crates/engine/paint/src/backends/compare_backend.rs:145` **fn** `new` — Создаёт CompareBackend из двух headless-бэкендов
`crates/engine/paint/src/backends/compare_backend.rs:153` **fn** `last_diff` — Возвращает результат pixel-diff последнего render-а
`crates/engine/paint/src/backends/compare_backend.rs:158` **fn** `primary` — Предоставляет read-only доступ к первичному бэкенду
`crates/engine/paint/src/backends/compare_backend.rs:163` **fn** `secondary` — Предоставляет read-only доступ к вторичному бэкенду
`crates/engine/paint/src/backends/cpu_backend.rs:31` **struct** `CpuBackend` — Headless CPU-бэкенд на tiny-skia: детерминированный рендер без GPU
`crates/engine/paint/src/backends/cpu_backend.rs:44` **fn** `new` — Создаёт headless CPU-бэкенд с заданным размером поверхности
`crates/engine/paint/src/backends/cpu_backend.rs:49` **fn** `last_image` — Возвращает Image из последнего рендера, если он был выполнен
`crates/engine/paint/src/backends/femtovg_backend.rs:373` **struct** `FemtovgBackend` — femtovg/OpenGL рендер-бэкенд (Phase 2, ADR-010)
`crates/engine/paint/src/backends/femtovg_backend.rs:1125` **fn** `new` — Создаёт оконный femtovg-бэкенд из winit-окна
`crates/engine/paint/src/backends/vello_backend.rs:43` **struct** `VelloBackend` — Phase 3 рендер-бэкенд на базе Vello (ADR-010, RB-7 заглушка)
`crates/engine/paint/src/backends/vello_backend.rs:57` **fn** `new` — Создаёт заглушку `VelloBackend` с начальным размером поверхности
`crates/engine/paint/src/backends/wgpu_backend.rs:52` **struct** `WgpuBackend` — wgpu-бэкенд: тонкая обёртка над [`Renderer`], реализующая [`RenderBackend`]
`crates/engine/paint/src/backends/wgpu_backend.rs:67` **fn** `new` — Создаёт оконный бэкенд из winit-окна
`crates/engine/paint/src/backends/wgpu_backend.rs:82` **fn** `new_headless` — Создаёт headless-бэкенд для тестов и `--print-to-pdf`
`crates/engine/paint/src/backends/wgpu_backend.rs:100` **fn** `target_color_space` — Target color space selected for the output surface
`crates/engine/paint/src/backends/wgpu_backend.rs:110` **fn** `is_wide_gamut` — `true` если текущий вывод configured для wide-gamut (Display P3 или Rec.2020)
`crates/engine/paint/src/backends/wgpu_backend.rs:118` **fn** `renderer` — Неизменяемый доступ к внутреннему [`Renderer`]
`crates/engine/paint/src/backends/wgpu_backend.rs:123` **fn** `renderer_mut` — Изменяемый доступ к внутреннему [`Renderer`]
`crates/engine/paint/src/blend_modes.rs:24` **fn** `blend_channel` — Separable blend function `B(Cs, Cb)` per channel (CSS Compositing L1 §9)
`crates/engine/paint/src/blend_modes.rs:93` **fn** `blend_rgb` — Blend function `B(Cs, Cb)` for a full RGB triple (CSS Compositing L1 §9–10)
`crates/engine/paint/src/blend_modes.rs:120` **fn** `mix_blend_rgba` — CSS Compositing L1 §5 — blend `src` over `dst` with `mode`, then composite
`crates/engine/paint/src/blend_modes.rs:148` **fn** `lum` — Luminance of a straight RGB triple (Rec.601 weights, как в WGSL-шейдере)
`crates/engine/paint/src/blend_modes.rs:155` **fn** `clip_color` — `ClipColor` (CSS Compositing L1 §10): после SetLum компоненты могут выйти
`crates/engine/paint/src/blend_modes.rs:177` **fn** `set_lum` — `SetLum` (CSS Compositing L1 §10): сдвигает все каналы так, чтобы
`crates/engine/paint/src/blend_modes.rs:184` **fn** `sat` — Saturation of a straight RGB triple: `max − min` (CSS Compositing L1 §10)
`crates/engine/paint/src/blend_modes.rs:191` **fn** `set_sat` — `SetSat` (CSS Compositing L1 §10): задаёт saturation `s`, сохраняя порядок
`crates/engine/paint/src/color_management.rs:8` **fn** `detect_color_space_from_icc` — Legacy wrapper for ICC profile detection (deprecated, use lumen_core::detect_color_space_from_icc)
`crates/engine/paint/src/color_management.rs:15` **fn** `apply_tone_mapping` — Apply tone mapping for a detected color space (Phase 1 placeholder)
`crates/engine/paint/src/compositor.rs:63` **trait** `Layer` — Один layer: bbox + связь со stacking context-ом + локальный display list
`crates/engine/paint/src/compositor.rs:71` **trait** `LayerTree` — Коллекция layer-ов. Trait-обстракция, чтобы compositor мог принимать
`crates/engine/paint/src/compositor.rs:79` **struct** `BasicLayer` — Sprint 0 / Phase 0 concrete impl. Owned struct без интерлевания —
`crates/engine/paint/src/compositor.rs:100` **struct** `BasicLayerTree` — Sprint 0 / Phase 0 concrete impl. Один display-list = один layer
`crates/engine/paint/src/compositor.rs:108` **fn** `empty` — Пустой tree (нет ни одного layer-а). Полезен как начальное состояние
`crates/engine/paint/src/compositor.rs:117` **fn** `single_layer` — Phase 0: оборачивает весь display-list в один layer на bbox-страницы
`crates/engine/paint/src/compositor.rs:154` **trait** `Compositor` — Compositor: получает обновления сцены через `commit`, отдаёт активную
`crates/engine/paint/src/compositor.rs:187` **struct** `InProcessCompositor` — Single-thread in-process compositor: синхронный swap, без Mutex
`crates/engine/paint/src/compositor.rs:196` **fn** `new`
`crates/engine/paint/src/compositor.rs:331` **struct** `ThreadedCompositor` — Thread-safe compositor: тот же API two-buffer-а, но `commit` и
`crates/engine/paint/src/compositor.rs:338` **fn** `new`
`crates/engine/paint/src/compositor.rs:349` **fn** `handle` — Cheap-clone handle для другого потока: shared доступ к тому же
`crates/engine/paint/src/compositor.rs:434` **struct** `ThreadedCompositorHandle` — Cheap-clone handle на тот же state, что и parent [`ThreadedCompositor`]
`crates/engine/paint/src/compositor.rs:440` **fn** `commit`
`crates/engine/paint/src/compositor.rs:456` **fn** `flush_pending`
`crates/engine/paint/src/compositor.rs:474` **fn** `has_pending`
`crates/engine/paint/src/compositor.rs:483` **fn** `active_tree`
`crates/engine/paint/src/compositor.rs:492` **fn** `active_trees`
`crates/engine/paint/src/compositor.rs:526` **struct** `CompositorThread` — Реальный compositor thread: отдельный OS-поток с vsync tick-loop
`crates/engine/paint/src/compositor.rs:535` **fn** `spawn` — Запускает compositor thread. `handle` — разделяемый доступ к state
`crates/engine/paint/src/compositor.rs:560` **fn** `shutdown` — Запрашивает завершение потока и блокируется до его выхода
`crates/engine/paint/src/dash_math.rs:24` **fn** `dashed_border_offsets` — Returns `(offset, length)` pairs along a border side of length `total` for a
`crates/engine/paint/src/dash_math.rs:53` **fn** `dotted_border_offsets` — Returns `(offset, length)` pairs along a border side of length `total` for a
`crates/engine/paint/src/dash_math.rs:88` **fn** `dash_segments` — Разбивает полосу длиной `total_length` на серию dash-сегментов
`crates/engine/paint/src/display_list.rs:41` **enum** `FilterMode` — CSS Images L3 §4.3 — image-rendering filter mode (scaling algorithm)
`crates/engine/paint/src/display_list.rs:54` **fn** `from_image_rendering` — Преобразует `ImageRendering` в `FilterMode`
`crates/engine/paint/src/display_list.rs:70` **enum** `BlendMode` — CSS Compositing & Blending L1 §5 — blend mode. Phase 0 содержит только
`crates/engine/paint/src/display_list.rs:98` **fn** `from_keyword` — Парсит CSS-keyword `mix-blend-mode` / `background-blend-mode` (CSS
`crates/engine/paint/src/display_list.rs:135` **enum** `MaskMode` — CSS Masking L1 §6 — how to derive the mask value from rendered mask-layer pixels
`crates/engine/paint/src/display_list.rs:149` **struct** `CornerRadii` — Corner radii for CSS `border-radius`. Values are in CSS pixels, clamped to ≥ 0
`crates/engine/paint/src/display_list.rs:171` **fn** `all_zero` — Returns `true` if all eight radii are zero (no rounding needed)
`crates/engine/paint/src/display_list.rs:187` **fn** `from_style_and_box` — Builds `CornerRadii` from a `ComputedStyle` and the element's border-box dimensions
`crates/engine/paint/src/display_list.rs:203` **fn** `from_style` — Builds `CornerRadii` from a `ComputedStyle`. `border-radius: N%` values are
`crates/engine/paint/src/display_list.rs:218` **fn** `clamped_to_box` — Clamps every radius via the CSS Backgrounds L3 §5.5 corner-overlap rule
`crates/engine/paint/src/display_list.rs:245` **fn** `inner_for_border` — Computes the inner-edge corner radii for a border of per-side widths
`crates/engine/paint/src/display_list.rs:265` **enum** `ResolvedClipShape` — BUG-140: `clip-path` basic-shape, разрешённая эмиттером в page-координаты
`crates/engine/paint/src/display_list.rs:301` **fn** `bounding_rect` — Axis-aligned bounding box формы (page px, до transform). Используется
`crates/engine/paint/src/display_list.rs:330` **enum** `DisplayCommand`
`crates/engine/paint/src/display_list.rs:906` **type** `DisplayList`
`crates/engine/paint/src/display_list.rs:935` **fn** `fit_image_rect` — CSS Images L3 §5.5 — `object-fit` placement: где располагается
`crates/engine/paint/src/display_list.rs:1077` **fn** `fit_image_quad` — Финальный GPU-quad для `<img>`: пересечение «полного» placement-rect
`crates/engine/paint/src/display_list.rs:1150` **fn** `cull_display_list` — Returns `true` if the display list contains any `backdrop-filter` element
`crates/engine/paint/src/display_list.rs:1181` **fn** `contains_backdrop_filter` — Cheap pre-check the renderer uses to decide whether computing a frame
`crates/engine/paint/src/display_list.rs:1217` **fn** `hash_display_list` — Computes a content hash over a frame's display list plus the viewport state
`crates/engine/paint/src/display_list.rs:1245` **struct** `DiffResult` — Результат сравнения двух display-list-ов
`crates/engine/paint/src/display_list.rs:1257` **fn** `identical` — Создаёт DiffResult для идентичных display list-ов
`crates/engine/paint/src/display_list.rs:1271` **fn** `changed` — Создаёт DiffResult для изменённых display list-ов с заданным bounding rect
`crates/engine/paint/src/display_list.rs:1287` **fn** `diff_display_lists` — Сравнивает два display list-а по Debug hash каждой команды
`crates/engine/paint/src/display_list.rs:1405` **fn** `serialize_display_list`
`crates/engine/paint/src/display_list.rs:1845` **fn** `build_display_list`
`crates/engine/paint/src/display_list.rs:1861` **fn** `build_display_list_with_selection` — Like [`build_display_list`] but applies `::selection` CSS highlight styles
`crates/engine/paint/src/display_list.rs:1879` **fn** `build_display_list_with_anim` — Like `build_display_list` but applies compositor animation overrides per node
`crates/engine/paint/src/display_list.rs:1915` **fn** `build_display_list_ordered` — Билдер display list-а, **уважающий painting order** (CSS 2.1 Appendix E)
`crates/engine/paint/src/display_list.rs:1926` **fn** `build_display_list_ordered_dpr` — Like [`build_display_list_ordered`] but resolves `image-set()` background
`crates/engine/paint/src/display_list.rs:1975` **fn** `build_display_list_ordered_with_anim` — Like [`build_display_list_ordered`] but applies compositor animation overrides per node
`crates/engine/paint/src/display_list.rs:1986` **fn** `build_display_list_ordered_with_anim_dpr` — Like [`build_display_list_ordered_with_anim`] but resolves `image-set()`
`crates/engine/paint/src/display_list.rs:2040` **fn** `build_print_display_list` — Builds a print display list from paginated layout
`crates/engine/paint/src/display_list.rs:2105` **fn** `split_at_page_breaks` — Splits a print display list at `PageBreak` markers
`crates/engine/paint/src/display_list.rs:2132` **fn** `strip_background_graphics` — Removes background-graphics paint commands from each print page when the
`crates/engine/paint/src/display_list.rs:3249` **fn** `is_image_set` — CSS Images L4 §5 — is `value` an `image-set()` / `-webkit-image-set()` expression?
`crates/engine/paint/src/display_list.rs:3378` **fn** `select_image_set_url` — CSS Images L4 §5 — selects the best `image-set()` candidate URL for `dpr`
`crates/engine/paint/src/display_list.rs:4238` **fn** `point_on_resize_grip` — Возвращает `true`, если точка (`px`, `py`) попадает в resize-grip элемента
`crates/engine/paint/src/display_list.rs:14884` **fn** `emit_text_with_highlights` — CSS Custom Highlight API L1 — helper to emit DrawText with highlight name
`crates/engine/paint/src/display_list_cache.rs:21` **struct** `CachedDisplayLayer` — Cached display list for a stacking context or page subtree
`crates/engine/paint/src/display_list_cache.rs:45` **struct** `DisplayListCache` — LRU cache that maps `NodeId` (u32) to a pre-built `Vec<DisplayCommand>`
`crates/engine/paint/src/display_list_cache.rs:59` **fn** `new` — Create a cache with the default 32 MB budget
`crates/engine/paint/src/display_list_cache.rs:69` **fn** `with_budget` — Create with a custom byte budget
`crates/engine/paint/src/display_list_cache.rs:78` **fn** `get` — Look up the cached layer for `node_id`
`crates/engine/paint/src/display_list_cache.rs:96` **fn** `insert` — Insert or replace the cached display list for `node_id`
`crates/engine/paint/src/display_list_cache.rs:126` **fn** `remove` — Remove the cached layer for `node_id` and free its memory
`crates/engine/paint/src/display_list_cache.rs:133` **fn** `would_exceed_budget` — Returns `true` if adding `extra_bytes` would exceed the budget
`crates/engine/paint/src/display_list_cache.rs:140` **fn** `evict_lru` — Evict LRU entries until at least `target_bytes` have been freed
`crates/engine/paint/src/display_list_cache.rs:163` **fn** `clear` — Clear all cached entries and reset memory tracking
`crates/engine/paint/src/display_list_cache.rs:169` **fn** `len` — Number of cached entries
`crates/engine/paint/src/display_list_cache.rs:174` **fn** `is_empty` — `true` if the cache is empty
`crates/engine/paint/src/display_list_cache.rs:179` **fn** `used_bytes` — Current byte usage across all entries
`crates/engine/paint/src/display_list_cache.rs:184` **fn** `budget_bytes` — Configured budget in bytes
`crates/engine/paint/src/display_list_cache.rs:193` **fn** `on_memory_pressure` — React to an OS memory-pressure event
`crates/engine/paint/src/display_list_cache.rs:244` **fn** `hash_commands` — Compute a 64-bit content hash for a display-list command slice
`crates/engine/paint/src/fingerprint.rs:21` **struct** `GpuFingerprint` — GPU fingerprint info: normailzed vendor and renderer strings
`crates/engine/paint/src/fingerprint.rs:36` **fn** `from_adapter_info` — Create normalized GPU fingerprint from wgpu adapter info
`crates/engine/paint/src/fingerprint.rs:44` **fn** `vendor` — Vendor string: always "WebKit"
`crates/engine/paint/src/fingerprint.rs:49` **fn** `renderer` — Renderer string: always "Generic GPU"
`crates/engine/paint/src/gap_decorations.rs:18` **struct** `GapDecorationContext` — Parameters for gap rule rendering
`crates/engine/paint/src/gap_decorations.rs:31` **struct** `GapSegment` — One inter-cell gap in a flex, grid, or multicol layout
`crates/engine/paint/src/gap_decorations.rs:58` **fn** `emit_gap_rules` — Emits [`DisplayCommand::DrawBorder`] entries for gap decorations between
`crates/engine/paint/src/glsl.rs:32` **enum** `Val` — Runtime value inside the GLSL interpreter
`crates/engine/paint/src/glsl.rs:49` **fn** `to_float` — Convert any numeric-ish value to a scalar f32
`crates/engine/paint/src/glsl.rs:63` **fn** `to_vec4` — Convert any value to vec4 (broadcasting rules)
`crates/engine/paint/src/glsl.rs:75` **fn** `components` — Number of scalar components
`crates/engine/paint/src/glsl.rs:86` **fn** `get_component` — Read a single float component by index (0-based)
`crates/engine/paint/src/glsl.rs:323` **enum** `GlType` — GLSL type tag (declaration-time)
`crates/engine/paint/src/glsl.rs:394` **struct** `ParsedShader` — A parsed GLSL shader: declaration tables + the `main()` function body
`crates/engine/paint/src/glsl.rs:911` **fn** `parse` — Parse a GLSL ES shader source string
`crates/engine/paint/src/glsl.rs:920` **struct** `ShaderEnv` — Execution environment for a single shader invocation
`crates/engine/paint/src/glsl.rs:938` **fn** `new`
`crates/engine/paint/src/glsl.rs:977` **fn** `exec_main` — Execute the `main()` function of a parsed shader
`crates/engine/paint/src/glsl.rs:1546` **fn** `interp_varyings` — Linearly interpolate a map of varying values given barycentric weights
`crates/engine/paint/src/gradient_math.rs:25` **fn** `resolve_stop_positions` — CSS Images L3 §3.3 — resolve `GradientStop` positions to normalized [0,1]
`crates/engine/paint/src/gradient_math.rs:103` **fn** `premultiplied_subdivide_stops` — CSS Images L4 §3.1 — gradient colour interpolation is defined in
`crates/engine/paint/src/gradient_math.rs:133` **fn** `lerp_color_premul` — Premultiplied linear interpolation between two straight RGBA8 colours
`crates/engine/paint/src/gradient_math.rs:153` **fn** `sample_gradient_color` — Sample a resolved gradient stop list at position `t` (straight-colour linear
`crates/engine/paint/src/gradient_math.rs:183` **fn** `lerp_color` — Linear interpolation between two straight (non-premultiplied) RGBA8 colours
`crates/engine/paint/src/gradient_math.rs:196` **fn** `conic_sample_t` — CSS Images L4 §3.7 — отображает долю оборота `t` ∈ [0,1) в позицию сэмпла
`crates/engine/paint/src/gradient_math.rs:212` **fn** `atan2_det` — Deterministic `atan2(y, x)` returning radians in `(-π, π]`
`crates/engine/paint/src/hit_test.rs:48` **struct** `HitTestResult` — Результат hit-теста
`crates/engine/paint/src/hit_test.rs:77` **fn** `hit_test` — Hit-тест точки в viewport-координатах. `root` — layout-дерево из
`crates/engine/paint/src/layer_cache.rs:21` **struct** `LayerKey` — Layer identification key for cache lookup
`crates/engine/paint/src/layer_cache.rs:31` **fn** `new` — Create a new layer cache key
`crates/engine/paint/src/layer_cache.rs:38` **struct** `LayerEntry` — Metadata for a cached GPU layer texture
`crates/engine/paint/src/layer_cache.rs:54` **struct** `LayerCache` — Layer cache managing GPU memory via LRU eviction
`crates/engine/paint/src/layer_cache.rs:72` **fn** `new` — Create a new layer cache with default 256 MB GPU memory budget
`crates/engine/paint/src/layer_cache.rs:83` **fn** `with_budget` — Create with custom GPU memory budget (in bytes)
`crates/engine/paint/src/layer_cache.rs:94` **fn** `used_bytes` — Get the current GPU memory usage
`crates/engine/paint/src/layer_cache.rs:99` **fn** `budget_bytes` — Get the GPU memory budget
`crates/engine/paint/src/layer_cache.rs:104` **fn** `would_exceed_budget` — Check if adding a layer of given size would exceed budget
`crates/engine/paint/src/layer_cache.rs:111` **fn** `insert` — Insert or update a cached layer
`crates/engine/paint/src/layer_cache.rs:134` **fn** `access` — Mark a cached layer as accessed (used by current render)
`crates/engine/paint/src/layer_cache.rs:144` **fn** `get_lru_candidates` — Get candidates for LRU eviction, sorted from least- to most-recently-used
`crates/engine/paint/src/layer_cache.rs:153` **fn** `remove_keys` — Remove cached layers by key, freeing GPU memory
`crates/engine/paint/src/layer_cache.rs:169` **fn** `clear` — Clear all cached entries (full eviction), including promoted layer registrations
`crates/engine/paint/src/layer_cache.rs:176` **fn** `len` — Get the number of cached layers
`crates/engine/paint/src/layer_cache.rs:181` **fn** `is_empty` — Check if cache is empty
`crates/engine/paint/src/layer_cache.rs:186` **fn** `contains` — Check if a specific layer is in cache
`crates/engine/paint/src/layer_cache.rs:196` **fn** `promote_layer` — Promote a node to its own GPU layer (for `will-change: transform/opacity/filter`)
`crates/engine/paint/src/layer_cache.rs:204` **fn** `is_layer_promoted` — Returns `true` if the given node has a promoted GPU layer
`crates/engine/paint/src/layer_cache.rs:209` **fn** `demote_layer` — Remove the promoted GPU layer for a node, freeing its cache entry
`crates/engine/paint/src/layer_cache.rs:218` **fn** `sync_promoted_layers` — Remove promoted layers for nodes NOT in `current_nodes`
`crates/engine/paint/src/layer_cache.rs:231` **fn** `promoted_count` — Number of nodes currently promoted to their own GPU layer
`crates/engine/paint/src/layer_cache.rs:240` **fn** `on_memory_pressure` — React to an OS memory pressure event by evicting GPU layer textures
`crates/engine/paint/src/lib.rs:101` **struct** `FontMeasurer` — Реализация [`TextMeasurer`] на основе TTF-данных шрифта
`crates/engine/paint/src/lib.rs:111` **fn** `new`
`crates/engine/paint/src/lib.rs:310` **struct** `MultiFontMeasurer` — Многошрифтовый измеритель: поддерживает @font-face-загруженные шрифты
`crates/engine/paint/src/lib.rs:320` **fn** `new` — Создаёт измеритель с bundled-шрифтом как fallback
`crates/engine/paint/src/lib.rs:334` **fn** `register_family` — Регистрирует @font-face шрифт под именем `family` без unicode-range ограничений
`crates/engine/paint/src/lib.rs:348` **fn** `register_family_with_ranges` — Регистрирует @font-face шрифт с `unicode-range` ограничением
`crates/engine/paint/src/lib.rs:364` **fn** `family_count` — Количество зарегистрированных семей (для тестов)
`crates/engine/paint/src/lib.rs:379` **fn** `resolve_font_stretch` — Resolves `font-stretch` percentage for the first matching family
`crates/engine/paint/src/matrix_util.rs:19` **fn** `mat4_to_2d_affine` — Извлекает 2D-аффинные компоненты `[a, b, c, d, e, f]` из column-major
`crates/engine/paint/src/renderer.rs:1272` **struct** `OffscreenLayer` — GPU-ресурсы одного off-screen opacity layer-а. Создаётся лениво через
`crates/engine/paint/src/renderer.rs:1303` **enum** `SnapshotUploadError` — Ошибка `Renderer::upload_layer_snapshot`
`crates/engine/paint/src/renderer.rs:1332` **enum** `ImageRegisterError` — Ошибка `Renderer::register_image`
`crates/engine/paint/src/renderer.rs:1396` **struct** `Renderer`
`crates/engine/paint/src/renderer.rs:1605` **fn** `new`
`crates/engine/paint/src/renderer.rs:1695` **fn** `new_headless` — Creates a headless `Renderer` for off-screen rendering without a winit window
`crates/engine/paint/src/renderer.rs:3130` **fn** `with_font_provider` — Заменяет источник лукапа face-ов. Полезно для тестов (mock-provider) и
`crates/engine/paint/src/renderer.rs:3138` **fn** `set_font_provider` — Заменяет `FontProvider` на работающем рендере. Используется shell-ом,
`crates/engine/paint/src/renderer.rs:3151` **fn** `preload_fallback_chain` — Эагерно загружает указанные family-имена через текущий `FontProvider`,
`crates/engine/paint/src/renderer.rs:3165` **fn** `gpu_fingerprint` — Returns the normalized GPU fingerprint (vendor/renderer strings)
`crates/engine/paint/src/renderer.rs:3178` **fn** `preload_curated_fallbacks` — Shortcut: эагерно загружает `CURATED_FALLBACK_FAMILIES` (Noto Color
`crates/engine/paint/src/renderer.rs:3259` **fn** `register_image` — Регистрирует декодированное изображение в GPU-cache под ключом `src`
`crates/engine/paint/src/renderer.rs:3405` **fn** `unregister_image` — Снимает регистрацию изображения. После этого `DrawImage` для `src`
`crates/engine/paint/src/renderer.rs:3414` **fn** `clear_images` — Снимает регистрацию всех картинок (например, при переходе на новую
`crates/engine/paint/src/renderer.rs:3421` **fn** `has_image` — Зарегистрирована ли картинка с таким `src` (для shell-логирования)
`crates/engine/paint/src/renderer.rs:3439` **fn** `upload_layer_snapshot` — Загружает CPU-пиксели (`Rgba8`, 4 байта/пиксель) как именованный
`crates/engine/paint/src/renderer.rs:3506` **fn** `evict_layer_snapshot` — Удаляет снимок с `id`. GPU-память освобождается при drop-е
`crates/engine/paint/src/renderer.rs:3511` **fn** `clear_layer_snapshots` — Удаляет все снимки (например, при переходе на новую страницу)
`crates/engine/paint/src/renderer.rs:3517` **fn** `has_layer_snapshot` — Зарегистрирован ли снимок с таким `id`
`crates/engine/paint/src/renderer.rs:3522` **fn** `layer_cache` — Получить ссылку на layer cache для статистики / монитора GPU памяти
`crates/engine/paint/src/renderer.rs:3530` **fn** `set_backdrop_cache_enabled` — Enables or disables the `backdrop-filter` result cache (CSS Filter
`crates/engine/paint/src/renderer.rs:3539` **fn** `clear_backdrop_cache` — Drops every cached `backdrop-filter` texture and its metadata. The next
`crates/engine/paint/src/renderer.rs:3546` **fn** `backdrop_cache_len` — Number of live cached `backdrop-filter` textures (for stats / tests)
`crates/engine/paint/src/renderer.rs:3553` **fn** `backdrop_cache_on_memory_pressure` — Forwards a memory-pressure signal to the `backdrop-filter` cache and
`crates/engine/paint/src/renderer.rs:3565` **fn** `atlas_on_memory_pressure` — Forwards a memory-pressure signal to the glyph atlas so it can evict
`crates/engine/paint/src/renderer.rs:3570` **fn** `layer_cache_mut` — Получить мutable ссылку для прямого управления кэшем (advanced usage)
`crates/engine/paint/src/renderer.rs:3576` **fn** `access_layer` — Отметить layer как используемый текущим render pass
`crates/engine/paint/src/renderer.rs:3583` **fn** `cache_layer` — Кэшировать layer слой. Returns `true` if this is a new layer, `false` if updated
`crates/engine/paint/src/renderer.rs:3589` **fn** `return_layer_to_pool` — Return an off-screen layer texture to the pool for recycling (Phase 2 ADR-008)
`crates/engine/paint/src/renderer.rs:3605` **fn** `promote_layer` — Promote a node to its own GPU layer for `will-change: transform/opacity/filter`
`crates/engine/paint/src/renderer.rs:3615` **fn** `is_layer_promoted` — Returns `true` if the given node has a promoted GPU layer
`crates/engine/paint/src/renderer.rs:3620` **fn** `demote_layer` — Remove the promoted GPU layer for a node, freeing its cache entry
`crates/engine/paint/src/renderer.rs:3625` **fn** `clear_layer_cache` — Очистить весь layer cache (полная эвикция) и очистить texture pool
`crates/engine/paint/src/renderer.rs:3631` **fn** `texture_pool_len` — Get the number of free textures in the pool (for diagnostics)
`crates/engine/paint/src/renderer.rs:3636` **fn** `texture_pool_len_for_size` — Get the number of free textures of a specific size (for diagnostics)
`crates/engine/paint/src/renderer.rs:3641` **fn** `clear_texture_pool` — Clear all pooled textures (e.g., when resizing or memory pressure is high)
`crates/engine/paint/src/renderer.rs:3647` **fn** `snapshot_dimensions` — Возвращает `(width, height)` снимка, или `None` если `id` не зарегистрирован
`crates/engine/paint/src/renderer.rs:3653` **fn** `resize` — Resizes the render target. For windowed mode, reconfigures the wgpu surface
`crates/engine/paint/src/renderer.rs:3682` **fn** `set_scale_factor` — Обновить device-pixel-ratio. Вызывается shell-ом по `WindowEvent::ScaleFactorChanged`
`crates/engine/paint/src/renderer.rs:3691` **fn** `scale_factor` — Текущий device-pixel-ratio. Для отладки / тестов (UI обычно его не читает —
`crates/engine/paint/src/renderer.rs:3701` **fn** `target_color_space` — Target color space for this renderer's output surface
`crates/engine/paint/src/renderer.rs:3710` **fn** `set_canvas_background` — Updates the root-element canvas background used as the framebuffer clear colour
`crates/engine/paint/src/renderer.rs:3753` **fn** `viewport_size` — Текущий viewport в **logical** (CSS) пикселях: `physical / scale_factor`
`crates/engine/paint/src/renderer.rs:3938` **fn** `render` — Рендерит две полосы display list-а одним кадром:
`crates/engine/paint/src/renderer.rs:6772` **fn** `render_to_image_cpu` — CPU-based rasterization using tiny-skia (feature="cpu-render" only)
`crates/engine/paint/src/renderer.rs:6798` **fn** `render_tile`
`crates/engine/paint/src/renderer.rs:6837` **fn** `render_to_image` — Renders display commands and returns a CPU `Image` (RGBA8)
`crates/engine/paint/src/renderer.rs:6940` **fn** `render_print_pages` — Renders a print display list into one `Image` per page
`crates/engine/paint/src/scroll_snap.rs:33` **fn** `find_scroll_snap_y` — CSS Scroll Snap L1 — returns the Y scroll offset to snap to, or `None`
`crates/engine/paint/src/scroll_snap.rs:54` **fn** `find_scroll_snap_y_proximity` — CSS Scroll Snap L1 — same as [`find_scroll_snap_y`] but restricts candidates
`crates/engine/paint/src/svg_path.rs:16` **enum** `PathSegment` — One SVG path command (absolute coords, after normalization)
`crates/engine/paint/src/svg_path.rs:36` **fn** `parse_svg_path` — Parses SVG path `d` attribute into absolute-coordinate segments
`crates/engine/paint/src/svg_path.rs:308` **fn** `flatten_path` — Flatten path segments to a list of closed contours
`crates/engine/paint/src/svg_path.rs:552` **fn** `tessellate_polygon` — Tessellate a single closed polygon (no holes) using ear-clipping
`crates/engine/paint/src/svg_path.rs:586` **fn** `tessellate_fill` — Tessellate a path (all contours) into triangles. Multi-contour paths are
`crates/engine/paint/src/svg_path.rs:608` **fn** `tessellate_fill_even_odd` — Tessellate the **even-odd** fill region of all contours into a flat triangle
`crates/engine/paint/src/svg_path.rs:815` **fn** `tessellate_stroke` — Tessellate stroke outlines for all contours into a flat triangle vertex list
`crates/engine/paint/src/svg_path.rs:919` **enum** `StrokeLinecap` — Stroke caps applied at open sub-path endpoints
`crates/engine/paint/src/svg_path.rs:931` **enum** `StrokeLinejoin` — Join style at connected segment vertices
`crates/engine/paint/src/svg_path.rs:943` **struct** `StrokeParams` — Parameters for advanced stroke tessellation
`crates/engine/paint/src/svg_path.rs:976` **fn** `apply_dash_pattern` — Apply a dash pattern to a list of contours
`crates/engine/paint/src/svg_path.rs:1075` **fn** `tessellate_stroke_ex` — Tessellate strokes with full linecap / linejoin / miterlimit / dasharray support
`crates/engine/paint/src/texture_pool.rs:15` **struct** `TextureKey` — Key for a pool entry: texture dimensions
`crates/engine/paint/src/texture_pool.rs:24` **fn** `new` — Create a new texture pool key
`crates/engine/paint/src/texture_pool.rs:34` **struct** `PooledTexture` — A pooled GPU texture resource
`crates/engine/paint/src/texture_pool.rs:53` **struct** `TexturePool` — Texture pool managing free textures for recycling
`crates/engine/paint/src/texture_pool.rs:63` **fn** `new` — Create a new empty texture pool
`crates/engine/paint/src/texture_pool.rs:73` **fn** `acquire` — Try to allocate a texture of the given size from the pool
`crates/engine/paint/src/texture_pool.rs:82` **fn** `release` — Return a texture to the pool for reuse
`crates/engine/paint/src/texture_pool.rs:88` **fn** `clear` — Clear all pooled textures, freeing GPU memory
`crates/engine/paint/src/texture_pool.rs:94` **fn** `len` — Get the number of free textures in the pool (across all sizes)
`crates/engine/paint/src/texture_pool.rs:99` **fn** `is_empty` — Check if the pool is empty
`crates/engine/paint/src/texture_pool.rs:104` **fn** `len_for_size` — Get the number of free textures of a specific size
`crates/engine/paint/src/texture_pool.rs:110` **fn** `pool_size` — Get total tracked pool size (for diagnostics)
`crates/engine/paint/src/texture_pool.rs:115` **fn** `update_size` — Update internal pool size counter (call after creating or destroying a texture)
`crates/engine/paint/src/tile_grid.rs:19` **enum** `TileDirty` — Dirty state of a single tile
`crates/engine/paint/src/tile_grid.rs:31` **struct** `TileGrid` — Tile-grid for dirty-rect tracking
`crates/engine/paint/src/tile_grid.rs:40` **fn** `new` — Create a new grid with all tiles missing (implicitly dirty)
`crates/engine/paint/src/tile_grid.rs:48` **fn** `default_size` — Create a new grid with the default 256 px tile size
`crates/engine/paint/src/tile_grid.rs:53` **fn** `mark_dirty` — Mark a single tile dirty
`crates/engine/paint/src/tile_grid.rs:58` **fn** `mark_clean` — Mark a single tile clean
`crates/engine/paint/src/tile_grid.rs:63` **fn** `is_dirty` — Return `true` if the tile is dirty or has never been rendered
`crates/engine/paint/src/tile_grid.rs:71` **fn** `mark_all_dirty` — Mark all tiles covered by the given page dimensions dirty
`crates/engine/paint/src/tile_grid.rs:84` **fn** `dirty_tiles` — Return all tiles currently marked dirty
`crates/engine/paint/src/tile_grid.rs:107` **fn** `update_from_diff` — Diff `old_dl` against `new_dl` and mark tiles that contain changed
`crates/engine/paint/src/varied_text.rs:27` **enum** `PathCmd` — One path-building command in screen pixels (origin top-left, Y down)
`crates/engine/paint/src/varied_text.rs:115` **fn** `build_varied_text_paths` — Builds filled-glyph path commands for a text run rendered with
`crates/engine/paint/src/webgl.rs:114` **struct** `SoftwareWebGl` — Pure-Rust software WebGL 1.0 context
`crates/engine/paint/src/webgl.rs:170` **fn** `new` — Create a context with a `width × height` drawing buffer
`crates/engine/paint/src/webgl.rs:197` **fn** `width` — Drawing-buffer width in pixels
`crates/engine/paint/src/webgl.rs:202` **fn** `height` — Drawing-buffer height in pixels
`crates/engine/paint/src/webgl.rs:207` **fn** `pixels` — Borrow the RGBA8 framebuffer (top-left origin, `width*height*4` bytes)
`crates/engine/paint/src/webgl.rs:213` **fn** `pixel` — Read the RGBA pixel at `(x, y)` (top-left origin). Returns
`crates/engine/paint/src/webgl.rs:227` **fn** `viewport` — `gl.viewport(x, y, w, h)`
`crates/engine/paint/src/webgl.rs:232` **fn** `clear_color` — `gl.clearColor(r, g, b, a)`. Components are clamped to `[0, 1]`
`crates/engine/paint/src/webgl.rs:238` **fn** `clear` — `gl.clear(mask)`. Only `COLOR_BUFFER_BIT` has a visible effect; the
`crates/engine/paint/src/webgl.rs:255` **fn** `create_buffer` — `gl.createBuffer()` → opaque buffer id (never 0)
`crates/engine/paint/src/webgl.rs:265` **fn** `bind_buffer` — `gl.bindBuffer(target, buffer)`. `buffer == 0` unbinds. Only
`crates/engine/paint/src/webgl.rs:273` **fn** `buffer_data_f32` — `gl.bufferData(target, data, usage)` for float data. Stores `data`
`crates/engine/paint/src/webgl.rs:280` **fn** `create_shader` — `gl.createShader(kind)` → opaque shader id, or 0 for an unknown kind
`crates/engine/paint/src/webgl.rs:294` **fn** `shader_source` — `gl.shaderSource(shader, source)`
`crates/engine/paint/src/webgl.rs:303` **fn** `compile_shader` — `gl.compileShader(shader)`. Parses the GLSL source into an AST so
`crates/engine/paint/src/webgl.rs:312` **fn** `shader_compiled` — `gl.getShaderParameter(shader, COMPILE_STATUS)` — true once compiled
`crates/engine/paint/src/webgl.rs:317` **fn** `create_program` — `gl.createProgram()` → opaque program id (never 0)
`crates/engine/paint/src/webgl.rs:325` **fn** `attach_shader` — `gl.attachShader(program, shader)`. Slots the shader by its kind
`crates/engine/paint/src/webgl.rs:340` **fn** `link_program` — `gl.linkProgram(program)`. Always marks the program linked
`crates/engine/paint/src/webgl.rs:347` **fn** `program_linked` — `gl.getProgramParameter(program, LINK_STATUS)` — true once linked
`crates/engine/paint/src/webgl.rs:352` **fn** `use_program` — `gl.useProgram(program)`. `program == 0` clears the active program
`crates/engine/paint/src/webgl.rs:358` **fn** `get_attrib_location` — `gl.getAttribLocation(program, name)` → stable location (≥ 0), or -1 if
`crates/engine/paint/src/webgl.rs:375` **fn** `get_uniform_location` — `gl.getUniformLocation(program, name)` → stable location (≥ 0), or -1 if
`crates/engine/paint/src/webgl.rs:391` **fn** `enable_vertex_attrib_array` — `gl.enableVertexAttribArray(index)`
`crates/engine/paint/src/webgl.rs:396` **fn** `disable_vertex_attrib_array` — `gl.disableVertexAttribArray(index)`
`crates/engine/paint/src/webgl.rs:407` **fn** `vertex_attrib_pointer` — `gl.vertexAttribPointer(index, size, type, normalized, stride, offset)`
`crates/engine/paint/src/webgl.rs:422` **fn** `uniform4f` — `gl.uniform4f(location, x, y, z, w)`
`crates/engine/paint/src/webgl.rs:430` **fn** `uniform3f` — `gl.uniform3f(location, x, y, z)`
`crates/engine/paint/src/webgl.rs:437` **fn** `uniform2f` — `gl.uniform2f(location, x, y)`
`crates/engine/paint/src/webgl.rs:444` **fn** `uniform1f` — `gl.uniform1f(location, x)`
`crates/engine/paint/src/webgl.rs:451` **fn** `uniform1i` — `gl.uniform1i(location, v)`. Used to bind sampler2D to a texture unit
`crates/engine/paint/src/webgl.rs:459` **fn** `uniform_matrix4fv` — `gl.uniformMatrix4fv(location, transpose, values)`. Stores a 4×4 float
`crates/engine/paint/src/webgl.rs:468` **fn** `active_texture` — `gl.activeTexture(unit_enum)`. Sets the active texture unit
`crates/engine/paint/src/webgl.rs:473` **fn** `bind_texture` — `gl.bindTexture(target, texture_id)`. Records binding for the active unit
`crates/engine/paint/src/webgl.rs:479` **fn** `tex_image_2d_rgba` — `gl.texImage2D(…, data)`. Averages pixel data to a 1×1 solid colour for
`crates/engine/paint/src/webgl.rs:498` **fn** `draw_arrays` — `gl.drawArrays(mode, first, count)`. Executes vertex and fragment shaders
`crates/engine/paint/src/webgpu_compute.rs:67` **struct** `AdapterInfo` — Информация о GPU-адаптере для отдачи в JS (`GPUAdapter.info`)
`crates/engine/paint/src/webgpu_compute.rs:154` **fn** `is_available` — Доступен ли реальный GPU-бэкенд (есть адаптер и устройство)
`crates/engine/paint/src/webgpu_compute.rs:159` **fn** `adapter_info` — Информация о реальном GPU-адаптере или `None`, если GPU недоступен
`crates/engine/paint/src/webgpu_compute.rs:171` **fn** `validate_wgsl` — Валидирует исходник WGSL на настоящем GPU-устройстве (трансляция + типовая проверка)
`crates/engine/paint/src/webgpu_compute.rs:275` **fn** `buffer_create` — Создаёт настоящий `wgpu::Buffer` и регистрирует его
`crates/engine/paint/src/webgpu_compute.rs:294` **fn** `buffer_write` — Записывает байты в буфер по смещению через `queue.write_buffer`
`crates/engine/paint/src/webgpu_compute.rs:316` **fn** `buffer_read` — Читает байты из буфера (буфер должен иметь usage `MAP_READ`)
`crates/engine/paint/src/webgpu_compute.rs:337` **fn** `buffer_destroy` — Удаляет буфер из реестра (освобождает GPU-память при дропе)
`crates/engine/paint/src/webgpu_compute.rs:414` **fn** `shader_create` — Создаёт `wgpu::ShaderModule` из WGSL и регистрирует его
`crates/engine/paint/src/webgpu_compute.rs:432` **fn** `compute_pipeline_create` — Создаёт compute-пайплайн с авто-layout (`layout: 'auto'`) из ранее созданного шейдера
`crates/engine/paint/src/webgpu_compute.rs:464` **fn** `pipeline_bind_group_layout` — Возвращает хэндл bind-group-layout, выведенного пайплайном для группы `group`
`crates/engine/paint/src/webgpu_compute.rs:481` **struct** `BufferBindEntry` — Одна entry bind-group: буфер-ресурс, привязанный к WGSL binding-индексу
`crates/engine/paint/src/webgpu_compute.rs:497` **fn** `bind_group_create` — Создаёт bind-group, связывающий буферы по binding-индексам, по заданному layout
`crates/engine/paint/src/webgpu_compute.rs:531` **fn** `compute_pipeline_destroy` — Удаляет compute-пайплайн из реестра
`crates/engine/paint/src/webgpu_compute.rs:624` **struct** `VertexAttr` — Одна вершинная атрибута (`GPUVertexAttribute`): формат, смещение, `@location`
`crates/engine/paint/src/webgpu_compute.rs:635` **struct** `VertexBufferLayout` — Один вершинный буфер пайплайна (`GPUVertexBufferLayout`): шаг, режим, атрибуты
`crates/engine/paint/src/webgpu_compute.rs:649` **fn** `texture_create` — Создаёт offscreen-текстуру (render-таргет) и регистрирует её
`crates/engine/paint/src/webgpu_compute.rs:677` **fn** `texture_destroy` — Удаляет текстуру из реестра (освобождает GPU-память при дропе)
`crates/engine/paint/src/webgpu_compute.rs:693` **fn** `texture_read_rgba` — Читает отрисованную текстуру обратно в плотный RGBA8 для present в страничный `<canvas>`
`crates/engine/paint/src/webgpu_compute.rs:787` **fn** `render_pipeline_create` — Создаёт render-пайплайн с авто-layout (`layout: 'auto'`)
`crates/engine/paint/src/webgpu_compute.rs:876` **fn** `render_pipeline_bind_group_layout` — Возвращает хэндл bind-group-layout, выведенного render-пайплайном для группы `group`
`crates/engine/paint/src/webgpu_compute.rs:889` **fn** `render_pipeline_destroy` — Удаляет render-пайплайн из реестра
`crates/engine/paint/src/webgpu_compute.rs:897` **enum** `ComputeCmd` — Одна команда внутри записанного compute-pass
`crates/engine/paint/src/webgpu_compute.rs:920` **enum** `RenderCmd` — Одна команда внутри записанного render-pass
`crates/engine/paint/src/webgpu_compute.rs:980` **enum** `GpuOp` — Одна записанная операция command-encoder для исполнения на `queue.submit`
`crates/engine/paint/src/webgpu_compute.rs:1035` **fn** `submit` — Исполняет набор операций в одном `CommandEncoder` и сабмитит на очередь

## lumen-shell  (902 symbols)

`crates/shell/src/adblock.rs:44` **fn** `browser_data_dir` — Root of all browser user data (portable): `<exe_dir>/data`
`crates/shell/src/adblock.rs:52` **fn** `adblock_dir` — `<data>/adblock` — root of the ad-block subsystem's files
`crates/shell/src/adblock.rs:57` **fn** `lists_dir` — `<data>/adblock/lists` — downloaded list bodies
`crates/shell/src/adblock.rs:62` **fn** `db_path` — Path to the SQLite store (`adblock.db`)
`crates/shell/src/adblock.rs:67` **fn** `ensure_dirs` — Create `data/adblock/lists/` if missing (best-effort)
`crates/shell/src/adblock.rs:74` **fn** `default_subscriptions` — The lists seeded on first run: EasyList (ads) + EasyPrivacy (trackers)
`crates/shell/src/adblock.rs:171` **fn** `load_and_install` — Read the enabled subscriptions' cached bodies from disk, merge them into a
`crates/shell/src/adblock.rs:208` **fn** `refresh` — Conditionally refresh all enabled subscriptions over the network
`crates/shell/src/address_bar.rs:55` **enum** `OmniboxPrefix` — Префикс @-команды, распознанный в строке ввода
`crates/shell/src/address_bar.rs:78` **fn** `parse_omnibox_prefix` — Разбирает raw ввод → `(OmniboxPrefix, query_str)`
`crates/shell/src/address_bar.rs:97` **enum** `OmniboxSuggestion` — Одна строка autocomplete в dropdown omnibox
`crates/shell/src/address_bar.rs:163` **fn** `commit_value` — Строка, которая будет зафиксирована при выборе этой подсказки
`crates/shell/src/address_bar.rs:174` **fn** `label` — Основной текст строки dropdown
`crates/shell/src/address_bar.rs:194` **fn** `sub_label` — Дополнительный текст под основным label
`crates/shell/src/address_bar.rs:240` **struct** `AddressBarState` — Состояние адресной строки. Хранится в `Lumen` struct наряду с `FindState`
`crates/shell/src/address_bar.rs:255` **fn** `open` — Открыть бар, предзаполнив поле текущим URL страницы
`crates/shell/src/address_bar.rs:263` **fn** `close`
`crates/shell/src/address_bar.rs:271` **fn** `is_open`
`crates/shell/src/address_bar.rs:275` **fn** `input`
`crates/shell/src/address_bar.rs:280` **fn** `suggestions` — Текущий список подсказок (для рендера и клавиатурной навигации)
`crates/shell/src/address_bar.rs:285` **fn** `selected_idx` — Индекс выделенной подсказки. `None` — ни одна не выделена
`crates/shell/src/address_bar.rs:291` **fn** `set_suggestions` — Установить новый список подсказок и сбросить выделение
`crates/shell/src/address_bar.rs:297` **fn** `select_next` — Перейти к следующей (вниз) подсказке
`crates/shell/src/address_bar.rs:308` **fn** `select_prev` — Перейти к предыдущей (вверх) подсказке. `None` если уже на первой
`crates/shell/src/address_bar.rs:316` **fn** `append_str` — Добавить непечатаемые символы (printable chars из keyboard event)
`crates/shell/src/address_bar.rs:330` **fn** `backspace` — Backspace — удалить последний Unicode-символ
`crates/shell/src/address_bar.rs:340` **fn** `commit` — Зафиксировать текущий ввод или выделенную подсказку: закрыть бар и,
`crates/shell/src/address_bar.rs:357` **fn** `take_commit` — Вернуть зафиксированный URL/запрос (если есть) и сбросить его
`crates/shell/src/address_bar.rs:365` **struct** `BarOverlay` — Параметры для сборки overlay display list
`crates/shell/src/address_bar.rs:373` **fn** `build_bar_overlay` — Собирает display list адресной строки. Вызывается каждый кадр, пока
`crates/shell/src/animation_scheduler.rs:116` **struct** `AnimationScheduler` — Планировщик CSS-анимаций. Хранит timing-состояние между кадрами
`crates/shell/src/animation_scheduler.rs:121` **fn** `new`
`crates/shell/src/animation_scheduler.rs:133` **fn** `tick` — Тик планировщика: обходит layout-дерево, для каждой активной анимации
`crates/shell/src/animation_scheduler.rs:157` **fn** `clear` — Удалить все записи для элементов, которых больше нет в дереве
`crates/shell/src/backend_factory.rs:40` **fn** `create_backend` — Создаёт windowed рендер-бэкенд для окна `window`
`crates/shell/src/click_log.rs:27` **fn** `init` — Вызвать один раз при старте с результатом разбора флага --activity-log
`crates/shell/src/click_log.rs:43` **fn** `is_enabled`
`crates/shell/src/click_log.rs:97` **struct** `ClickInfo` — Клик мышью: window-координаты и что под курсором
`crates/shell/src/click_log.rs:107` **struct** `HitInfo`
`crates/shell/src/click_log.rs:114` **enum** `ClickOutcome`
`crates/shell/src/click_log.rs:123` **fn** `log_click`
`crates/shell/src/click_log.rs:152` **fn** `log_nav` — Навигация на новый URL запущена (navigate_to вызван)
`crates/shell/src/click_log.rs:158` **fn** `log_load_start` — Фоновый поток загрузки страницы стартовал
`crates/shell/src/click_log.rs:165` **fn** `log_load_ok` — Страница загружена и отрисована
`crates/shell/src/click_log.rs:173` **fn** `log_load_err` — Ошибка загрузки
`crates/shell/src/click_log.rs:181` **fn** `log_fragment` — Скроллинг к фрагменту (#id) без перезагрузки страницы
`crates/shell/src/click_log.rs:188` **fn** `log_js_nav` — Навигация из JS (location.href=, history.pushState, window.open …)
`crates/shell/src/click_log.rs:194` **fn** `log_page_ready` — Страница полностью применена (apply_loaded_page завершён)
`crates/shell/src/config.rs:48` **fn** `init_global` — Install the process-global fingerprint profile. Idempotent: the first call
`crates/shell/src/config.rs:54` **fn** `global` — Return the process-global fingerprint profile, or the default if unset
`crates/shell/src/config.rs:120` **fn** `init_adblock` — Initialise the ad-block subsystem and install the process-global filter
`crates/shell/src/config.rs:149` **struct** `FingerprintProfile` — User-configurable fingerprint identity (9F.1)
`crates/shell/src/config.rs:218` **fn** `effective_tls_profile` — Resolve the effective TLS profile: explicit override, else derived from
`crates/shell/src/config.rs:230` **fn** `navigator_profile` — Build the JS-side [`lumen_js::NavigatorProfile`] from this config
`crates/shell/src/config.rs:254` **fn** `install_navigator` — Install the navigator/screen/timezone values into the process-global JS
`crates/shell/src/config.rs:260` **fn** `apply_http` — Stamp the HTTP and TLS fingerprint onto an [`HttpClient`] builder
`crates/shell/src/config.rs:316` **fn** `effective_socks5_proxy` — Resolve the effective SOCKS5 proxy: explicit override first, then
`crates/shell/src/config.rs:339` **fn** `config_path` — Resolve the path to the portable `fingerprint.toml`
`crates/shell/src/config.rs:348` **fn** `load` — Load and parse the fingerprint profile from the default config path
`crates/shell/src/config.rs:360` **fn** `parse` — Parse a flat `key = value` TOML subset into a [`FingerprintProfile`]
`crates/shell/src/deterministic.rs:15` **struct** `DetConfig` — Parsed deterministic-mode configuration from CLI args
`crates/shell/src/deterministic.rs:27` **fn** `extract_deterministic` — Extract all deterministic-mode flags from CLI args
`crates/shell/src/devtools/console_panel.rs:49` **enum** `ConsoleLevel` — Severity level of a console message
`crates/shell/src/devtools/console_panel.rs:94` **struct** `ConsoleMessage` — A single captured console message
`crates/shell/src/devtools/console_panel.rs:107` **struct** `ConsolePanel` — DevTools JS console panel
`crates/shell/src/devtools/console_panel.rs:123` **fn** `new` — Create a new, empty, hidden console panel
`crates/shell/src/devtools/console_panel.rs:135` **fn** `push_batch` — Push a batch of `(level_u8, text)` entries drained from the JS runtime
`crates/shell/src/devtools/console_panel.rs:153` **fn** `clear` — Clear all stored messages and reset scroll
`crates/shell/src/devtools/console_panel.rs:159` **fn** `toggle` — Toggle panel visibility
`crates/shell/src/devtools/console_panel.rs:165` **fn** `len` — Number of stored messages
`crates/shell/src/devtools/console_panel.rs:171` **fn** `is_empty` — `true` when no messages are stored
`crates/shell/src/devtools/console_panel.rs:177` **fn** `scroll_up` — Scroll up by `n` lines (towards older messages)
`crates/shell/src/devtools/console_panel.rs:184` **fn** `scroll_down` — Scroll down by `n` lines (towards newer messages)
`crates/shell/src/devtools/console_panel.rs:196` **fn** `build_console_panel` — Build the viewport-locked console panel overlay
`crates/shell/src/devtools/inspector.rs:118` **enum** `InspectorTab` — Which tab of the DevTools inspector panel is currently active
`crates/shell/src/devtools/inspector.rs:133` **struct** `SelectedNode` — A node currently pinned by the inspector, with its computed-style snapshot
`crates/shell/src/devtools/inspector.rs:160` **struct** `DomInspectorPanel` — DevTools DOM inspector panel state
`crates/shell/src/devtools/inspector.rs:185` **fn** `new` — Create a hidden inspector with no hover or selection
`crates/shell/src/devtools/inspector.rs:191` **fn** `toggle` — Toggle inspector activity. Clears hover (but keeps the last selection)
`crates/shell/src/devtools/inspector.rs:200` **fn** `set_hovered` — Update the node under the cursor. Returns `true` when the value changed
`crates/shell/src/devtools/inspector.rs:213` **fn** `select` — Pin a node as the current selection
`crates/shell/src/devtools/inspector.rs:234` **fn** `switch_tab` — Switch the active tab to `tab`
`crates/shell/src/devtools/inspector.rs:241` **fn** `set_network_entries` — Replace the Network-tab snapshot with `entries` (oldest first). Clamps the
`crates/shell/src/devtools/inspector.rs:251` **fn** `is_panel_click` — Returns `true` if `x` is inside the right-docked panel, given window CSS width
`crates/shell/src/devtools/inspector.rs:257` **fn** `click_tab_at` — Handle a click that is inside the panel. Switches tab when the click lands
`crates/shell/src/devtools/inspector.rs:287` **fn** `scroll_up` — Scroll the active tab's list up
`crates/shell/src/devtools/inspector.rs:312` **fn** `scroll_down` — Scroll the active tab's list down, clamped so the last page stays visible
`crates/shell/src/devtools/inspector.rs:341` **fn** `find_box` — Find the [`LayoutBox`] for `node` in document order. Returns `None` when the
`crates/shell/src/devtools/inspector.rs:360` **fn** `box_model_rects` — Compute the four box-model rectangles for `lb` in document (page) coordinates
`crates/shell/src/devtools/inspector.rs:415` **fn** `build_box_overlay` — Build the box-model overlay for the hovered box, translated from page
`crates/shell/src/devtools/inspector.rs:448` **fn** `element_label` — Build a human-readable DOM label for `node`, e.g. `div#main.card`, `#text`,
`crates/shell/src/devtools/inspector.rs:480` **fn** `computed_style_map` — Extract a curated computed-style map from a [`LayoutBox`] as ordered
`crates/shell/src/devtools/inspector.rs:590` **fn** `build_inspector_panel` — Build the right-docked inspector side panel
`crates/shell/src/devtools/network_panel.rs:76` **struct** `NetworkEntry` — A single recorded HTTP request and its lifecycle state
`crates/shell/src/devtools/network_panel.rs:109` **struct** `NetworkLog` — Shared, append-only log of HTTP requests for the network panel
`crates/shell/src/devtools/network_panel.rs:116` **fn** `record_started` — Record a newly started request: appends a pending entry
`crates/shell/src/devtools/network_panel.rs:133` **fn** `record_completed` — Record a completed request: fills the most recent matching pending entry
`crates/shell/src/devtools/network_panel.rs:163` **fn** `record_js` — Record a fully-formed request logged by page JS via
`crates/shell/src/devtools/network_panel.rs:185` **fn** `record_blocked` — Record a request blocked by the content filter. `reason` is the matched
`crates/shell/src/devtools/network_panel.rs:205` **fn** `record_failed` — Record a network-level failure for a previously started request
`crates/shell/src/devtools/network_panel.rs:232` **fn** `clear` — Clear all recorded requests (call on every top-level navigation)
`crates/shell/src/devtools/network_panel.rs:238` **fn** `len` — Number of recorded requests
`crates/shell/src/devtools/network_panel.rs:244` **fn** `is_empty` — `true` when no requests have been recorded
`crates/shell/src/devtools/network_panel.rs:265` **struct** `NetworkLogSink` — [`EventSink`] wrapper that forwards every event to an inner sink AND records
`crates/shell/src/devtools/network_panel.rs:302` **struct** `NetworkPanel` — DevTools network log panel (§7E.4)
`crates/shell/src/devtools/network_panel.rs:317` **fn** `new` — Create a new hidden panel backed by the given shared `log`
`crates/shell/src/devtools/network_panel.rs:327` **fn** `toggle` — Toggle panel visibility
`crates/shell/src/devtools/network_panel.rs:333` **fn** `refresh` — Pull the latest entries from the shared [`NetworkLog`] into the panel
`crates/shell/src/devtools/network_panel.rs:340` **fn** `clear_log` — Clear the shared log (call on every top-level navigation)
`crates/shell/src/devtools/network_panel.rs:353` **fn** `entries_clone` — Pull a fresh clone of the shared log's entries
`crates/shell/src/devtools/network_panel.rs:362` **fn** `record_js_request` — Append a JS-logged request to the shared log (drained from
`crates/shell/src/devtools/network_panel.rs:376` **fn** `len` — Number of entries in the current snapshot
`crates/shell/src/devtools/network_panel.rs:382` **fn** `is_empty` — `true` when the current snapshot has no entries
`crates/shell/src/devtools/network_panel.rs:387` **fn** `scroll_up` — Scroll up by `n` rows (towards older requests)
`crates/shell/src/devtools/network_panel.rs:393` **fn** `scroll_down` — Scroll down by `n` rows (towards newer requests)
`crates/shell/src/devtools/network_panel.rs:405` **fn** `build_network_panel` — Build the viewport-locked network panel overlay
`crates/shell/src/download.rs:45` **struct** `DownloadId` — Opaque identifier for a single download entry
`crates/shell/src/download.rs:50` **enum** `DownloadStatus` — Current state of a download entry
`crates/shell/src/download.rs:71` **struct** `DownloadEntry` — A single download: source URL, destination path, and current status
`crates/shell/src/download.rs:93` **fn** `progress_fraction` — Fraction written so far in `0.0..=1.0`, or `None` when the total size is
`crates/shell/src/download.rs:106` **enum** `DownloadAction` — The result of hit-testing a click against the download panel
`crates/shell/src/download.rs:143` **struct** `DownloadManager` — Manages concurrent background downloads and the visibility of the download
`crates/shell/src/download.rs:163` **fn** `new` — Create a new, empty download manager
`crates/shell/src/download.rs:182` **fn** `start_download` — Start a background download of `url` into `dest`
`crates/shell/src/download.rs:219` **fn** `cancel` — Request cancellation of download `id`
`crates/shell/src/download.rs:236` **fn** `open_download` — Open the file in the default OS application
`crates/shell/src/download.rs:250` **fn** `show_in_folder` — Reveal the downloaded file in the OS file manager (Explorer / Finder /
`crates/shell/src/download.rs:269` **fn** `start_url_download` — Start a download of `url`, choosing a destination automatically
`crates/shell/src/download.rs:283` **fn** `poll` — Drain the internal mpsc channel and update entry statuses
`crates/shell/src/download.rs:324` **fn** `entries` — All entries in insertion order (most recent last)
`crates/shell/src/download.rs:329` **fn** `active_count` — Number of entries whose status is `InProgress` or `Pending`
`crates/shell/src/download.rs:339` **fn** `toggle_visible` — Toggle panel visibility
`crates/shell/src/download.rs:344` **fn** `open` — Show the panel
`crates/shell/src/download.rs:349` **fn** `close` — Hide the panel
`crates/shell/src/download.rs:725` **fn** `hit_test` — Hit-test a click at `(x, y)` (CSS px) against the download panel
`crates/shell/src/download.rs:755` **fn** `build_download_bar` — Build the viewport-locked download panel overlay
`crates/shell/src/extensions/mod.rs:33` **struct** `ContentScript` — A single content-script entry from `manifest.json`
`crates/shell/src/extensions/mod.rs:42` **struct** `ExtensionManifest` — A parsed `manifest.json` for one extension
`crates/shell/src/extensions/mod.rs:69` **struct** `ExtensionRegistry` — Registry of all installed extensions for the current profile
`crates/shell/src/extensions/mod.rs:84` **fn** `extensions_dir` — Return the extensions directory under the portable browser-data folder
`crates/shell/src/extensions/mod.rs:94` **fn** `load` — Scan the extensions directory and load all valid extensions
`crates/shell/src/extensions/mod.rs:103` **fn** `load_from_dir` — Load extensions from an explicit directory (used in tests)
`crates/shell/src/extensions/mod.rs:130` **fn** `len` — Return the number of loaded extensions
`crates/shell/src/extensions/mod.rs:137` **fn** `is_empty` — Return `true` if no extensions are loaded
`crates/shell/src/extensions/mod.rs:146` **fn** `content_scripts_for_url` — Collect all JS source strings for content scripts that match `page_url`
`crates/shell/src/extensions/mod.rs:311` **fn** `url_matches` — Match `url` against a Chrome-style content-script match pattern
`crates/shell/src/find.rs:29` **struct** `FindState` — Состояние find bar и текущего запроса
`crates/shell/src/find.rs:38` **fn** `is_open`
`crates/shell/src/find.rs:42` **fn** `query`
`crates/shell/src/find.rs:46` **fn** `active_index`
`crates/shell/src/find.rs:50` **fn** `is_regex_mode`
`crates/shell/src/find.rs:54` **fn** `open`
`crates/shell/src/find.rs:58` **fn** `close`
`crates/shell/src/find.rs:64` **fn** `append_str`
`crates/shell/src/find.rs:79` **fn** `backspace`
`crates/shell/src/find.rs:90` **fn** `toggle_regex_mode` — Переключает режим plain-text ↔ regex. Сбрасывает счётчик активного
`crates/shell/src/find.rs:98` **fn** `next` — Циклически переходит к следующему совпадению. `total` — текущее число
`crates/shell/src/find.rs:104` **fn** `prev`
`crates/shell/src/find.rs:115` **struct** `FindMatch` — Найденный матч: bounding box в координатах окна и индекс DrawText-команды
`crates/shell/src/find.rs:128` **fn** `scroll_to_match` — Вычисляет новое значение `scroll_y` так, чтобы `match_rect` попал в
`crates/shell/src/find.rs:152` **fn** `find_matches` — Находит все непересекающиеся вхождения `query` в DrawText-командах `dl`
`crates/shell/src/find.rs:221` **fn** `is_valid_regex_pattern` — Проверяет, является ли `pattern` корректным regex-паттерном
`crates/shell/src/find.rs:238` **fn** `find_matches_regex` — Находит все regex-матчи паттерна `pattern` по [`TextFragment`]-ам
`crates/shell/src/find.rs:314` **struct** `BarOverlay` — Параметры overlay-бара
`crates/shell/src/find.rs:332` **fn** `build_page_with_highlights` — Собирает page-полосу display list-а: исходные команды + highlight-FillRect-ы
`crates/shell/src/find.rs:365` **fn** `build_bar_overlay` — Собирает overlay-полосу: только find-bar (фон + label + input + counter +
`crates/shell/src/find.rs:377` **fn** `build_with_overlay` — Совместимая сборка: page + bar в один list. Только для тестов и dump-режимов
`crates/shell/src/forms.rs:31` **struct** `FormControlState` — Mutable runtime state for a single form control
`crates/shell/src/forms.rs:41` **type** `FormState` — `NodeId` → mutable state map for all form controls on the current page
`crates/shell/src/forms.rs:49` **enum** `FormClickAction` — What the shell should do after a left-click on `node`
`crates/shell/src/forms.rs:72` **fn** `classify_click` — Classify a click on `node` given the current DOM tree
`crates/shell/src/forms.rs:132` **fn** `toggle_details_open` — Toggle the `open` attribute on a `<details>` element in the live DOM
`crates/shell/src/forms.rs:145` **fn** `toggle_checkbox` — Toggle the `checked` attribute on a checkbox input in the live DOM
`crates/shell/src/forms.rs:157` **fn** `set_value` — Set `value` attribute of an input / textarea in the DOM
`crates/shell/src/forms.rs:173` **fn** `apply_range_value` — Update a range input's `value` attribute from a click at `click_x` within
`crates/shell/src/forms.rs:198` **fn** `find_validation_error` — Depth-first walk: find the first form control that fails HTML5 constraint
`crates/shell/src/forms.rs:209` **fn** `find_control_rect_and_error` — Find rect and error message for a specific invalid control
`crates/shell/src/forms.rs:220` **fn** `find_all_validation_errors` — Collect all form controls that fail HTML5 constraint validation
`crates/shell/src/forms.rs:345` **fn** `find_box_rect` — Find the bounding rect of the LayoutBox for `node`. Returns `None` if the
`crates/shell/src/forms.rs:358` **fn** `find_layout_box` — Find the LayoutBox subtree for `node`. Returns `None` if the node has no box
`crates/shell/src/forms.rs:373` **fn** `collect_modal_dialogs` — Walk `doc` and collect all NodeIds with `data-lumen-modal` attribute
`crates/shell/src/forms.rs:397` **fn** `build_dialog_overlay` — Build a `::backdrop` + translated dialog overlay for a modal `<dialog>`
`crates/shell/src/forms.rs:438` **fn** `build_validation_tooltip` — Build a validation tooltip anchored below `anchor` (document coordinates)
`crates/shell/src/forms.rs:498` **fn** `collect_form_entries` — Собрать данные формы для submit — DOM-значения, поверх которых наложен
`crates/shell/src/forms.rs:541` **fn** `build_form_submit_event` — Построить параметры отправки формы: `(action, method, body)`
`crates/shell/src/forms.rs:551` **fn** `encode_form_fields` — Encode form fields for submission. Wraps a FormSubmitEvent::Valid variant
`crates/shell/src/forms.rs:564` **fn** `encode_form_fields_multipart` — Encode form fields as `multipart/form-data` (RFC 7578)
`crates/shell/src/forms.rs:576` **fn** `get_form_enctype` — Return the `enctype` attribute of the `<form>` ancestor of `submit_node`,
`crates/shell/src/forms.rs:594` **fn** `build_form_submit`
`crates/shell/src/forms.rs:626` **fn** `make_get_url` — Построить итоговый URL для GET-формы: добавить `?body` к action URL
`crates/shell/src/forms.rs:666` **fn** `build_color_picker` — Build a color-swatch picker anchored below `anchor` (document coordinates)
`crates/shell/src/forms.rs:703` **fn** `hit_color_swatch` — If viewport-space point `(px, py)` lands on a swatch, return its `[r, g, b]`
`crates/shell/src/forms.rs:724` **fn** `swatch_to_css_color` — Format `[r, g, b]` as CSS `#rrggbb`
`crates/shell/src/forms.rs:734` **struct** `SelectOption` — One entry in a `<select>` dropdown list
`crates/shell/src/forms.rs:757` **fn** `collect_select_options` — Collect all direct `<option>` children of a `<select>` DOM node
`crates/shell/src/forms.rs:794` **fn** `build_select_dropdown` — Build a dropdown overlay anchored below (or above if near the bottom of the
`crates/shell/src/forms.rs:880` **fn** `hit_select_option` — If viewport-space point `(px, py)` lands on an option row, return its index
`crates/shell/src/forms.rs:917` **fn** `apply_select_choice` — Apply the selection of option at `opt_idx` to the `<select>` DOM node:
`crates/shell/src/forms.rs:938` **enum** `DatePickerHit` — What a viewport-space click hit inside an open date picker
`crates/shell/src/forms.rs:967` **fn** `is_leap_year` — True if `year` is a leap year
`crates/shell/src/forms.rs:972` **fn** `days_in_month` — Number of days in the given month (1-based month, Gregorian calendar)
`crates/shell/src/forms.rs:983` **fn** `first_weekday_of_month` — ISO weekday (0=Mon … 6=Sun) of the first day of the given month
`crates/shell/src/forms.rs:998` **fn** `month_name` — English month name, 1-based
`crates/shell/src/forms.rs:1010` **fn** `parse_date_value` — Parse an ISO 8601 date string `YYYY-MM-DD` → `(year, month, day)`
`crates/shell/src/forms.rs:1021` **fn** `format_date_value` — Format `(year, month, day)` as `YYYY-MM-DD`
`crates/shell/src/forms.rs:1027` **fn** `today_year_month` — Return the current year and month derived from the system clock
`crates/shell/src/forms.rs:1051` **fn** `build_date_picker` — Build a calendar date-picker overlay anchored below `anchor` (document coords)
`crates/shell/src/forms.rs:1209` **fn** `hit_date_picker` — Hit-test a viewport-space click `(px, py)` against an open date picker
`crates/shell/src/forms.rs:1271` **fn** `advance_month` — Advance display month by `delta` months (positive = forward, negative = backward)
`crates/shell/src/gc_tick.rs:20` **struct** `GcTick` — Throttled idle GC poller
`crates/shell/src/gc_tick.rs:27` **fn** `new` — Create a new `GcTick`. The first poll fires after [`GC_INTERVAL`] elapses
`crates/shell/src/gc_tick.rs:42` **fn** `poll` — Poll the GC scheduler
`crates/shell/src/hints.rs:18` **struct** `HintItem` — Hint badge for one clickable element
`crates/shell/src/hints.rs:27` **struct** `HintState` — Keyboard hint mode state machine
`crates/shell/src/hints.rs:38` **enum** `HintResult` — Result returned by [`HintState::push_char`]
`crates/shell/src/hints.rs:49` **fn** `is_active` — Whether the hint overlay is currently visible
`crates/shell/src/hints.rs:54` **fn** `open` — Open hint mode with a snapshot of the current page's clickable elements
`crates/shell/src/hints.rs:63` **fn** `close` — Dismiss the overlay without activating anything
`crates/shell/src/hints.rs:71` **fn** `push_char` — Record one typed character and return the resulting state
`crates/shell/src/hints.rs:99` **fn** `typed` — Characters typed so far — used to dim non-matching badges
`crates/shell/src/hints.rs:107` **fn** `items` — Compute viewport-space hint items for the current scroll offsets
`crates/shell/src/hints.rs:172` **fn** `build_hints_overlay` — Build the viewport-locked overlay display list for all active hint badges
`crates/shell/src/image_cache.rs:44` **enum** `DecodedImage` — Decoded image payload shared between the streaming progressive loader and the
`crates/shell/src/image_cache.rs:86` **struct** `DecodedImageCache` — Shared, generation-scoped decoded-image cache for page `<img>` resources
`crates/shell/src/image_cache.rs:100` **fn** `reset` — Drop all cached entries and adopt navigation `generation`
`crates/shell/src/image_cache.rs:112` **fn** `reset_new` — Drop all cached entries and bump to a fresh generation
`crates/shell/src/image_cache.rs:119` **fn** `current_generation` — The navigation generation the cache is currently scoped to
`crates/shell/src/image_cache.rs:130` **fn** `get_or_decode` — Decode `src` through the cache for navigation `generation`
`crates/shell/src/image_cache.rs:173` **fn** `get_or_decode_current` — Convenience for the UI-thread consumer ([`fetch_and_decode_images`]): decode
`crates/shell/src/input/gesture.rs:36` **enum** `GestureDir` — Six-way gesture direction code
`crates/shell/src/input/gesture.rs:55` **enum** `GestureAction` — Shell action emitted when a completed gesture matches a binding
`crates/shell/src/input/gesture.rs:81` **struct** `GestureMap` — Configurable mapping from [`GestureDir`] to [`GestureAction`]
`crates/shell/src/input/gesture.rs:97` **fn** `empty` — Empty map — no bindings
`crates/shell/src/input/gesture.rs:103` **fn** `bind` — Bind `dir` to `action`, replacing any previous binding
`crates/shell/src/input/gesture.rs:109` **fn** `unbind` — Remove the binding for `dir`
`crates/shell/src/input/gesture.rs:114` **fn** `lookup` — Return the action bound to `dir`, or `None` if unbound
`crates/shell/src/input/gesture.rs:150` **struct** `GestureRecognizer` — State machine for recognizing right-button drag mouse gestures
`crates/shell/src/input/gesture.rs:157` **fn** `new` — Create a recognizer with the default gesture map
`crates/shell/src/input/gesture.rs:163` **fn** `with_map` — Create a recognizer with a custom gesture map
`crates/shell/src/input/gesture.rs:169` **fn** `set_map` — Replace the gesture map at runtime (e.g. from settings)
`crates/shell/src/input/gesture.rs:175` **fn** `map` — Shared reference to the current gesture map
`crates/shell/src/input/gesture.rs:181` **fn** `map_mut` — Mutable reference to the current gesture map
`crates/shell/src/input/gesture.rs:189` **fn** `begin` — Begin tracking a right-button drag from `(x, y)` in CSS pixels
`crates/shell/src/input/gesture.rs:197` **fn** `track` — Update the current drag end-point
`crates/shell/src/input/gesture.rs:211` **fn** `finish` — Finish the drag and return the mapped [`GestureAction`], if any
`crates/shell/src/input/gesture.rs:226` **fn** `cancel` — Cancel the in-progress drag without emitting an action
`crates/shell/src/input/gesture.rs:232` **fn** `is_active` — Returns `true` while a right-button drag is being tracked
`crates/shell/src/input/humanlike.rs:136` **struct** `HumanLikeConfig` — Timing and motion parameters for [`HumanLikeSender`]
`crates/shell/src/input/humanlike.rs:177` **enum** `InputMode` — Controls how injected inputs are delivered to the shell
`crates/shell/src/input/humanlike.rs:202` **struct** `HumanLikeSender` — Wraps [`InputSender`] and injects human-like timing and mouse motion
`crates/shell/src/input/humanlike.rs:216` **fn** `new` — Create a new sender wrapping `inner` with default configuration
`crates/shell/src/input/humanlike.rs:226` **fn** `with_seed` — Create a sender with a fixed PRNG seed for deterministic replay
`crates/shell/src/input/humanlike.rs:235` **fn** `click_at` — Move the cursor along a Bézier arc to `(x, y)`, then dwell, then click
`crates/shell/src/input/humanlike.rs:267` **fn** `type_text` — Type `text` with Gaussian-distributed inter-keystroke delays
`crates/shell/src/input/humanlike.rs:287` **fn** `scroll_to` — Scroll to `(x, y)` immediately (no path animation for scrolls)
`crates/shell/src/input/humanlike.rs:295` **fn** `set_cursor_position` — Override the assumed cursor starting position without moving it
`crates/shell/src/input/mod.rs:40` **enum** `InputCommand` — A single injected input command
`crates/shell/src/input/mod.rs:107` **struct** `InputSender` — Sender side of the input injection channel
`crates/shell/src/input/mod.rs:112` **fn** `click` — Send a synthetic left-click at CSS-pixel coordinates `(x, y)`
`crates/shell/src/input/mod.rs:118` **fn** `mouse_move` — Send a synthetic mouse-move event to CSS-pixel coordinates `(x, y)`
`crates/shell/src/input/mod.rs:124` **fn** `type_text` — Send a synthetic text-typing command
`crates/shell/src/input/mod.rs:130` **fn** `scroll` — Send a synthetic scroll command to position `(x, y)` in CSS pixels
`crates/shell/src/input/mod.rs:140` **fn** `key_down` — Press and release a special key identified by its W3C `KeyboardEvent.code`
`crates/shell/src/input/mod.rs:146` **fn** `enter` — Press Enter in the focused element (submits forms, confirms dialogs)
`crates/shell/src/input/mod.rs:152` **fn** `backspace` — Press Backspace in the focused element (deletes character before cursor)
`crates/shell/src/input/mod.rs:158` **fn** `tab` — Press Tab (move focus to the next focusable element)
`crates/shell/src/input/mod.rs:164` **fn** `escape` — Press Escape (dismiss dialogs, close menus, blur focused element)
`crates/shell/src/input/mod.rs:172` **struct** `InputReceiver` — Receiver side of the input injection channel
`crates/shell/src/input/mod.rs:176` **fn** `drain` — Non-blocking drain: returns all pending commands without blocking
`crates/shell/src/input/mod.rs:185` **fn** `channel` — Create a new input injection channel
`crates/shell/src/input/vim.rs:41` **enum** `VimState` — Which sub-mode the Vim keybinding layer is currently in
`crates/shell/src/input/vim.rs:61` **enum** `VimAction` — Decoded action that the caller should execute in response to a keypress
`crates/shell/src/input/vim.rs:106` **struct** `VimMode` — Vim-mode state machine
`crates/shell/src/input/vim.rs:115` **fn** `new` — Create a new `VimMode` in [`VimState::Normal`]
`crates/shell/src/input/vim.rs:123` **fn** `feed` — Feed one physical key event.  Returns the action to take
`crates/shell/src/links.rs:15` **fn** `find_link_href` — Walk up the ancestor chain from `node_id` to find the nearest `<a>` element
`crates/shell/src/links.rs:43` **fn** `is_navigable_href` — Return true if `href` is a URL scheme the browser should navigate to
`crates/shell/src/links.rs:53` **fn** `fragment_only` — If `href` is a fragment-only reference (starts with `#`), return the
`crates/shell/src/links.rs:63` **fn** `fragment_url` — Build the absolute URL for a same-document fragment navigation: replaces the
`crates/shell/src/links.rs:87` **fn** `same_document_fragment` — Determine whether navigating from `current` to `resolved` is a same-document
`crates/shell/src/links.rs:111` **fn** `find_element_by_id` — Walk the document tree and return the first element whose `id` attribute
`crates/shell/src/memory_poll.rs:23` **struct** `MemoryPollTick` — Throttled memory pressure poller
`crates/shell/src/memory_poll.rs:36` **fn** `new` — Create a new poller using the given platform source
`crates/shell/src/memory_poll.rs:49` **fn** `tick` — Poll memory pressure and broadcast to `registry` if pressure is Medium or High
`crates/shell/src/memory_poll.rs:66` **fn** `last_level` — Last sampled pressure level.  May be stale by up to [`POLL_INTERVAL`]
`crates/shell/src/memory_poll.rs:75` **fn** `platform_source` — Build the appropriate [`MemoryPressureSource`] for the current platform
`crates/shell/src/momentum_anim.rs:26` **struct** `MomentumAnim` — Velocity-based momentum анимация. Хранится в `Lumen.momentum_anim`
`crates/shell/src/momentum_anim.rs:36` **fn** `new`
`crates/shell/src/momentum_anim.rs:43` **fn** `advance` — Прогнать анимацию до `now_ms`. Возвращает `(Δy, Δx, done)`
`crates/shell/src/network_service.rs:26` **struct** `NetworkServiceHandle` — Хендл живого подпроцесса `lumen-network-service`
`crates/shell/src/network_service.rs:38` **fn** `spawn` — Запустить `lumen-network-service` из той же директории, что и текущий исполняемый файл
`crates/shell/src/newtab.rs:23` **struct** `TopSite` — Одна плитка speed dial: целевой URL и отображаемый заголовок
`crates/shell/src/newtab.rs:85` **fn** `build_newtab_html` — Строит полный HTML страницы `about:newtab` со speed dial из `sites`
`crates/shell/src/notification.rs:18` **fn** `show_os_notification` — Show a desktop notification asynchronously
`crates/shell/src/omnibox/mod.rs:20` **enum** `AliasAction` — Action produced by resolving a raw omnibox input against the alias table
`crates/shell/src/omnibox/mod.rs:39` **fn** `resolve` — Resolve `input` against the alias table and built-in `@` actions
`crates/shell/src/page_context_menu.rs:45` **enum** `SpellMenuAction` — An action the user can pick from the spell suggestion menu
`crates/shell/src/page_context_menu.rs:58` **struct** `SpellTarget` — Everything the shell needs to apply the chosen action: which control holds
`crates/shell/src/page_context_menu.rs:72` **fn** `word` — The misspelled word slice
`crates/shell/src/page_context_menu.rs:77` **fn** `apply` — Rebuild the control's value with the word replaced by `replacement`
`crates/shell/src/page_context_menu.rs:88` **struct** `PageContextMenu` — State of the page-level spell suggestion menu. One menu is open at a time
`crates/shell/src/page_context_menu.rs:107` **fn** `open_for` — Open the menu at cursor `(x, y)` for `target`, offering `suggestions`
`crates/shell/src/page_context_menu.rs:121` **fn** `close` — Hide the menu and drop its context
`crates/shell/src/page_context_menu.rs:129` **fn** `is_open` — `true` while the menu is visible
`crates/shell/src/page_context_menu.rs:134` **fn** `target` — The target context (word + control), if the menu is open
`crates/shell/src/page_context_menu.rs:158` **fn** `item_at` — Map a CSS-px `(x, y)` to the row index under it, or `None`
`crates/shell/src/page_context_menu.rs:176` **fn** `action_at` — Map a CSS-px `(x, y)` to the [`SpellMenuAction`] under it, or `None`
`crates/shell/src/page_context_menu.rs:181` **fn** `build_overlay` — Build a viewport-locked display list for the open menu; empty when closed
`crates/shell/src/panel_layout.rs:46` **enum** `Dock` — Which window edge a docked sidebar hugs
`crates/shell/src/panel_layout.rs:58` **fn** `width_from_cursor` — Resolve the dragged cursor x-position into a panel width for this dock,
`crates/shell/src/panel_layout.rs:67` **fn** `opposite` — The opposite window edge (used by cross-dock "move to other side")
`crates/shell/src/panel_layout.rs:76` **fn** `as_token` — Lowercase token used in the persisted layout file (`left` / `right`)
`crates/shell/src/panel_layout.rs:85` **fn** `from_token` — Parse a persisted token; `None` for anything but `left` / `right`
`crates/shell/src/panel_layout.rs:100` **fn** `default_dock` — Compiled default dock side for a panel id
`crates/shell/src/panel_layout.rs:129` **struct** `PanelLayout` — Runtime, persisted widths of the docked panels, keyed by panel id
`crates/shell/src/panel_layout.rs:154` **fn** `load` — Load the persisted layout, or an empty (all-default) layout if the file
`crates/shell/src/panel_layout.rs:225` **fn** `width_for` — Width to use for the panel `id`, falling back to `default` when the user
`crates/shell/src/panel_layout.rs:235` **fn** `set_width` — Record a new width for panel `id` (clamped). Returns `true` if the stored
`crates/shell/src/panel_layout.rs:252` **fn** `dock_for` — Effective dock side for panel `id`: the user's cross-dock override, or
`crates/shell/src/panel_layout.rs:258` **fn** `set_dock` — Record a dock side for panel `id`. Returns `true` if the stored value
`crates/shell/src/panel_layout.rs:272` **fn** `save` — Persist the layout to disk (best-effort)
`crates/shell/src/panels/a11y_panel.rs:66` **struct** `A11yPanel` — Accessibility settings panel state
`crates/shell/src/panels/a11y_panel.rs:75` **fn** `new` — Create a new hidden panel with default preferences
`crates/shell/src/panels/a11y_panel.rs:87` **fn** `toggle` — Toggle panel visibility
`crates/shell/src/panels/a11y_panel.rs:92` **fn** `load_draft` — Load current preferences into the draft so edits start from persisted values
`crates/shell/src/panels/a11y_panel.rs:107` **enum** `A11yHit` — Result of a click on (or near) the accessibility panel
`crates/shell/src/panels/a11y_panel.rs:133` **fn** `hit_test` — Classify a click at `(x, y)` CSS px
`crates/shell/src/panels/a11y_panel.rs:223` **fn** `build_a11y_panel` — Build the centred accessibility settings panel overlay
`crates/shell/src/panels/ai_panel.rs:57` **struct** `AiPanel` — AI assistant sidebar panel state (§12.8)
`crates/shell/src/panels/ai_panel.rs:70` **fn** `new` — Create a new hidden AI panel with empty input and response
`crates/shell/src/panels/ai_panel.rs:80` **fn** `toggle` — Toggle panel visibility
`crates/shell/src/panels/ai_panel.rs:85` **fn** `close` — Close the panel (hide; input and response are preserved)
`crates/shell/src/panels/ai_panel.rs:90` **fn** `push_char` — Append a character to the input field
`crates/shell/src/panels/ai_panel.rs:95` **fn** `backspace` — Remove the last character from the input field (backspace)
`crates/shell/src/panels/ai_panel.rs:110` **enum** `AiHit` — Result of a click inside the AI panel
`crates/shell/src/panels/ai_panel.rs:124` **fn** `hit_test` — Hit-test `(x, y)` in CSS px against the AI panel
`crates/shell/src/panels/ai_panel.rs:169` **fn** `build_panel` — Build the display list for the AI sidebar panel
`crates/shell/src/panels/bookmark_panel.rs:87` **struct** `BmEntry` — Lightweight bookmark entry used for panel rendering (loaded from the
`crates/shell/src/panels/bookmark_panel.rs:101` **struct** `BookmarkPanel` — Bookmark manager panel state
`crates/shell/src/panels/bookmark_panel.rs:123` **fn** `new` — Create a new (hidden) panel with an empty bookmark list
`crates/shell/src/panels/bookmark_panel.rs:137` **fn** `toggle` — Flip visibility.  Resets transient state (search focus, drag) when hiding
`crates/shell/src/panels/bookmark_panel.rs:146` **fn** `set_data` — Replace the cached bookmark list and recompute the folder set
`crates/shell/src/panels/bookmark_panel.rs:166` **fn** `visible_entries` — Bookmarks visible under the current folder filter and search query, in
`crates/shell/src/panels/bookmark_panel.rs:183` **fn** `append_search` — Append typed text to the search query (called while `search_active`)
`crates/shell/src/panels/bookmark_panel.rs:189` **fn** `backspace_search` — Delete the last character of the search query
`crates/shell/src/panels/bookmark_panel.rs:195` **fn** `begin_drag` — Begin dragging the bookmark with the given id
`crates/shell/src/panels/bookmark_panel.rs:200` **fn** `take_drag` — Take (and clear) the dragged bookmark id, if a drag is in progress
`crates/shell/src/panels/bookmark_panel.rs:207` **fn** `scroll_by` — Scroll the bookmark list by `dy` CSS px, clamped to `[0, max]` where
`crates/shell/src/panels/bookmark_panel.rs:227` **enum** `BookmarkHit` — Result of a click inside the bookmark panel
`crates/shell/src/panels/bookmark_panel.rs:244` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the panel anchored with its
`crates/shell/src/panels/bookmark_panel.rs:302` **fn** `build_panel` — Build the display list for the panel anchored at `(ax, ay)` (top-left)
`crates/shell/src/panels/cert_panel.rs:55` **struct** `PanelCertData` — Certificate data shown in the panel
`crates/shell/src/panels/cert_panel.rs:78` **fn** `has_data` — Returns `true` if there is meaningful data to display
`crates/shell/src/panels/cert_panel.rs:87` **struct** `CertPanel` — Certificate viewer panel state
`crates/shell/src/panels/cert_panel.rs:98` **fn** `new` — Create a new, hidden panel
`crates/shell/src/panels/cert_panel.rs:105` **fn** `open` — Open the panel with the given certificate data
`crates/shell/src/panels/cert_panel.rs:112` **fn** `close` — Close the panel
`crates/shell/src/panels/cert_panel.rs:117` **fn** `toggle` — Toggle visibility.  On open: resets scroll to top
`crates/shell/src/panels/cert_panel.rs:126` **fn** `scroll_by` — Scroll the content by `delta` CSS px (positive = down)
`crates/shell/src/panels/cert_panel.rs:134` **fn** `hit_test` — Hit-test a pointer position relative to panel origin
`crates/shell/src/panels/cert_panel.rs:147` **enum** `CertHit` — Result of a pointer hit test on the cert panel
`crates/shell/src/panels/cert_panel.rs:243` **fn** `build_panel` — Append display commands for the cert panel to `buf`
`crates/shell/src/panels/command_palette.rs:80` **enum** `PaletteAction` — A built-in browser action invokable from the palette
`crates/shell/src/panels/command_palette.rs:111` **fn** `label` — Human-readable label shown in the result row
`crates/shell/src/panels/command_palette.rs:130` **fn** `shortcut` — Keyboard-shortcut hint rendered right-aligned in the row (`""` if none)
`crates/shell/src/panels/command_palette.rs:150` **fn** `all` — The full curated command list, in display order (shown first when the
`crates/shell/src/panels/command_palette.rs:174` **enum** `PaletteKind` — What kind of target a palette item represents (drives the row icon and the
`crates/shell/src/panels/command_palette.rs:185` **struct** `PaletteItem` — A single searchable entry in the palette
`crates/shell/src/panels/command_palette.rs:196` **fn** `command` — Build a command item
`crates/shell/src/panels/command_palette.rs:205` **fn** `bookmark` — Build a bookmark item (falls back to the URL when the title is empty)
`crates/shell/src/panels/command_palette.rs:211` **fn** `history` — Build a history item (falls back to the URL when the title is empty)
`crates/shell/src/panels/command_palette.rs:230` **struct** `CommandPalette` — Command palette modal state
`crates/shell/src/panels/command_palette.rs:247` **fn** `new` — Create a hidden palette with the curated command list pre-loaded
`crates/shell/src/panels/command_palette.rs:253` **fn** `open` — Open the palette, resetting the query and selection
`crates/shell/src/panels/command_palette.rs:261` **fn** `close` — Close the palette
`crates/shell/src/panels/command_palette.rs:266` **fn** `toggle` — Toggle visibility; opening resets transient state
`crates/shell/src/panels/command_palette.rs:277` **fn** `set_items` — Replace the item list (commands + bookmarks + history) and clamp the
`crates/shell/src/panels/command_palette.rs:283` **fn** `append` — Append typed text to the query and reset the selection to the top
`crates/shell/src/panels/command_palette.rs:290` **fn** `backspace` — Delete the last character of the query
`crates/shell/src/panels/command_palette.rs:301` **fn** `filtered` — Indices into `items` matching the current query, best match first
`crates/shell/src/panels/command_palette.rs:318` **fn** `select_next` — Move the selection down by one (clamped to the last result)
`crates/shell/src/panels/command_palette.rs:328` **fn** `select_prev` — Move the selection up by one (clamped to the first result)
`crates/shell/src/panels/command_palette.rs:336` **fn** `selected_item` — The currently highlighted item index into `items`, if any result exists
`crates/shell/src/panels/command_palette.rs:377` **fn** `fuzzy_score` — Score `haystack` against `needle` as a case-insensitive subsequence match
`crates/shell/src/panels/command_palette.rs:427` **enum** `PaletteHit` — Result of a click inside the modal palette
`crates/shell/src/panels/command_palette.rs:451` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the modal palette in a
`crates/shell/src/panels/command_palette.rs:477` **fn** `build_panel` — Build the display list for the modal palette over a `viewport_w`×`viewport_h`
`crates/shell/src/panels/focus_panel.rs:74` **struct** `PomodoroTimer` — Wall-clock-driven countdown timer
`crates/shell/src/panels/focus_panel.rs:90` **fn** `new` — Create a running timer of `duration_min` minutes with zero elapsed time
`crates/shell/src/panels/focus_panel.rs:102` **fn** `tick` — Advance the timer to wall-clock `now_ms`.  Adds the delta since the last
`crates/shell/src/panels/focus_panel.rs:113` **fn** `remaining_ms` — Remaining time in milliseconds, clamped to `>= 0`
`crates/shell/src/panels/focus_panel.rs:118` **fn** `progress` — Elapsed fraction in `[0, 1]`.  Returns `1.0` for a zero-length duration
`crates/shell/src/panels/focus_panel.rs:126` **fn** `is_finished` — `true` once the full duration has elapsed
`crates/shell/src/panels/focus_panel.rs:131` **fn** `pause` — Pause counting.  Clears the tick baseline so the paused span is excluded
`crates/shell/src/panels/focus_panel.rs:138` **fn** `resume` — Resume counting.  Clears the tick baseline so the gap before the next
`crates/shell/src/panels/focus_panel.rs:144` **fn** `toggle_pause` — Flip between paused and running
`crates/shell/src/panels/focus_panel.rs:153` **fn** `label` — Remaining time formatted as `MM:SS` (rounded up to whole seconds)
`crates/shell/src/panels/focus_panel.rs:164` **struct** `FocusModePanel` — Focus-mode panel state: the active flag plus the embedded [`PomodoroTimer`]
`crates/shell/src/panels/focus_panel.rs:173` **fn** `new` — Create an inactive panel with a default-length (paused-at-zero) timer
`crates/shell/src/panels/focus_panel.rs:181` **fn** `enter` — Enter focus mode with a fresh `duration_min`-minute timer
`crates/shell/src/panels/focus_panel.rs:187` **fn** `exit` — Leave focus mode (the timer state is kept but no longer ticked)
`crates/shell/src/panels/focus_panel.rs:192` **fn** `toggle` — Toggle focus mode: enter with `duration_min` when off, else exit
`crates/shell/src/panels/focus_panel.rs:201` **fn** `tick` — Advance the embedded timer to `now_ms` when active (no-op otherwise)
`crates/shell/src/panels/focus_panel.rs:218` **enum** `FocusHit` — Result of a click inside the focus widget card
`crates/shell/src/panels/focus_panel.rs:234` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the focus widget card
`crates/shell/src/panels/focus_panel.rs:257` **fn** `build_panel` — Build the display list for the focus widget overlay
`crates/shell/src/panels/history_panel.rs:84` **struct** `HistoryItem` — Lightweight history entry for panel rendering
`crates/shell/src/panels/history_panel.rs:99` **enum** `HistoryRow` — One display row in the scrollable body — either a date-group header or an entry
`crates/shell/src/panels/history_panel.rs:108` **struct** `HistoryPanel` — History panel state
`crates/shell/src/panels/history_panel.rs:138` **fn** `new` — Create a new, hidden panel
`crates/shell/src/panels/history_panel.rs:143` **fn** `toggle` — Toggle visibility and reset scroll/search when opening
`crates/shell/src/panels/history_panel.rs:152` **fn** `set_items` — Replace the displayed rows (call after data refresh or search)
`crates/shell/src/panels/history_panel.rs:157` **fn** `append_search` — Append a character to the search query
`crates/shell/src/panels/history_panel.rs:162` **fn** `backspace_search` — Delete the last character from the search query
`crates/shell/src/panels/history_panel.rs:167` **fn** `scroll_by` — Scroll by `dy` CSS px (positive = down)
`crates/shell/src/panels/history_panel.rs:173` **fn** `max_scroll` — Maximum scroll offset for the current row set
`crates/shell/src/panels/history_panel.rs:214` **enum** `HistoryHit` — Result of a click inside the history panel
`crates/shell/src/panels/history_panel.rs:234` **fn** `hit_test` — Classify a click at `(mx, my)` in window-space CSS px
`crates/shell/src/panels/history_panel.rs:287` **fn** `build_panel` — Build the panel display list
`crates/shell/src/panels/note_viewer.rs:59` **enum** `NoteHit` — Which region of the overlay was hit by a mouse click
`crates/shell/src/panels/note_viewer.rs:70` **struct** `NoteViewerPanel` — Floating overlay for displaying a single user annotation
`crates/shell/src/panels/note_viewer.rs:85` **fn** `new` — Create a hidden panel with empty state
`crates/shell/src/panels/note_viewer.rs:96` **fn** `open` — Show the panel populated with the given note data
`crates/shell/src/panels/note_viewer.rs:105` **fn** `close` — Hide the panel (data is preserved for re-open)
`crates/shell/src/panels/note_viewer.rs:110` **fn** `panel_height` — Total height of the overlay given the current content
`crates/shell/src/panels/note_viewer.rs:118` **fn** `hit_test` — Hit-test a click at `(px, py)` in viewport coordinates
`crates/shell/src/panels/note_viewer.rs:153` **fn** `build_note_viewer` — Build the display list for the note viewer overlay
`crates/shell/src/panels/permission_panel.rs:56` **enum** `PermissionKind` — A single browser permission kind tracked by the panel
`crates/shell/src/panels/permission_panel.rs:77` **fn** `label` — Short display name for the permission row label
`crates/shell/src/panels/permission_panel.rs:87` **fn** `icon` — Emoji icon shown to the left of the label
`crates/shell/src/panels/permission_panel.rs:99` **enum** `PermissionState` — Grant state for a single permission on a single origin
`crates/shell/src/panels/permission_panel.rs:112` **fn** `label` — Label shown on the toggle button
`crates/shell/src/panels/permission_panel.rs:121` **fn** `cycle` — Cycle to the next state: Ask → Allow → Deny → Ask
`crates/shell/src/panels/permission_panel.rs:133` **struct** `PermissionPanel` — Per-site permission popover state (7C.2)
`crates/shell/src/panels/permission_panel.rs:148` **fn** `new` — Create a new hidden panel with no stored permissions
`crates/shell/src/panels/permission_panel.rs:157` **fn** `toggle` — Flip panel visibility
`crates/shell/src/panels/permission_panel.rs:162` **fn** `set_origin` — Update the current origin on navigation (does not clear stored grants)
`crates/shell/src/panels/permission_panel.rs:169` **fn** `state_for` — Return the stored state for `kind` at the current origin
`crates/shell/src/panels/permission_panel.rs:182` **fn** `cycle_permission` — Cycle the state for `kind` at the current origin to the next value
`crates/shell/src/panels/permission_panel.rs:205` **enum** `PermissionHit` — Result of a click inside the permission panel
`crates/shell/src/panels/permission_panel.rs:218` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the permission panel
`crates/shell/src/panels/permission_panel.rs:262` **fn** `build_panel` — Build the display list for the permission floating panel
`crates/shell/src/panels/pip_os_window.rs:53` **struct** `PipOsConfig` — Geometry for the floating PiP window, in logical (CSS) pixels
`crates/shell/src/panels/pip_os_window.rs:87` **fn** `pip_window_attributes` — Build the winit attributes for the floating PiP window
`crates/shell/src/panels/pip_os_window.rs:116` **fn** `build_pip_content` — Build the display list shown in the floating PiP window for a `<video>`
`crates/shell/src/panels/pip_os_window.rs:164` **enum** `PipAction` — What the shell should do after feeding a request into [`PipController`]
`crates/shell/src/panels/pip_os_window.rs:179` **struct** `PipController` — Tracks which `<video>` (by node id) currently owns the OS PiP window
`crates/shell/src/panels/pip_os_window.rs:186` **fn** `new` — Create an idle controller with no active PiP window
`crates/shell/src/panels/pip_os_window.rs:195` **fn** `active` — Node id of the element currently in OS PiP, or `None`
`crates/shell/src/panels/pip_os_window.rs:201` **fn** `is_active` — `true` while an OS PiP window should be shown
`crates/shell/src/panels/pip_os_window.rs:206` **fn** `on_enter` — Handle `_lumen_pip_enter(nid)`: open or re-target the floating window
`crates/shell/src/panels/pip_os_window.rs:215` **fn** `on_exit` — Handle `_lumen_pip_exit(_)` or an OS close button: tear the window down
`crates/shell/src/panels/pip_window.rs:65` **struct** `PipWindow` — Picture-in-picture window state
`crates/shell/src/panels/pip_window.rs:88` **fn** `new` — Create an inactive PiP window positioned at the origin (re-anchored to the
`crates/shell/src/panels/pip_window.rs:102` **fn** `open` — Open the PiP card for a `<video>` source, anchored to the bottom-right of
`crates/shell/src/panels/pip_window.rs:120` **fn** `close` — Close the card (state is retained but no longer drawn)
`crates/shell/src/panels/pip_window.rs:126` **fn** `toggle_play` — Flip the play / pause flag
`crates/shell/src/panels/pip_window.rs:131` **fn** `default_pos` — Default bottom-right anchored top-left corner for a `win_w`×`win_h` window
`crates/shell/src/panels/pip_window.rs:140` **fn** `clamp_to_window` — Clamp the card so it stays fully inside a `win_w`×`win_h` window, leaving
`crates/shell/src/panels/pip_window.rs:148` **fn** `begin_drag` — Begin dragging the card: record the pointer offset from the card origin
`crates/shell/src/panels/pip_window.rs:153` **fn** `dragging` — `true` while a title-bar drag is in progress
`crates/shell/src/panels/pip_window.rs:159` **fn** `drag_to` — Update the card position from the pointer during a drag, clamped to the
`crates/shell/src/panels/pip_window.rs:167` **fn** `end_drag` — End an in-progress drag
`crates/shell/src/panels/pip_window.rs:182` **enum** `PipHit` — Result of a click inside the PiP card
`crates/shell/src/panels/pip_window.rs:198` **fn** `hit_test` — Hit-test a click at window CSS-px `(x, y)` against the PiP card
`crates/shell/src/panels/pip_window.rs:235` **fn** `build_panel` — Build the display list for the PiP overlay.  Empty when inactive
`crates/shell/src/panels/print_panel.rs:57` **enum** `PaperSize` — Paper size for the print job
`crates/shell/src/panels/print_panel.rs:68` **enum** `Orientation` — Page orientation for the print job
`crates/shell/src/panels/print_panel.rs:77` **enum** `MarginPreset` — Margin preset for the print job
`crates/shell/src/panels/print_panel.rs:88` **enum** `ColorMode` — Output colour mode for the print job
`crates/shell/src/panels/print_panel.rs:97` **enum** `PrintField` — Which editable text field currently has keyboard focus in the print panel
`crates/shell/src/panels/print_panel.rs:111` **struct** `PrintPanel` — Print dialog panel state
`crates/shell/src/panels/print_panel.rs:138` **fn** `new` — Create a new hidden panel with default print settings
`crates/shell/src/panels/print_panel.rs:154` **fn** `toggle` — Toggle panel visibility; clears the active editing field on hide
`crates/shell/src/panels/print_panel.rs:162` **fn** `close` — Hide the panel and clear the editing field
`crates/shell/src/panels/print_panel.rs:168` **fn** `push_char` — Append a character to the currently focused text field
`crates/shell/src/panels/print_panel.rs:177` **fn** `pop_char` — Delete the last character from the currently focused text field
`crates/shell/src/panels/print_panel.rs:188` **fn** `margin_px` — Resolve margin values (top/bottom, left/right) in CSS px at 96 DPI
`crates/shell/src/panels/print_panel.rs:207` **enum** `PrintHit` — Result of a click on (or near) the print panel
`crates/shell/src/panels/print_panel.rs:252` **fn** `hit_test` — Classify a click at `(x, y)` CSS px
`crates/shell/src/panels/print_panel.rs:406` **fn** `build_panel` — Build the centred print dialog overlay
`crates/shell/src/panels/privacy_panel.rs:71` **fn** `list_body_height` — Height in CSS px of the scrollable request-list area, given the full window
`crates/shell/src/panels/privacy_panel.rs:80` **struct** `PrivacyPanel` — Privacy network panel (V5). Holds a snapshot of the shared [`NetworkLog`] and
`crates/shell/src/panels/privacy_panel.rs:96` **fn** `new` — Create a new hidden panel backed by the given shared `log`
`crates/shell/src/panels/privacy_panel.rs:106` **fn** `toggle` — Toggle panel visibility
`crates/shell/src/panels/privacy_panel.rs:112` **fn** `refresh` — Pull the latest entries from the shared [`NetworkLog`] into the snapshot
`crates/shell/src/panels/privacy_panel.rs:121` **fn** `clear_log` — Clear the shared log (call on every top-level navigation). The network
`crates/shell/src/panels/privacy_panel.rs:131` **fn** `len` — Number of entries in the current snapshot
`crates/shell/src/panels/privacy_panel.rs:137` **fn** `is_empty` — `true` when the current snapshot has no entries
`crates/shell/src/panels/privacy_panel.rs:142` **fn** `blocked_count` — Number of blocked requests in the current snapshot
`crates/shell/src/panels/privacy_panel.rs:148` **fn** `allowed_count` — Number of allowed (not blocked) requests in the current snapshot —
`crates/shell/src/panels/privacy_panel.rs:159` **fn** `scroll_down` — Scroll towards older requests by `n` rows
`crates/shell/src/panels/privacy_panel.rs:164` **fn** `scroll_up` — Scroll towards newer requests by `n` rows
`crates/shell/src/panels/privacy_panel.rs:173` **enum** `PrivacyHit` — Result of a click on (or near) the privacy panel
`crates/shell/src/panels/privacy_panel.rs:184` **fn** `hit_test` — Classify a click at `(x, y)` CSS px. `tab_bar_h` is the tab strip height;
`crates/shell/src/panels/privacy_panel.rs:215` **fn** `build_privacy_panel` — Build the right-docked privacy panel overlay
`crates/shell/src/panels/read_later_panel.rs:51` **struct** `ReadLaterPanel` — Read-later panel state
`crates/shell/src/panels/read_later_panel.rs:61` **fn** `new`
`crates/shell/src/panels/read_later_panel.rs:66` **fn** `toggle` — Toggle visibility; resets scroll when opening
`crates/shell/src/panels/read_later_panel.rs:74` **fn** `refresh` — Replace the cached entry list (call after save/delete or on open)
`crates/shell/src/panels/read_later_panel.rs:78` **fn** `scroll_up`
`crates/shell/src/panels/read_later_panel.rs:82` **fn** `scroll_down`
`crates/shell/src/panels/read_later_panel.rs:87` **fn** `max_scroll` — Maximum scroll offset for the current entry count
`crates/shell/src/panels/read_later_panel.rs:98` **enum** `ReadLaterHit` — Result of a click inside or near the panel
`crates/shell/src/panels/read_later_panel.rs:114` **fn** `hit_test` — Classify a click at `(mx, my)` (window-space CSS px)
`crates/shell/src/panels/read_later_panel.rs:152` **fn** `build_panel` — Build the panel display list
`crates/shell/src/panels/read_later_panel.rs:352` **fn** `extract_title_from_html` — Extract the page title from raw HTML bytes
`crates/shell/src/panels/restore_spinner.rs:24` **fn** `build_spinner` — Build spinner overlay if restore has taken longer than THRESHOLD_MS
`crates/shell/src/panels/settings_panel.rs:63` **enum** `SettingsSection` — The four top-level settings sections
`crates/shell/src/panels/settings_panel.rs:85` **fn** `label` — Display label for the tab
`crates/shell/src/panels/settings_panel.rs:99` **enum** `SettingInput` — Which text input currently has keyboard focus
`crates/shell/src/panels/settings_panel.rs:108` **struct** `SettingsPanel` — Settings panel UI state
`crates/shell/src/panels/settings_panel.rs:123` **fn** `new` — Create a new, hidden panel
`crates/shell/src/panels/settings_panel.rs:134` **fn** `open` — Open the panel, loading a fresh snapshot as the working draft
`crates/shell/src/panels/settings_panel.rs:143` **fn** `toggle` — Toggle visibility. When opening, loads `snap` as the draft
`crates/shell/src/panels/settings_panel.rs:152` **fn** `apply_draft` — Clone the current draft for persistence
`crates/shell/src/panels/settings_panel.rs:157` **fn** `append_char` — Append a printable character to the focused text field
`crates/shell/src/panels/settings_panel.rs:166` **fn** `backspace` — Remove the last character from the focused text field
`crates/shell/src/panels/settings_panel.rs:176` **fn** `scroll_by` — Scroll the content area by `dy` CSS px (positive = down)
`crates/shell/src/panels/settings_panel.rs:191` **enum** `SettingsHit` — Result of classifying a click inside the settings panel
`crates/shell/src/panels/settings_panel.rs:222` **fn** `hit_test` — Classify a click at `(mx, my)` in window CSS px. `(px, py)` is the panel
`crates/shell/src/panels/settings_panel.rs:356` **fn** `build_panel` — Append display commands for the settings panel to `list`
`crates/shell/src/panels/shields_panel.rs:62` **struct** `BlockedLog` — Shared accumulator for blocked-request counts, indexed by hostname
`crates/shell/src/panels/shields_panel.rs:73` **fn** `record` — Increment the count for the hostname extracted from `url`
`crates/shell/src/panels/shields_panel.rs:81` **fn** `clear` — Clear all counts (call on every top-level navigation)
`crates/shell/src/panels/shields_panel.rs:87` **fn** `count_for` — Blocked count for a specific hostname (0 if unseen)
`crates/shell/src/panels/shields_panel.rs:100` **struct** `ShieldCountSink` — [`EventSink`] wrapper that forwards every event to an inner sink AND
`crates/shell/src/panels/shields_panel.rs:123` **struct** `ShieldsPanel` — Shields floating panel state (7C.4)
`crates/shell/src/panels/shields_panel.rs:147` **fn** `new` — Create a new hidden panel backed by the given shared `log`
`crates/shell/src/panels/shields_panel.rs:159` **fn** `toggle` — Flip panel visibility
`crates/shell/src/panels/shields_panel.rs:164` **fn** `set_domain` — Update `current_domain` and refresh blocked counts
`crates/shell/src/panels/shields_panel.rs:171` **fn** `refresh` — Pull the latest counts from the shared [`BlockedLog`] into the panel
`crates/shell/src/panels/shields_panel.rs:183` **fn** `clear_log` — Clear the shared blocked log (call on top-level navigation)
`crates/shell/src/panels/shields_panel.rs:192` **fn** `blocked_domain_count` — Blocked-request count for the current domain (from last `refresh`)
`crates/shell/src/panels/shields_panel.rs:197` **fn** `blocked_total_count` — Total blocked-request count for the current page (from last `refresh`)
`crates/shell/src/panels/shields_panel.rs:206` **enum** `ShieldsHit` — Result of a click inside the shields panel
`crates/shell/src/panels/shields_panel.rs:219` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the shields panel
`crates/shell/src/panels/shields_panel.rs:254` **fn** `build_panel` — Build the display list for the shields floating panel
`crates/shell/src/panels/shortcuts_panel.rs:47` **struct** `ShortcutRow` — One entry in the shortcuts list: human label + current binding
`crates/shell/src/panels/shortcuts_panel.rs:60` **fn** `binding_label` — Formatted binding string shown in the key badge (e.g. `"Ctrl+R"`)
`crates/shell/src/panels/shortcuts_panel.rs:76` **fn** `default_rows` — Compile-time default bindings for all displayed commands
`crates/shell/src/panels/shortcuts_panel.rs:125` **enum** `ShortcutsHit` — Hit result from `hit_test`
`crates/shell/src/panels/shortcuts_panel.rs:136` **struct** `ShortcutsPanel` — Keyboard shortcuts panel UI state
`crates/shell/src/panels/shortcuts_panel.rs:152` **fn** `new` — Create a new, hidden panel using compile-time default bindings
`crates/shell/src/panels/shortcuts_panel.rs:164` **fn** `open` — Show the panel
`crates/shell/src/panels/shortcuts_panel.rs:170` **fn** `toggle` — Toggle visibility
`crates/shell/src/panels/shortcuts_panel.rs:175` **fn** `close` — Hide the panel and cancel any pending rebind
`crates/shell/src/panels/shortcuts_panel.rs:181` **fn** `scroll_by` — Scroll the content area by `delta` px (clamped to valid range)
`crates/shell/src/panels/shortcuts_panel.rs:190` **fn** `accept_rebind` — Called when a rebind keypress arrives
`crates/shell/src/panels/shortcuts_panel.rs:206` **fn** `cancel_rebind` — Cancel the current rebind without changing the binding
`crates/shell/src/panels/shortcuts_panel.rs:211` **fn** `hit_test` — Hit-test a click at `(cx, cy)` in panel-local coordinates
`crates/shell/src/panels/shortcuts_panel.rs:231` **fn** `build_panel` — Render the panel into `dl`, anchored at `(ox, oy)` in screen space
`crates/shell/src/panels/sidebar_panel.rs:59` **struct** `SidebarPanel` — Right-docked sidebar web panel state (7D.3)
`crates/shell/src/panels/sidebar_panel.rs:78` **fn** `new` — Create a new hidden sidebar panel with no page loaded
`crates/shell/src/panels/sidebar_panel.rs:91` **fn** `toggle` — Toggle panel visibility.  No-op when no URL has been set
`crates/shell/src/panels/sidebar_panel.rs:101` **fn** `open` — Open the sidebar with `url`.  Clears content if the URL changed
`crates/shell/src/panels/sidebar_panel.rs:114` **fn** `close` — Close the sidebar (hide; URL and content are preserved for re-open)
`crates/shell/src/panels/sidebar_panel.rs:121` **fn** `set_page` — Store a freshly-rendered display list for the sidebar page
`crates/shell/src/panels/sidebar_panel.rs:133` **fn** `update_page` — Replace the page display list after a width reflow (F2-6 drag-resize)
`crates/shell/src/panels/sidebar_panel.rs:141` **fn** `max_scroll` — Maximum valid `scroll_y` (0 if content fits in viewport)
`crates/shell/src/panels/sidebar_panel.rs:157` **enum** `SidebarHit` — Result of a click inside the sidebar panel
`crates/shell/src/panels/sidebar_panel.rs:170` **fn** `hit_test` — Hit-test `(x, y)` in CSS px against the sidebar panel
`crates/shell/src/panels/sidebar_panel.rs:212` **fn** `build_panel` — Build the display list for the docked sidebar panel
`crates/shell/src/panels/sleep_hint.rs:26` **fn** `build_sleep_hint` — Build the sleep-restore hint overlay if restore has taken longer than THRESHOLD_MS
`crates/shell/src/panels/split_view.rs:22` **enum** `SplitFocus` — Which pane receives keyboard and scroll input
`crates/shell/src/panels/split_view.rs:36` **struct** `SplitPane` — Frozen rendering state for the right pane in a split view
`crates/shell/src/panels/split_view.rs:56` **struct** `SplitView` — Active split-view state: two side-by-side `ContentViewport` slots
`crates/shell/src/panels/split_view.rs:65` **fn** `new` — Open split view: right pane shows the given tab's last rendered state
`crates/shell/src/panels/split_view.rs:99` **fn** `build_combined_dl` — Build a combined display list for split-view rendering
`crates/shell/src/panels/split_view.rs:155` **fn** `cursor_in_right` — Return `true` if `window_x` (CSS px) falls inside the right pane
`crates/shell/src/panels/split_view.rs:161` **fn** `right_content_x` — Map a window-space x coord to right-pane content x (accounts for scroll)
`crates/shell/src/panels/split_view.rs:167` **fn** `right_content_y` — Map a window-space y coord to right-pane content y (accounts for scroll)
`crates/shell/src/panels/split_view.rs:172` **fn** `toggle_focus` — Toggle keyboard/scroll focus between left and right pane
`crates/shell/src/panels/split_view.rs:180` **fn** `focus_at` — Transfer focus to whichever pane contains `window_x`
`crates/shell/src/panels/split_view.rs:190` **fn** `scroll_focused_by` — Scroll the focused pane by `dy` CSS px (clamped to content bounds)
`crates/shell/src/panels/themes.rs:12` **enum** `AccentPreset` — Preset accent colours available in the Appearance settings section
`crates/shell/src/panels/themes.rs:40` **fn** `color` — RGB colour for this preset
`crates/shell/src/panels/themes.rs:52` **fn** `key` — Short lowercase key, used in settings serialisation
`crates/shell/src/panels/themes.rs:64` **fn** `from_key` — Parse from the short key.  Unknown key falls back to `Blue`
`crates/shell/src/panels/themes.rs:78` **enum** `ThemeBase` — Base brightness mode for the shell chrome
`crates/shell/src/panels/themes.rs:93` **struct** `ShellTheme` — Shell appearance configuration: base brightness + accent colour
`crates/shell/src/panels/themes.rs:102` **fn** `accent_color` — Accent colour for the active tab indicator and other chrome highlights
`crates/shell/src/panels/themes.rs:110` **fn** `is_dark` — Whether the chrome should use the dark palette
`crates/shell/src/panels/themes.rs:119` **fn** `parse` — Parse from the compact settings string (e.g. `"dark"`, `"light+rose"`)
`crates/shell/src/panels/themes.rs:133` **fn** `to_settings_str` — Serialise to the compact settings string
`crates/shell/src/panels/themes.rs:152` **fn** `palette` — Resolve the concrete chrome [`Palette`] for this theme
`crates/shell/src/panels/themes.rs:169` **struct** `Palette` — Resolved chrome colour tokens for the shell UI (tab strip, address bar,
`crates/shell/src/panels/tree_tabs.rs:82` **struct** `TreeTabsPanel` — Tree-style tabs panel state
`crates/shell/src/panels/tree_tabs.rs:91` **fn** `new` — Create a new hidden panel with no collapsed subtrees
`crates/shell/src/panels/tree_tabs.rs:96` **fn** `toggle` — Flip visibility. Caller must trigger relayout + redraw
`crates/shell/src/panels/tree_tabs.rs:105` **fn** `toggle_collapsed` — Toggle the collapsed state of the subtree rooted at `tab_id`
`crates/shell/src/panels/tree_tabs.rs:124` **enum** `TreeTabHit` — Result of a click inside the tree tabs panel
`crates/shell/src/panels/tree_tabs.rs:139` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the tree tabs panel
`crates/shell/src/panels/tree_tabs.rs:182` **fn** `build_panel` — Build the display list for the tree-style tabs panel
`crates/shell/src/panels/vertical_tabs.rs:52` **struct** `VerticalTabsPanel` — Vertical tabs panel: list of open tabs rendered as a left-docked sidebar
`crates/shell/src/panels/vertical_tabs.rs:64` **fn** `new` — Create a new (hidden) panel
`crates/shell/src/panels/vertical_tabs.rs:69` **fn** `toggle` — Flip visibility. Caller must trigger relayout + redraw
`crates/shell/src/panels/vertical_tabs.rs:77` **fn** `scroll_by` — Scroll the panel by `delta` CSS px (positive = down)
`crates/shell/src/panels/vertical_tabs.rs:93` **enum** `VTabHit` — Result of a click inside the vertical tab panel area
`crates/shell/src/panels/vertical_tabs.rs:108` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the vertical tabs panel
`crates/shell/src/panels/vertical_tabs.rs:143` **fn** `build_tab_bar_vertical` — Build the display list for the vertical tabs panel with scroll support
`crates/shell/src/panels/workspace_panel.rs:65` **struct** `WsEntry` — Lightweight workspace entry used for panel rendering (loaded from storage on
`crates/shell/src/panels/workspace_panel.rs:78` **struct** `WorkspacePanel` — Workspace switcher panel state
`crates/shell/src/panels/workspace_panel.rs:90` **fn** `new` — Create a new (hidden) panel with an empty workspace list
`crates/shell/src/panels/workspace_panel.rs:100` **fn** `toggle` — Flip visibility.  Caller must trigger redraw (and relayout if changing
`crates/shell/src/panels/workspace_panel.rs:105` **fn** `set_workspaces` — Replace the cached workspace list (call after any storage mutation)
`crates/shell/src/panels/workspace_panel.rs:110` **fn** `set_active` — Mark `id` as the active workspace
`crates/shell/src/panels/workspace_panel.rs:125` **enum** `WorkspaceHit` — Result of a click inside the workspace switcher bar
`crates/shell/src/panels/workspace_panel.rs:140` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the workspace switcher bar
`crates/shell/src/panels/workspace_panel.rs:199` **fn** `build_panel` — Build the display list for the workspace switcher bar
`crates/shell/src/panels/workspace_panel.rs:335` **fn** `parse_ws_color` — Convert a stored CSS colour string (`#RRGGBB`, `#RGB`, or named colour
`crates/shell/src/platform/audio_capture.rs:40` **struct** `PlatformAudioCapture` — Platform audio capture provider (WASAPI / ALSA via `cpal`)
`crates/shell/src/platform/audio_player.rs:98` **struct** `PlatformAudioPlayer` — Shell-side implementation of `AudioPlaybackProvider` using `rodio`
`crates/shell/src/platform/audio_player.rs:105` **fn** `new` — Create a new player (no OS resources allocated until the first handle)
`crates/shell/src/platform/clipboard.rs:24` **struct** `PlatformClipboard` — Reads and writes the host platform clipboard for `navigator.clipboard`
`crates/shell/src/platform/dark_mode.rs:20` **fn** `theme_prefers_dark` — Maps an OS colour-scheme [`Theme`] to the `prefers-color-scheme: dark`
`crates/shell/src/platform/display_color_profile.rs:89` **struct** `PlatformDisplayColorProfile` — Windows display-color-profile provider via GDI `GetICMProfile`
`crates/shell/src/platform/display_color_profile.rs:94` **fn** `new`
`crates/shell/src/platform/file_dialog.rs:14` **struct** `FilePickerEntry`
`crates/shell/src/platform/file_dialog.rs:34` **fn** `open_file_dialog` — Open the OS file-picker dialog and return selected files
`crates/shell/src/platform/file_dialog.rs:52` **fn** `entries_to_json_with_tokens` — Build a JSON array that includes opaque `token` values instead of raw paths
`crates/shell/src/platform/screen_capture.rs:114` **struct** `PlatformScreenCapture` — Platform screen capture provider using Win32 GDI BitBlt
`crates/shell/src/platform/wake_lock.rs:25` **struct** `PlatformWakeLock` — Platform-backed wake-lock provider
`crates/shell/src/platform/wake_lock.rs:32` **fn** `new` — Create a new provider with no lock held initially
`crates/shell/src/prefetch.rs:57` **struct** `PrefetchCache` — Shared, generation-scoped byte cache for page subresources. See module docs
`crates/shell/src/prefetch.rs:71` **fn** `reset` — Drop all cached entries and adopt navigation `generation`
`crates/shell/src/prefetch.rs:78` **fn** `current_generation` — The navigation generation the cache is currently scoped to
`crates/shell/src/prefetch.rs:93` **fn** `fetch` — Fetch `url` through the cache for navigation `generation`
`crates/shell/src/prefetch.rs:138` **fn** `fetch_current` — Convenience for the UI-thread consumer (`parse_and_layout`): fetch using the
`crates/shell/src/reader_view.rs:18` **struct** `ArticleContent` — Article content extracted from a raw HTML page
`crates/shell/src/reader_view.rs:37` **fn** `extract_article` — Parse `html` and extract the main article content
`crates/shell/src/reader_view.rs:52` **fn** `build_reader_html` — Wrap an [`ArticleContent`] in the reader template and return a
`crates/shell/src/runtime.rs:39` **enum** `TaskSource` — Источник task-а — HTML §8.1.4.3 «Task sources». Каждому источнику —
`crates/shell/src/runtime.rs:91` **struct** `Task` — Task — отложенное действие, выполняемое за пределами текущего call-stack-а
`crates/shell/src/runtime.rs:97` **fn** `new`
`crates/shell/src/runtime.rs:104` **fn** `source`
`crates/shell/src/runtime.rs:108` **fn** `run`
`crates/shell/src/runtime.rs:122` **struct** `TaskQueue` — Per-source очереди task-ов. Каждый `TaskSource` — отдельная FIFO,
`crates/shell/src/runtime.rs:141` **fn** `new`
`crates/shell/src/runtime.rs:145` **fn** `queue`
`crates/shell/src/runtime.rs:153` **fn** `pop` — Достать task с highest-priority непустой очереди (по
`crates/shell/src/runtime.rs:164` **fn** `len`
`crates/shell/src/runtime.rs:168` **fn** `is_empty`
`crates/shell/src/runtime.rs:174` **fn** `len_of` — Длина очереди конкретного источника — для тестов и метрик
`crates/shell/src/runtime.rs:183` **struct** `Microtask` — Microtask — действие, выполняемое в microtask checkpoint после каждой
`crates/shell/src/runtime.rs:188` **fn** `new`
`crates/shell/src/runtime.rs:194` **fn** `run`
`crates/shell/src/runtime.rs:200` **struct** `MicrotaskQueue`
`crates/shell/src/runtime.rs:205` **fn** `new`
`crates/shell/src/runtime.rs:209` **fn** `queue`
`crates/shell/src/runtime.rs:213` **fn** `pop`
`crates/shell/src/runtime.rs:217` **fn** `len`
`crates/shell/src/runtime.rs:221` **fn** `is_empty`
`crates/shell/src/runtime.rs:229` **type** `AnimationFrameHandle` — Уникальный идентификатор rAF-callback-а, возвращается `request_animation_frame`
`crates/shell/src/runtime.rs:237` **enum** `ObserverKind` — Тип наблюдателя — определяет, в какой стадии rendering steps его callback
`crates/shell/src/runtime.rs:245` **type** `ObserverHandle` — Уникальный handle наблюдателя. `disconnect_observer` снимает регистрацию
`crates/shell/src/runtime.rs:267` **type** `IdleCallbackHandle` — Уникальный идентификатор idle-callback-а — возвращается
`crates/shell/src/runtime.rs:281` **struct** `IdleDeadline` — Аргумент idle-callback-а (W3C `requestIdleCallback` §3 `IdleDeadline`)
`crates/shell/src/runtime.rs:289` **fn** `time_remaining` — Сколько миллисекунд осталось до конца текущего idle-окна. Отрицательные
`crates/shell/src/runtime.rs:300` **fn** `did_timeout` — Был ли callback вызван из-за timeout-параметра запроса (а не реального
`crates/shell/src/runtime.rs:339` **enum** `StepResult` — Результат одной итерации `step()`: запустилась ли task
`crates/shell/src/runtime.rs:349` **struct** `EventLoop` — HTML event loop. Реализует §8.1.4.2 «Processing model» в минимально полезном
`crates/shell/src/runtime.rs:360` **fn** `new`
`crates/shell/src/runtime.rs:368` **fn** `handle` — Дешёвая клон-копия handle-а для постановки task-ов извне и изнутри
`crates/shell/src/runtime.rs:381` **fn** `step` — Один step event-loop-а:
`crates/shell/src/runtime.rs:396` **fn** `perform_microtask_checkpoint` — HTML §8.1.4.4 «Microtask checkpoint». Drain-all: вновь поставленный
`crates/shell/src/runtime.rs:418` **fn** `run_rendering_step` — Rendering opportunity stage — HTML §8.1.5.1 «Run the animation frame
`crates/shell/src/runtime.rs:435` **fn** `pending_tasks` — Сколько task-ов сейчас в очереди (для тестов / отладки)
`crates/shell/src/runtime.rs:440` **fn** `pending_microtasks` — Сколько microtask-ов сейчас в очереди (для тестов / отладки)
`crates/shell/src/runtime.rs:446` **fn** `pending_animation_frames` — Сколько rAF-callback-ов сейчас ждёт следующего rendering step
`crates/shell/src/runtime.rs:452` **fn** `pending_idle_callbacks` — Сколько idle-callback-ов сейчас ждёт следующего `run_idle_callbacks`
`crates/shell/src/runtime.rs:474` **fn** `run_idle_callbacks` — W3C `requestIdleCallback` §3 — выполнить ожидающие idle-callback-и
`crates/shell/src/runtime.rs:496` **fn** `active_observers` — Сколько активных наблюдателей указанного типа (для тестов / отладки)
`crates/shell/src/runtime.rs:514` **fn** `deliver_observer_records` — Доставить records всем активным наблюдателям указанного типа
`crates/shell/src/runtime.rs:532` **struct** `EventLoopHandle` — Дёшево клонируемая ссылка на event loop. Closure-ы task-ов / microtask-ов
`crates/shell/src/runtime.rs:537` **fn** `queue_task`
`crates/shell/src/runtime.rs:544` **fn** `queue_microtask`
`crates/shell/src/runtime.rs:553` **fn** `request_animation_frame` — Зарегистрировать rAF-callback. Будет вызван на ближайшем
`crates/shell/src/runtime.rs:572` **fn** `cancel_animation_frame` — Отменить rAF до выполнения. Если handle уже выполнен или неизвестен —
`crates/shell/src/runtime.rs:587` **fn** `request_idle_callback` — Зарегистрировать idle-callback (W3C `requestIdleCallback` §3). Будет
`crates/shell/src/runtime.rs:607` **fn** `cancel_idle_callback` — Отменить idle-callback до выполнения. Неизвестный или уже выполненный
`crates/shell/src/runtime.rs:613` **fn** `register_observer` — Зарегистрировать observer выбранного типа. Callback-ы вызываются при
`crates/shell/src/runtime.rs:630` **fn** `disconnect_observer` — Снять регистрацию наблюдателя. Неизвестный handle — no-op
`crates/shell/src/scroll/decode_gating.rs:22` **fn** `discard_offscreen_images` — Drop CPU-decoded images for all `BoxKind::Image` boxes that are NOT in the
`crates/shell/src/scroll_anim.rs:23` **struct** `ScrollAnim` — Снапшот анимации scroll_y. Хранится в `Lumen.scroll_anim`. Pure-данные —
`crates/shell/src/scroll_anim.rs:36` **fn** `target` — Целевая точка анимации — для аддитивных вызовов
`crates/shell/src/scroll_anim.rs:49` **fn** `sample` — Posizione в момент `now_ms` (CSS px) и флаг завершения
`crates/shell/src/scroll_anim.rs:66` **fn** `ease_out_cubic` — Out-cubic easing: `f(t) = 1 - (1-t)^3`. `f(0)=0`, `f(1)=1`. Параметр
`crates/shell/src/scrollbar.rs:57` **fn** `build_scrollbar_overlay` — Собрать display-command-ы scrollbar-а для подмешивания в overlay
`crates/shell/src/scrollbar.rs:97` **fn** `thumb_geometry` — Pure-fn геометрия thumb-а — `(top, height)` в координатах overlay
`crates/shell/src/scrollbar.rs:119` **enum** `TrackClick` — Результат классификации точки клика по scrollbar-у. `Thumb` — стартуем
`crates/shell/src/scrollbar.rs:132` **fn** `classify_track_click` — Куда попал клик в scrollbar-track: вне / в thumb / выше thumb / ниже thumb
`crates/shell/src/scrollbar.rs:185` **struct** `ScrollDrag` — Снапшот состояния на момент начала drag-а: scroll_y страницы и cursor_y
`crates/shell/src/scrollbar.rs:191` **fn** `new`
`crates/shell/src/scrollbar.rs:199` **fn** `scroll_for` — Желаемый `scroll_y` при текущей позиции курсора. Если scrollbar
`crates/shell/src/session_persist.rs:31` **fn** `open_store` — Open the session store at [`SESSION_DB_PATH`], falling back to an in-memory
`crates/shell/src/session_persist.rs:43` **fn** `active_index` — Index of the tab to make active after restore: the first `is_active` tab, or
`crates/shell/src/source_view.rs:15` **fn** `build_view_source_html` — Wrap `raw` HTML source in a syntax-highlighted page
`crates/shell/src/spellcheck.rs:22` **fn** `spell_data_dir` — Папка с пользовательскими словарями: `<exe_dir>/data/spell`
`crates/shell/src/spellcheck.rs:29` **struct** `MultiDictionary` — Комбинированный словарь нескольких локалей. Слово считается верным,
`crates/shell/src/spellcheck.rs:36` **fn** `empty` — Создаёт пустой набор словарей (спелл-чек отключён)
`crates/shell/src/spellcheck.rs:44` **fn** `is_empty` — Проверяет, загружен ли хотя бы один словарь
`crates/shell/src/spellcheck.rs:113` **fn** `load_dictionaries` — Загружает все пары `<stem>.aff` + `<stem>.dic` из `dir`
`crates/shell/src/spellcheck.rs:168` **fn** `extract_words` — Извлекает байтовые диапазоны слов в `text`
`crates/shell/src/spellcheck.rs:210` **fn** `misspelled_ranges_with` — Возвращает диапазоны слов, для которых `checker.check` вернул `false`, при
`crates/shell/src/spellcheck.rs:228` **fn** `word_at_x` — Находит байтовый диапазон слова в `text`, чья горизонтальная проекция
`crates/shell/src/spellcheck.rs:240` **fn** `user_words_path` — Путь к пользовательскому словарю: `<exe_dir>/data/spell/user_words.txt`
`crates/shell/src/spellcheck.rs:246` **fn** `load_user_words` — Загружает пользовательский словарь: по одному слову в строке, lowercase
`crates/shell/src/spellcheck.rs:259` **fn** `add_user_word` — Добавляет слово (lowercase) в файл пользовательского словаря, дописывая
`crates/shell/src/spellcheck.rs:270` **fn** `build_spell_overlay` — Строит команды отрисовки волнистого подчёркивания для ошибочных диапазонов
`crates/shell/src/surface/ctx.rs:22` **struct** `PaintCtx` — Read-only context for [`super::Panel::paint`]
`crates/shell/src/surface/ctx.rs:39` **fn** `new` — Build a paint context with default (non-focused, non-hovered) hints
`crates/shell/src/surface/ctx.rs:56` **struct** `EventCtx` — Side effects a panel may request while handling an event
`crates/shell/src/surface/ctx.rs:71` **fn** `new` — A fresh context with no pending effects
`crates/shell/src/surface/ctx.rs:76` **fn** `dispatch` — Queue a command to be applied after `on_event` returns
`crates/shell/src/surface/ctx.rs:81` **fn** `request_repaint` — Mark this panel dirty so it repaints on the next frame
`crates/shell/src/surface/ctx.rs:86` **fn** `set_cursor` — Ask the shell to show `cursor` while over this panel
`crates/shell/src/surface/ctx.rs:91` **fn** `request_focus` — Ask to capture keyboard focus
`crates/shell/src/surface/ctx.rs:96` **fn** `release_focus` — Ask to release keyboard focus
`crates/shell/src/surface/ctx.rs:101` **fn** `start_drag` — Ask the manager to begin dragging this panel (window-local `grab_offset`)
`crates/shell/src/surface/ctx.rs:108` **fn** `commands` — Commands queued during this event, in dispatch order
`crates/shell/src/surface/ctx.rs:113` **fn** `take_commands` — Take ownership of the queued commands, leaving the context empty
`crates/shell/src/surface/ctx.rs:118` **fn** `wants_repaint` — Whether the panel requested a repaint
`crates/shell/src/surface/ctx.rs:123` **fn** `requested_cursor` — The cursor the panel requested, if any
`crates/shell/src/surface/ctx.rs:129` **fn** `requested_focus_change` — The focus change the panel requested: `Some(true)` to capture focus,
`crates/shell/src/surface/ctx.rs:134` **fn** `requested_drag` — The drag the panel requested to start, if any
`crates/shell/src/surface/manager.rs:61` **struct** `SlotRect` — Resolved window-space rect for a named docked slot
`crates/shell/src/surface/manager.rs:69` **struct** `LayoutNode` — Informational snapshot of one slot in the docked layout tree
`crates/shell/src/surface/manager.rs:95` **struct** `SurfaceManager` — Single coordinator for all shell UI panels (ADR-009 §SurfaceManager)
`crates/shell/src/surface/manager.rs:126` **fn** `new` — Create an empty manager sized to `(width, height)` CSS px
`crates/shell/src/surface/manager.rs:141` **fn** `register` — Register a panel.  Its rect is computed immediately; `on_mount` is called
`crates/shell/src/surface/manager.rs:154` **fn** `composite` — Composite all visible panels into one `DisplayList` for the renderer
`crates/shell/src/surface/manager.rs:189` **fn** `slot_rect` — Resolved rect for a named docked slot, or `None` if not present
`crates/shell/src/surface/manager.rs:196` **fn** `layout_snapshot` — Snapshot of the docked layout tree (diagnostic / test helper)
`crates/shell/src/surface/manager.rs:211` **fn** `on_resize` — Notify that the window was resized.  All panel rects are recomputed and
`crates/shell/src/surface/manager.rs:227` **fn** `set_visible` — Show or hide a panel by id.  Triggers layout recomputation
`crates/shell/src/surface/manager.rs:236` **fn** `set_theme` — Set the active `Theme` for all subsequent `paint()` calls
`crates/shell/src/surface/manager.rs:241` **fn** `theme` — Active theme
`crates/shell/src/surface/manager.rs:246` **fn** `has_panel` — Whether a panel with `id` is registered
`crates/shell/src/surface/manager.rs:251` **fn** `panel_count` — Number of registered panels
`crates/shell/src/surface/manager.rs:256` **fn** `window_size` — Current window size (CSS px)
`crates/shell/src/surface/manager.rs:261` **fn** `panel_rect` — Rect of a registered panel, or `None` if not found / hidden
`crates/shell/src/surface/manager.rs:275` **fn** `route_mouse_move` — Route a mouse-move event and return the combined response
`crates/shell/src/surface/manager.rs:291` **fn** `route_mouse_down` — Route a mouse-down event
`crates/shell/src/surface/manager.rs:303` **fn** `route_mouse_up` — Route a mouse-up event
`crates/shell/src/surface/manager.rs:316` **fn** `route_click` — Route a click (press + release in the same panel)
`crates/shell/src/surface/manager.rs:321` **fn** `route_scroll` — Route a scroll event
`crates/shell/src/surface/manager.rs:331` **fn** `move_panel_to_slot` — Override the slot a panel is docked into and recompute the layout
`crates/shell/src/surface/manager.rs:348` **fn** `set_slot_size` — Set a per-slot size override (px) and recompute the layout
`crates/shell/src/surface/manager.rs:357` **fn** `panel_slot` — Effective docked slot of the panel with `id`, or `None` if not docked
`crates/shell/src/surface/manager.rs:366` **fn** `is_dragging` — `true` while a panel is being dragged to a new slot
`crates/shell/src/surface/manager.rs:372` **fn** `drop_target_rect` — Rect of the slot currently hovered as the drop target, for an insertion
`crates/shell/src/surface/manager.rs:380` **fn** `begin_drag` — Begin dragging `panel_id`, grabbed at panel-local `grab_offset`, with the
`crates/shell/src/surface/manager.rs:390` **fn** `cancel_drag` — Abort any in-progress drag without redocking
`crates/shell/src/surface/manager.rs:402` **fn** `serialize_layout` — Serialise the current panel layout to a compact, forward-compatible
`crates/shell/src/surface/manager.rs:423` **fn** `apply_layout` — Apply a layout previously produced by [`Self::serialize_layout`]
`crates/shell/src/surface/mod.rs:48` **trait** `Panel` — A self-contained shell UI block
`crates/shell/src/surface/theme.rs:21` **struct** `Theme` — All design tokens for one shell appearance
`crates/shell/src/surface/theme.rs:90` **fn** `sand_indigo` — V1 / default: warm sand + indigo (light)
`crates/shell/src/surface/theme.rs:121` **fn** `graphite_amber` — V2 / dark: graphite + amber
`crates/shell/src/surface/theme.rs:152` **fn** `for_dark_mode` — Pick a built-in theme by OS dark-mode preference
`crates/shell/src/surface/types.rs:28` **enum** `Surface` — Where and how a panel appears on screen
`crates/shell/src/surface/types.rs:73` **fn** `is_docked` — `true` for [`Surface::Docked`]
`crates/shell/src/surface/types.rs:78` **fn** `is_overlay` — `true` for floats and modals (anything on the overlay layer)
`crates/shell/src/surface/types.rs:85` **enum** `Corner` — Window corner, used by [`FloatAnchor::Corner`]
`crates/shell/src/surface/types.rs:98` **enum** `FloatAnchor` — Where a [`Surface::Float`] panel is positioned
`crates/shell/src/surface/types.rs:117` **enum** `SizeRule` — How a panel (or slot) describes its desired extent along one axis
`crates/shell/src/surface/types.rs:136` **fn** `resolve` — Resolve a concrete length against the `available` space along the axis
`crates/shell/src/surface/types.rs:146` **fn** `is_flex` — `true` if this rule expands to fill leftover space
`crates/shell/src/surface/types.rs:155` **enum** `MouseButton` — Mouse button identity
`crates/shell/src/surface/types.rs:163` **struct** `ScrollDelta` — Scroll wheel / trackpad delta in CSS px
`crates/shell/src/surface/types.rs:175` **enum** `PanelEvent` — An event delivered to a panel via [`super::Panel::on_event`]
`crates/shell/src/surface/types.rs:218` **struct** `DragData` — State carried while a panel is being dragged from its dock slot
`crates/shell/src/surface/types.rs:229` **fn** `new` — Build drag state for `source_panel` grabbed at `grab_offset` (window-local
`crates/shell/src/surface/types.rs:238` **enum** `EventResponse` — What a panel returns from [`super::Panel::on_event`]
`crates/shell/src/surface/types.rs:258` **enum** `Command` — State-changing intents a panel can emit
`crates/shell/src/surface/types.rs:290` **enum** `CursorIcon` — Mouse cursor shape requested for a hit target
`crates/shell/src/surface/types.rs:302` **enum** `HitElement` — Semantic identity of the element under the cursor
`crates/shell/src/surface/types.rs:327` **struct** `HitTarget` — Result of [`super::Panel::hit_test`]: what is under a point and how the shell
`crates/shell/src/surface/types.rs:340` **fn** `new` — A minimal hit target for `element` with a default cursor and no tooltip
`crates/shell/src/surface/types.rs:366` **fn** `rect_contains` — `true` if `rect` contains `p` (left/top inclusive, right/bottom exclusive)
`crates/shell/src/tab_lifecycle/manager.rs:14` **type** `TabId` — Opaque tab identifier. Callers create sequential IDs (0, 1, 2, …) or any u64
`crates/shell/src/tab_lifecycle/manager.rs:18` **struct** `TierTransition` — A tier transition that occurred during `tick_idle` or `lru_evict`
`crates/shell/src/tab_lifecycle/manager.rs:35` **struct** `TabLifecycleManager` — Manages lifecycle state for all open tabs
`crates/shell/src/tab_lifecycle/manager.rs:54` **fn** `new` — Create a new manager with the given timeouts and LRU budget
`crates/shell/src/tab_lifecycle/manager.rs:68` **fn** `open_tab` — Open a new tab. The tab starts in Active state and becomes the foreground tab
`crates/shell/src/tab_lifecycle/manager.rs:91` **fn** `activate_tab` — Switch to an existing tab, activating it and sending the previous active tab
`crates/shell/src/tab_lifecycle/manager.rs:136` **fn** `close_tab` — Mark a tab as closed. Advances it to `TabState::Closed` and removes it
`crates/shell/src/tab_lifecycle/manager.rs:157` **fn** `set_pinned` — Pin/unpin a tab. Pinned tabs are never evicted past T1
`crates/shell/src/tab_lifecycle/manager.rs:164` **fn** `tab_state` — Returns the current state of a tab, or `None` if the tab is unknown
`crates/shell/src/tab_lifecycle/manager.rs:169` **fn** `is_active` — Returns `true` if `id` is the foreground (Active) tab
`crates/shell/src/tab_lifecycle/manager.rs:177` **fn** `tick_idle` — Advance all background tabs whose idle timeout has elapsed, and apply
`crates/shell/src/tab_lifecycle/manager.rs:227` **fn** `lru_evict` — Evict least-recently-used background tabs until the number of
`crates/shell/src/tab_lifecycle/manager.rs:283` **fn** `snapshot` — Returns a snapshot of all tab IDs and their current states
`crates/shell/src/tab_lifecycle/restore.rs:22` **struct** `TabMetadata` — Lightweight per-tab identity kept in RAM while a tab is hibernated (T3)
`crates/shell/src/tab_lifecycle/sleep.rs:24` **fn** `serialize_form_state` — Serialise a `FormState` map to a compact JSON string
`crates/shell/src/tab_lifecycle/sleep.rs:47` **fn** `deserialize_form_state` — Deserialise a JSON string produced by [`serialize_form_state`] back into a `FormState`
`crates/shell/src/tab_lifecycle/state.rs:10` **enum** `TabState` — Tab lifecycle state (memory tier)
`crates/shell/src/tab_lifecycle/state.rs:34` **enum** `TransitionReason` — Reason for a lifecycle tier transition
`crates/shell/src/tab_lifecycle/state.rs:59` **struct** `TabLifecycle` — Per-tab lifecycle state tracking
`crates/shell/src/tab_lifecycle/state.rs:78` **struct** `TierTimeouts` — User-configurable timeouts for tier transitions
`crates/shell/src/tab_lifecycle/state.rs:101` **enum** `MemoryPressure` — OS memory pressure levels (mirrors `MemoryPressureLevel` from lumen-core)
`crates/shell/src/tab_lifecycle/state.rs:109` **fn** `new` — New tab starts in T0 Active
`crates/shell/src/tab_lifecycle/state.rs:120` **fn** `activate` — Transition to Active (T0), resetting idle counters
`crates/shell/src/tab_lifecycle/state.rs:129` **fn** `hide` — Record the moment the tab was hidden, starting the idle countdown
`crates/shell/src/tab_lifecycle/state.rs:136` **fn** `advance_tier` — Advance to the next tier. Returns `true` if a transition occurred
`crates/shell/src/tab_lifecycle/state.rs:150` **fn** `should_transition_on_idle` — Returns `true` if the idle timeout for the current tier has elapsed
`crates/shell/src/tab_lifecycle/state.rs:167` **fn** `suggested_pressure_state` — If memory pressure justifies an earlier-than-scheduled tier advance, returns
`crates/shell/src/tabs/archive.rs:58` **struct** `ArchivedTab` — A tab that was auto-archived and removed from the visible tab strip
`crates/shell/src/tabs/archive.rs:74` **enum** `ArchiveHit` — Hit result from the archive button or panel
`crates/shell/src/tabs/archive.rs:86` **struct** `TabArchive` — State of the tab archive system
`crates/shell/src/tabs/archive.rs:103` **fn** `new` — Create an empty archive with the panel closed
`crates/shell/src/tabs/archive.rs:108` **fn** `push` — Push a newly-archived tab (prepend — newest entry shown first)
`crates/shell/src/tabs/archive.rs:113` **fn** `take` — Remove and return the archived entry with the given original tab `id`
`crates/shell/src/tabs/archive.rs:119` **fn** `count` — Number of archived entries
`crates/shell/src/tabs/archive.rs:124` **fn** `toggle` — Toggle panel open/closed; resets scroll on open
`crates/shell/src/tabs/archive.rs:132` **fn** `close` — Close panel without clearing entries
`crates/shell/src/tabs/archive.rs:138` **fn** `scroll_up` — Scroll up by one row (clamped at zero)
`crates/shell/src/tabs/archive.rs:144` **fn** `scroll_down` — Scroll down by one row (clamped at last page)
`crates/shell/src/tabs/archive.rs:157` **fn** `archive_btn_x` — Pixel x-coordinate where the archive button begins (right of all tabs)
`crates/shell/src/tabs/archive.rs:177` **fn** `hit_test_button` — Hit-test the archive toolbar button area
`crates/shell/src/tabs/archive.rs:185` **fn** `hit_test_panel` — Hit-test the archive panel when it is open
`crates/shell/src/tabs/archive.rs:238` **fn** `build_button` — Build the archive toolbar button appended to the right of the tab bar
`crates/shell/src/tabs/archive.rs:317` **fn** `build_panel` — Build the drop-down archive panel anchored below the archive button
`crates/shell/src/tabs/containers.rs:44` **enum** `ContainerKind` — Kind of tab container. Drives the border-top colour in the tab strip
`crates/shell/src/tabs/containers.rs:65` **fn** `border_color` — Border-top strip colour, or `None` for [`ContainerKind::None`]
`crates/shell/src/tabs/containers.rs:82` **fn** `name` — Human-readable container name for UI labels
`crates/shell/src/tabs/containers.rs:112` **struct** `ContainerStore` — Origin+container → cookie/storage store id
`crates/shell/src/tabs/containers.rs:122` **fn** `new` — Create an empty store. First minted id will be `0`
`crates/shell/src/tabs/containers.rs:131` **fn** `get_or_create` — Get the store id for `(origin, container)`, allocating a fresh one
`crates/shell/src/tabs/containers.rs:144` **fn** `get` — Look up an existing store id without allocating
`crates/shell/src/tabs/containers.rs:150` **fn** `len` — Number of `(origin, container)` mappings tracked
`crates/shell/src/tabs/containers.rs:156` **fn** `is_empty` — `true` if no mapping has been allocated yet
`crates/shell/src/tabs/context_menu.rs:41` **fn** `menu_height` — Total menu height in CSS px (background box)
`crates/shell/src/tabs/context_menu.rs:49` **enum** `MenuAction` — An action the user can pick from the tab context menu
`crates/shell/src/tabs/context_menu.rs:112` **struct** `TabContextMenu` — State of the right-click tab context menu
`crates/shell/src/tabs/context_menu.rs:152` **fn** `open_for` — Open the menu for tab `idx` at cursor `(x, y)`. `pinned` is the target
`crates/shell/src/tabs/context_menu.rs:172` **fn** `close` — Hide the menu
`crates/shell/src/tabs/context_menu.rs:178` **fn** `is_open` — `true` while the menu is visible
`crates/shell/src/tabs/context_menu.rs:196` **fn** `item_at` — Map a CSS-px `(x, y)` to the menu row index under it, or `None` if the
`crates/shell/src/tabs/context_menu.rs:214` **fn** `action_at` — Map a CSS-px `(x, y)` to the [`MenuAction`] under it, or `None`
`crates/shell/src/tabs/context_menu.rs:224` **fn** `build_overlay` — Build a viewport-locked display list for the open menu
`crates/shell/src/tabs/groups.rs:24` **enum** `GroupColor` — One of the preset tab-group colours (Chrome-compatible palette)
`crates/shell/src/tabs/groups.rs:59` **fn** `color` — Fully-opaque RGB for the strip label and the per-tab accent bar
`crates/shell/src/tabs/groups.rs:74` **fn** `index` — Stable palette index (`0..8`), used as the persisted on-disk value
`crates/shell/src/tabs/groups.rs:81` **fn** `from_index` — Inverse of [`index`](GroupColor::index). Out-of-range indices clamp to
`crates/shell/src/tabs/groups.rs:99` **struct** `TabGroup` — A named, colour-coded group of tabs
`crates/shell/src/tabs/groups.rs:114` **fn** `new` — Create an expanded group with the given id, label and colour
`crates/shell/src/tabs/strip.rs:95` **struct** `TabEntry` — Metadata for one browser tab
`crates/shell/src/tabs/strip.rs:146` **struct** `TabStrip` — State of the tab strip (tab list + active index)
`crates/shell/src/tabs/strip.rs:161` **fn** `new` — Create the initial tab strip with one blank tab
`crates/shell/src/tabs/strip.rs:182` **fn** `len` — Number of open tabs
`crates/shell/src/tabs/strip.rs:190` **fn** `push_blank` — Append a new blank tab and return its index
`crates/shell/src/tabs/strip.rs:214` **fn** `push_with_opener` — Append a new blank child tab opened by the tab with `opener_id`
`crates/shell/src/tabs/strip.rs:235` **fn** `update_last_activated` — Record `now_ms` as the activation timestamp for the tab at `idx`
`crates/shell/src/tabs/strip.rs:247` **fn** `set_tab_container` — Assign `container` to the tab at `idx`. Out-of-bounds index is a no-op
`crates/shell/src/tabs/strip.rs:255` **fn** `remove` — Remove the tab at `idx`. Returns the new active index (clamped to valid
`crates/shell/src/tabs/strip.rs:267` **fn** `set_active_title` — Update the title of the active tab
`crates/shell/src/tabs/strip.rs:277` **fn** `set_tab_state` — Update the lifecycle state of the tab at `idx`
`crates/shell/src/tabs/strip.rs:287` **fn** `move_tab` — Reorder: move the tab currently at `src` so that it ends up at `dst`
`crates/shell/src/tabs/strip.rs:306` **fn** `toggle_pin` — Toggle the pinned flag of the tab at `idx`. Returns the new state
`crates/shell/src/tabs/strip.rs:316` **fn** `is_pinned` — `true` if the tab at `idx` is pinned. Out-of-bounds → `false`
`crates/shell/src/tabs/strip.rs:327` **fn** `duplicate` — Insert a duplicate of the tab at `src` immediately to its right
`crates/shell/src/tabs/strip.rs:355` **fn** `close_others` — Remove every tab except `keep_idx` and any pinned tabs
`crates/shell/src/tabs/strip.rs:380` **fn** `close_right` — Remove all non-pinned tabs positioned to the right of `idx`
`crates/shell/src/tabs/strip.rs:406` **fn** `create_group` — Create a new expanded [`TabGroup`] with `label` and `color`
`crates/shell/src/tabs/strip.rs:415` **fn** `group` — Borrow the group with the given id, if it exists
`crates/shell/src/tabs/strip.rs:421` **fn** `group_of` — The group id of the tab at `idx`, or `None` when ungrouped / out of bounds
`crates/shell/src/tabs/strip.rs:429` **fn** `assign_to_group` — Assign the tab at `idx` to the group `group_id`
`crates/shell/src/tabs/strip.rs:443` **fn** `ungroup` — Remove the tab at `idx` from its group (no-op if already ungrouped or
`crates/shell/src/tabs/strip.rs:451` **fn** `toggle_collapse` — Toggle the collapsed flag of the group `id`. Returns the new collapsed
`crates/shell/src/tabs/strip.rs:462` **fn** `is_collapsed` — `true` if the group `id` exists and is collapsed
`crates/shell/src/tabs/strip.rs:468` **fn** `group_color` — The colour of the group `id`, or `None` for an unknown group
`crates/shell/src/tabs/strip.rs:474` **fn** `group_members` — Strip indices of every tab in the group `id`, in left-to-right order
`crates/shell/src/tabs/strip.rs:485` **fn** `remove_group` — Remove the group `id` and ungroup all of its member tabs. No-op if the
`crates/shell/src/tabs/strip.rs:501` **fn** `visible_indices` — Strip indices of the tabs that should be drawn, in order
`crates/shell/src/tabs/strip.rs:525` **struct** `TabDragState` — State for an in-progress tab drag-and-drop
`crates/shell/src/tabs/strip.rs:539` **fn** `drop_target` — Compute the tab index where the dragged tab would be dropped if the
`crates/shell/src/tabs/strip.rs:551` **enum** `TabHit` — Result of clicking inside the tab bar area
`crates/shell/src/tabs/strip.rs:564` **enum** `TabLayout` — Tab layout mode: horizontal strip or vertical sidebar
`crates/shell/src/tabs/strip.rs:574` **fn** `from_str` — Parse from a stored settings string (`"horizontal"` or `"vertical"`)
`crates/shell/src/tabs/strip.rs:579` **fn** `as_str` — Serialize to a settings string
`crates/shell/src/tabs/strip.rs:592` **fn** `hit_test_layout_btn` — Returns `true` if `(x, y)` falls inside the layout-mode toggle button
`crates/shell/src/tabs/strip.rs:601` **fn** `build_layout_toggle_btn` — Build a display list for the vertical-tab layout toggle button
`crates/shell/src/tabs/strip.rs:646` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the tab bar
`crates/shell/src/tabs/strip.rs:688` **fn** `build_tab_bar` — Build a viewport-locked display list for the tab bar
`crates/shell/src/tabs/strip.rs:897` **fn** `build_tab_tooltip` — Build a small tooltip overlay for a tab with a non-Active tier badge
`crates/shell/src/tabs/tree.rs:22` **fn** `depth_of` — Compute the tree depth of the tab with `id` in the given slice
`crates/shell/src/tabs/tree.rs:38` **fn** `children_of` — Return the IDs of direct children of `parent_id` in strip order
`crates/shell/src/tabs/tree.rs:48` **fn** `subtree_ids` — Collect the IDs of all tabs in the subtree rooted at `root_id` (inclusive)
`crates/shell/src/tabs/tree.rs:63` **struct** `VisibleRow` — A row item produced by [`visible_order`]
`crates/shell/src/tabs/tree.rs:82` **fn** `visible_order` — Build the ordered list of visible tabs for tree-style rendering
`crates/shell/src/tracks.rs:24` **struct** `LoadedTrack` — Один `<track>` элемента `<video>`, отражённый в `TextTrack` JS-API
`crates/shell/src/tracks.rs:39` **struct** `PageTracks` — Загруженные cues по каждому `<video>` страницы
`crates/shell/src/tracks.rs:48` **fn** `is_empty` — Нет ни одного видео с загруженными cues
`crates/shell/src/tracks.rs:68` **fn** `load_video_tracks` — Обходит документ, для каждого `<video>` выбирает один `<track>` для оверлея,
`crates/shell/src/tracks.rs:116` **fn** `build_cue_overlay` — Строит оверлей активных cue. Время воспроизведения каждого видео
`crates/shell/src/tracks.rs:205` **fn** `collect_video_rects` — Рекурсивно собирает `(NodeId, Rect)` всех video-боксов layout-дерева
`crates/shell/src/zoom.rs:21` **fn** `zoom_in` — Increase zoom by one step, clamped to [`ZOOM_MAX`]
`crates/shell/src/zoom.rs:26` **fn** `zoom_out` — Decrease zoom by one step, clamped to [`ZOOM_MIN`]
`crates/shell/src/zoom.rs:31` **fn** `zoom_reset` — Reset zoom to 100%
`crates/shell/src/zoom.rs:40` **fn** `effective_viewport` — Compute the CSS layout viewport size from the physical window size

## lumen-storage  (511 symbols)

`crates/storage/src/a11y_prefs.rs:38` **enum** `CursorSize` — Accessibility cursor magnification level
`crates/storage/src/a11y_prefs.rs:50` **fn** `as_str` — Serialize to the storage string representation
`crates/storage/src/a11y_prefs.rs:59` **fn** `parse` — Parse from the storage string representation; unknown values → `Normal`
`crates/storage/src/a11y_prefs.rs:72` **struct** `A11yPrefsSnapshot` — All accessibility preferences as a copyable value type
`crates/storage/src/a11y_prefs.rs:105` **struct** `A11yPrefs` — Persistent accessibility preferences store
`crates/storage/src/a11y_prefs.rs:128` **fn** `open` — Open (or create) an on-disk accessibility preferences database
`crates/storage/src/a11y_prefs.rs:134` **fn** `open_in_memory` — Create an in-memory accessibility preferences database (for tests / ephemeral sessions)
`crates/storage/src/a11y_prefs.rs:184` **fn** `font_size_multiplier` — Font-size scale multiplier (e.g. 1.0, 1.25, 1.5)
`crates/storage/src/a11y_prefs.rs:189` **fn** `set_font_size_multiplier` — Set font-size scale multiplier
`crates/storage/src/a11y_prefs.rs:194` **fn** `reduced_motion` — Whether `prefers-reduced-motion` is active
`crates/storage/src/a11y_prefs.rs:199` **fn** `set_reduced_motion` — Set prefers-reduced-motion
`crates/storage/src/a11y_prefs.rs:204` **fn** `forced_colors` — Whether `prefers-forced-colors` is active
`crates/storage/src/a11y_prefs.rs:209` **fn** `set_forced_colors` — Set forced-colors preference
`crates/storage/src/a11y_prefs.rs:214` **fn** `cursor_size` — Cursor magnification level
`crates/storage/src/a11y_prefs.rs:219` **fn** `set_cursor_size` — Set cursor magnification level
`crates/storage/src/a11y_prefs.rs:224` **fn** `snapshot` — Read all preferences into a snapshot value
`crates/storage/src/a11y_prefs.rs:234` **fn** `apply_snapshot` — Persist all fields from a snapshot in one call
`crates/storage/src/adblock.rs:29` **struct** `Subscription` — A filter-list subscription the user follows
`crates/storage/src/adblock.rs:40` **struct** `ListMeta` — Cache metadata for one downloaded filter list
`crates/storage/src/adblock.rs:63` **struct** `AdblockStore` — SQLite-backed store for ad-block subscriptions and list cache metadata
`crates/storage/src/adblock.rs:75` **fn** `open` — Open (or create) the SQLite store at `path`, creating tables if needed
`crates/storage/src/adblock.rs:81` **fn** `open_in_memory` — Open an in-memory store (tests)
`crates/storage/src/adblock.rs:112` **fn** `list_subscriptions` — All subscriptions, ordered by title for stable display
`crates/storage/src/adblock.rs:134` **fn** `set_subscription` — Insert or update a subscription (keyed by URL)
`crates/storage/src/adblock.rs:150` **fn** `seed_defaults_if_empty` — Seed the given default subscriptions, but only when the table is empty
`crates/storage/src/adblock.rs:169` **fn** `get_meta` — Fetch cache metadata for a list slug, if present
`crates/storage/src/adblock.rs:193` **fn** `upsert_meta` — Insert or replace cache metadata for a list (keyed by slug)
`crates/storage/src/autofill.rs:17` **struct** `AutofillEntry`
`crates/storage/src/autofill.rs:25` **struct** `Autofill`
`crates/storage/src/autofill.rs:36` **fn** `open`
`crates/storage/src/autofill.rs:42` **fn** `open_in_memory`
`crates/storage/src/autofill.rs:75` **fn** `record` — Зафиксировать использование значения. Upsert: insert или
`crates/storage/src/autofill.rs:103` **fn** `suggestions` — Получить все сохранённые значения для (origin, field_name),
`crates/storage/src/autofill.rs:131` **fn** `best_for` — Самое популярное значение для поля
`crates/storage/src/autofill.rs:137` **fn** `delete` — Удалить конкретное значение
`crates/storage/src/autofill.rs:151` **fn** `clear_origin` — Удалить все autofill-данные для origin (clear-site-data)
`crates/storage/src/autofill.rs:165` **fn** `clear`
`crates/storage/src/autofill.rs:175` **fn** `count`
`crates/storage/src/bfcache.rs:24` **enum** `BfCachePayload` — Serialized page state for bfcache restoration
`crates/storage/src/bfcache.rs:41` **struct** `FrozenPage` — Fully frozen page state for bfcache restoration
`crates/storage/src/bfcache.rs:52` **struct** `BfCacheEntry` — Snapshot of a page suitable for bfcache restoration
`crates/storage/src/bfcache.rs:69` **struct** `BfCache` — In-memory LRU bfcache
`crates/storage/src/bfcache.rs:90` **fn** `new` — Create an empty cache with the given capacity
`crates/storage/src/bfcache.rs:103` **fn** `store` — Store or update an entry
`crates/storage/src/bfcache.rs:121` **fn** `retrieve` — Return a reference to the entry for `url`, or `None` if not cached
`crates/storage/src/bfcache.rs:126` **fn** `remove` — Remove the entry for `url` from the cache
`crates/storage/src/bfcache.rs:132` **fn** `len`
`crates/storage/src/bfcache.rs:136` **fn** `is_empty`
`crates/storage/src/bfcache.rs:140` **fn** `clear`
`crates/storage/src/bfcache.rs:146` **fn** `has_frozen` — Check whether a frozen page exists for the given URL
`crates/storage/src/bookmarks.rs:36` **struct** `Bookmark` — Одна закладка
`crates/storage/src/bookmarks.rs:46` **struct** `Bookmarks`
`crates/storage/src/bookmarks.rs:57` **fn** `open`
`crates/storage/src/bookmarks.rs:63` **fn** `open_in_memory`
`crates/storage/src/bookmarks.rs:103` **fn** `add` — Добавить или обновить закладку. Если url уже существует —
`crates/storage/src/bookmarks.rs:162` **fn** `get` — Получить закладку по url. None если нет
`crates/storage/src/bookmarks.rs:200` **fn** `delete` — Удалить закладку (вместе с тегами благодаря ON DELETE CASCADE)
`crates/storage/src/bookmarks.rs:214` **fn** `list_all` — Все закладки, отсортированные по папке (ASC), затем по created_at DESC
`crates/storage/src/bookmarks.rs:231` **fn** `set_folder` — Переместить закладку в другую папку (DnD reorder в UI-панели)
`crates/storage/src/bookmarks.rs:246` **fn** `list_by_folder` — Список закладок в данной папке (точное совпадение строки)
`crates/storage/src/bookmarks.rs:260` **fn** `list_by_tag` — Список закладок с данным тегом. Сортировка по created_at DESC
`crates/storage/src/bookmarks.rs:277` **fn** `all_tags` — Все уникальные теги в системе (для UI tag-cloud / autocomplete)
`crates/storage/src/bookmarks.rs:296` **fn** `all_folders` — Все уникальные папки
`crates/storage/src/bookmarks.rs:317` **fn** `count` — Общее число закладок
`crates/storage/src/broadcast_channels.rs:24` **struct** `ChannelRegistration`
`crates/storage/src/broadcast_channels.rs:34` **struct** `BroadcastChannels`
`crates/storage/src/broadcast_channels.rs:45` **fn** `open`
`crates/storage/src/broadcast_channels.rs:51` **fn** `open_in_memory`
`crates/storage/src/broadcast_channels.rs:83` **fn** `register` — `new BroadcastChannel(name)` — зарегистрировать. Если уже была
`crates/storage/src/broadcast_channels.rs:113` **fn** `get`
`crates/storage/src/broadcast_channels.rs:129` **fn** `listeners` — Все listeners на конкретном канале origin-а
`crates/storage/src/broadcast_channels.rs:152` **fn** `channels_for_origin` — Все channel-имена, на которые подписан origin (distinct)
`crates/storage/src/broadcast_channels.rs:174` **fn** `unregister` — `channel.close()` — снять регистрацию
`crates/storage/src/broadcast_channels.rs:188` **fn** `unregister_context` — При закрытии вкладки — снять все регистрации этого context-а
`crates/storage/src/broadcast_channels.rs:202` **fn** `count`
`crates/storage/src/browser_settings.rs:45` **struct** `BrowserSettingsSnapshot` — All browser settings in a single value type for easy read/write
`crates/storage/src/browser_settings.rs:91` **struct** `BrowserSettings` — Persistent settings store
`crates/storage/src/browser_settings.rs:114` **fn** `open` — Open (or create) an on-disk settings database
`crates/storage/src/browser_settings.rs:120` **fn** `open_in_memory` — Create an in-memory settings database (for tests / ephemeral sessions)
`crates/storage/src/browser_settings.rs:180` **fn** `homepage` — Homepage / new-tab URL
`crates/storage/src/browser_settings.rs:185` **fn** `set_homepage` — Set homepage URL
`crates/storage/src/browser_settings.rs:190` **fn** `search_engine_id` — ID of the default search engine (`SearchProviderEntry::id`)
`crates/storage/src/browser_settings.rs:195` **fn** `set_search_engine_id` — Set default search engine ID
`crates/storage/src/browser_settings.rs:200` **fn** `shields_enabled` — Whether shields (tracker blocker) are globally enabled
`crates/storage/src/browser_settings.rs:205` **fn** `set_shields_enabled` — Set shields on/off
`crates/storage/src/browser_settings.rs:210` **fn** `fingerprint_mode` — Fingerprint resistance mode: `"standard"`, `"strict"`, or `"off"`
`crates/storage/src/browser_settings.rs:215` **fn** `set_fingerprint_mode` — Set fingerprint resistance mode
`crates/storage/src/browser_settings.rs:220` **fn** `doh_enabled` — Whether DNS-over-HTTPS is enabled
`crates/storage/src/browser_settings.rs:225` **fn** `set_doh_enabled` — Set DNS-over-HTTPS on/off
`crates/storage/src/browser_settings.rs:230` **fn** `font_size` — Base font size in CSS px (e.g. 16.0)
`crates/storage/src/browser_settings.rs:235` **fn** `set_font_size` — Set base font size
`crates/storage/src/browser_settings.rs:240` **fn** `theme` — UI theme: `"dark"`, `"light"`, or `"system"`
`crates/storage/src/browser_settings.rs:245` **fn** `set_theme` — Set UI theme
`crates/storage/src/browser_settings.rs:250` **fn** `download_path` — Absolute path to the default download directory. Empty = OS default
`crates/storage/src/browser_settings.rs:255` **fn** `set_download_path` — Set default download directory path
`crates/storage/src/browser_settings.rs:260` **fn** `tab_layout` — Tab layout mode: `"horizontal"` or `"vertical"` (GG-4)
`crates/storage/src/browser_settings.rs:265` **fn** `set_tab_layout` — Set tab layout mode
`crates/storage/src/browser_settings.rs:270` **fn** `panel_layout` — Serialised docked-panel layout string (F2-6c); empty = built-in defaults
`crates/storage/src/browser_settings.rs:275` **fn** `set_panel_layout` — Persist the serialised docked-panel layout
`crates/storage/src/browser_settings.rs:280` **fn** `snapshot` — Read all settings into a snapshot value
`crates/storage/src/browser_settings.rs:296` **fn** `apply_snapshot` — Persist all fields from a snapshot in one call
`crates/storage/src/cache_storage.rs:19` **struct** `CachedEntry`
`crates/storage/src/cache_storage.rs:30` **struct** `CacheStorage`
`crates/storage/src/cache_storage.rs:41` **fn** `open`
`crates/storage/src/cache_storage.rs:47` **fn** `open_in_memory`
`crates/storage/src/cache_storage.rs:80` **fn** `put` — `cache.put(request, response)` — записать пару
`crates/storage/src/cache_storage.rs:122` **fn** `match_` — `cache.match(request)` — найти ответ. Метод по умолчанию `GET`
`crates/storage/src/cache_storage.rs:146` **fn** `delete` — `cache.delete(request)` — удалить пару. Возвращает true если удалили
`crates/storage/src/cache_storage.rs:168` **fn** `keys` — `cache.keys()` — все entries в одном именованном кэше
`crates/storage/src/cache_storage.rs:193` **fn** `list_cache_names` — `caches.keys()` — список имён всех кэшей origin-а (distinct)
`crates/storage/src/cache_storage.rs:215` **fn** `delete_cache` — `caches.delete(name)` — удалить весь кэш с именем `cache_name`
`crates/storage/src/cache_storage.rs:230` **fn** `clear_origin` — Очистить все entries для origin-а (origin storage clear)
`crates/storage/src/cache_storage.rs:244` **fn** `count`
`crates/storage/src/cache_storage.rs:256` **fn** `match_by_url` — `cache.match(url)` without knowing the method — returns first match by URL
`crates/storage/src/cache_storage.rs:280` **fn** `match_any` — `caches.match(url)` — search across all caches for the origin
`crates/storage/src/cache_storage.rs:303` **fn** `has_cache` — `caches.has(name)` — true if the named cache has at least one entry
`crates/storage/src/cached_dns.rs:39` **trait** `Clock` — Источник unix-времени. Дефолт — `SystemTime::now` через
`crates/storage/src/cached_dns.rs:47` **struct** `SystemClock` — Реальные часы через `SystemTime::now()`. При панике (часы до UNIX
`crates/storage/src/cached_dns.rs:63` **struct** `CachedDnsResolver` — Кеширующий DNS-резолвер
`crates/storage/src/cached_dns.rs:74` **fn** `new` — `default_ttl_seconds` — TTL для каждой записи (от `cached_at`)
`crates/storage/src/cached_dns.rs:88` **fn** `with_clock` — То же, что `new`, но с подменяемым clock (тесты)
`crates/storage/src/cookies.rs:28` **enum** `SameSite` — SameSite политика cookie. RFC 6265bis §4.1.2
`crates/storage/src/cookies.rs:59` **struct** `Cookie` — Один cookie с атрибутами. domain хранится lowercase, path — как есть
`crates/storage/src/cookies.rs:72` **struct** `CookieJar` — Cookie jar — обёртка над SQLite-БД cookies
`crates/storage/src/cookies.rs:83` **fn** `open`
`crates/storage/src/cookies.rs:89` **fn** `open_in_memory`
`crates/storage/src/cookies.rs:123` **fn** `set` — Записать (или обновить) cookie. domain нормализуется к lowercase
`crates/storage/src/cookies.rs:155` **fn** `delete` — Удалить конкретный cookie по (domain, path, name, top_level_site)
`crates/storage/src/cookies.rs:183` **fn** `clear_expired` — Удалить все expired cookies (`expires_at < now`). Session cookies
`crates/storage/src/cookies.rs:199` **fn** `clear_session` — Удалить все session cookies (`expires_at IS NULL`). Зовётся при
`crates/storage/src/cookies.rs:217` **fn** `get_for_request` — Получить все cookies, применимые к данному запросу. Фильтрация:
`crates/storage/src/cookies.rs:339` **fn** `parse_set_cookie` — Распарсить значение HTTP-заголовка `Set-Cookie` в `Cookie`. Без PSL
`crates/storage/src/cookies.rs:368` **fn** `parse_set_cookie_with_psl` — Расширенная версия [`parse_set_cookie`] с опциональной проверкой
`crates/storage/src/cookies.rs:554` **struct** `CookieJarProvider` — Implements [`CookieProvider`] using a shared [`CookieJar`]
`crates/storage/src/cookies.rs:561` **fn** `new` — Create a provider backed by the given jar
`crates/storage/src/csp_policies.rs:28` **fn** `parse_csp_header` — Парсит CSP-заголовок в map `directive → sources`
`crates/storage/src/csp_policies.rs:43` **struct** `CspPolicy`
`crates/storage/src/csp_policies.rs:52` **struct** `CspPolicies`
`crates/storage/src/csp_policies.rs:63` **fn** `open`
`crates/storage/src/csp_policies.rs:69` **fn** `open_in_memory`
`crates/storage/src/csp_policies.rs:93` **fn** `store`
`crates/storage/src/csp_policies.rs:110` **fn** `get`
`crates/storage/src/csp_policies.rs:140` **fn** `delete`
`crates/storage/src/csp_policies.rs:153` **fn** `count`
`crates/storage/src/dns_cache.rs:17` **struct** `DnsEntry`
`crates/storage/src/dns_cache.rs:26` **fn** `is_fresh`
`crates/storage/src/dns_cache.rs:31` **struct** `DnsCache`
`crates/storage/src/dns_cache.rs:42` **fn** `open`
`crates/storage/src/dns_cache.rs:48` **fn** `open_in_memory`
`crates/storage/src/dns_cache.rs:78` **fn** `put` — Сохранить DNS-resolve в кэше. Перезаписывает существующую запись
`crates/storage/src/dns_cache.rs:104` **fn** `get` — Получить fresh-запись. Если истекла — `None` (caller идёт в DNS-resolver)
`crates/storage/src/dns_cache.rs:134` **fn** `delete`
`crates/storage/src/dns_cache.rs:147` **fn** `clear_expired`
`crates/storage/src/dns_cache.rs:161` **fn** `clear`
`crates/storage/src/dns_cache.rs:171` **fn** `count`
`crates/storage/src/downloads.rs:16` **enum** `DownloadStatus` — Статус скачивания
`crates/storage/src/downloads.rs:49` **struct** `DownloadEntry` — Одна запись о скачивании
`crates/storage/src/downloads.rs:68` **struct** `Downloads`
`crates/storage/src/downloads.rs:79` **fn** `open`
`crates/storage/src/downloads.rs:85` **fn** `open_in_memory`
`crates/storage/src/downloads.rs:120` **fn** `start` — Создать запись о новом скачивании. Возвращает id
`crates/storage/src/downloads.rs:143` **fn** `update_progress` — Обновить bytes_received (для прогресса)
`crates/storage/src/downloads.rs:157` **fn** `complete` — Зафиксировать успешное завершение
`crates/storage/src/downloads.rs:171` **fn** `cancel` — Зафиксировать отмену пользователем
`crates/storage/src/downloads.rs:185` **fn** `fail` — Зафиксировать ошибку
`crates/storage/src/downloads.rs:198` **fn** `get`
`crates/storage/src/downloads.rs:215` **fn** `list_all` — Все записи в порядке started_at DESC
`crates/storage/src/downloads.rs:238` **fn** `list_by_status` — Только в указанном статусе
`crates/storage/src/downloads.rs:261` **fn** `delete` — Удалить запись (например, после удаления файла или clear-history)
`crates/storage/src/downloads.rs:272` **fn** `clear_completed` — Удалить все завершённые (done/cancelled/failed). Pending не трогаются
`crates/storage/src/downloads.rs:286` **fn** `count`
`crates/storage/src/history.rs:34` **struct** `HistoryEntry` — Запись истории. Возвращается при чтении / поиске
`crates/storage/src/history.rs:45` **struct** `History` — История пользователя
`crates/storage/src/history.rs:56` **fn** `open`
`crates/storage/src/history.rs:62` **fn** `open_in_memory`
`crates/storage/src/history.rs:98` **fn** `record_visit` — Зафиксировать визит. Если url уже встречался — обновляем title /
`crates/storage/src/history.rs:120` **fn** `set_favicon` — Установить favicon-hash для url. Никак не аффектит visit_count
`crates/storage/src/history.rs:134` **fn** `set_text_sha256` — Установить text_sha256 (для дедупликации readability-content)
`crates/storage/src/history.rs:148` **fn** `get` — Найти запись по URL
`crates/storage/src/history.rs:166` **fn** `recent` — Последние N записей (по убыванию visit_date)
`crates/storage/src/history.rs:188` **fn** `most_visited` — Топ-N записей по visit_count. Удобно для new-tab «most visited»
`crates/storage/src/history.rs:220` **fn** `search_prefix` — Поиск по url и title: case-insensitive substring match
`crates/storage/src/history.rs:257` **fn** `delete` — Удалить запись по url. Никаких ошибок, если url не существует
`crates/storage/src/history.rs:269` **fn** `delete_older_than` — Удалить все записи с `visit_date < before`. Возвращает число
`crates/storage/src/history.rs:284` **fn** `clear` — Полная очистка истории
`crates/storage/src/hsts.rs:19` **struct** `HstsEntry`
`crates/storage/src/hsts.rs:31` **fn** `parse_sts_header` — Парсит Strict-Transport-Security header
`crates/storage/src/hsts.rs:59` **struct** `HstsStore`
`crates/storage/src/hsts.rs:70` **fn** `open`
`crates/storage/src/hsts.rs:76` **fn** `open_in_memory`
`crates/storage/src/hsts.rs:106` **fn** `upsert` — Записать HSTS entry. `host` — lowercase ASCII hostname (без порта)
`crates/storage/src/hsts.rs:146` **fn** `is_https_only` — Проверить, должен ли host обрабатываться как HTTPS-only
`crates/storage/src/hsts.rs:189` **fn** `get`
`crates/storage/src/hsts.rs:212` **fn** `delete`
`crates/storage/src/hsts.rs:223` **fn** `purge_expired` — Удалить все просроченные entries (для GC)
`crates/storage/src/hsts.rs:237` **fn** `count`
`crates/storage/src/http_cache.rs:28` **struct** `CacheControl` — Распарсенные директивы Cache-Control. Из RFC 9111 §5.2 берём только
`crates/storage/src/http_cache.rs:43` **fn** `parse` — Распарсить значение Cache-Control HTTP-заголовка
`crates/storage/src/http_cache.rs:75` **fn** `is_cacheable` — Можно ли вообще хранить ответ в кеше?
`crates/storage/src/http_cache.rs:82` **struct** `CachedResponse` — Кешированная HTTP-запись
`crates/storage/src/http_cache.rs:97` **fn** `is_fresh`
`crates/storage/src/http_cache.rs:105` **struct** `HttpCache`
`crates/storage/src/http_cache.rs:116` **fn** `open`
`crates/storage/src/http_cache.rs:122` **fn** `open_in_memory`
`crates/storage/src/http_cache.rs:157` **fn** `put` — Положить ответ в кеш. Перезаписывает существующую запись с
`crates/storage/src/http_cache.rs:198` **fn** `get` — Получить ответ по URL. Возвращает `Some` даже если запись
`crates/storage/src/http_cache.rs:228` **fn** `get_fresh` — Получить ответ, но только если он свежий (`now < expires_at`)
`crates/storage/src/http_cache.rs:239` **fn** `delete` — Удалить запись
`crates/storage/src/http_cache.rs:253` **fn** `clear_expired` — Удалить expired записи. Возвращает число удалённых строк
`crates/storage/src/http_cache.rs:268` **fn** `clear` — Полная очистка кеша
`crates/storage/src/http_cache.rs:279` **fn** `count` — Общее число записей
`crates/storage/src/indexed_db.rs:42` **fn** `origin_key` — Вычислить безопасный файловый ключ для origin
`crates/storage/src/indexed_db.rs:65` **struct** `IdbStore` — Per-origin persistence для IndexedDB поверх [`StorageBackend`]
`crates/storage/src/indexed_db.rs:76` **fn** `new` — Создать store для конкретного `origin` поверх разделяемого `backend`
`crates/storage/src/indexed_db.rs:89` **fn** `open_or_create` — Открыть или создать выделенный SQLite-файл для IndexedDB
`crates/storage/src/indexed_db.rs:101` **fn** `for_origin` — Открыть или создать IDB-хранилище для `etld_plus_one` в директории `idb_dir`
`crates/storage/src/indexed_db.rs:147` **struct** `NativeIdbStore` — Structured per-origin SQLite backend for IndexedDB (Phase 3)
`crates/storage/src/indexed_db.rs:198` **fn** `open_or_create` — Open or create the structured IDB store at `path` (file is created if absent)
`crates/storage/src/indexed_db.rs:205` **fn** `open_in_memory` — Open an in-memory structured IDB store (tests / ephemeral sessions)
`crates/storage/src/indexed_db.rs:214` **fn** `for_origin` — Open/create the structured store for `etld_plus_one` under `idb_dir`
`crates/storage/src/keyboard_shortcuts.rs:15` **struct** `KeyboardShortcutEntry` — A single keybinding: a command name paired with its modifier + key strings
`crates/storage/src/keyboard_shortcuts.rs:27` **struct** `KeyboardShortcuts` — Persistent store for keyboard shortcut overrides
`crates/storage/src/keyboard_shortcuts.rs:51` **fn** `open` — Open (or create) an on-disk shortcuts database
`crates/storage/src/keyboard_shortcuts.rs:57` **fn** `open_in_memory` — Create an in-memory shortcuts database (for tests / ephemeral sessions)
`crates/storage/src/keyboard_shortcuts.rs:63` **fn** `all` — Return all stored overrides
`crates/storage/src/keyboard_shortcuts.rs:83` **fn** `get` — Return the stored override for `command`, or `None` if using default
`crates/storage/src/keyboard_shortcuts.rs:100` **fn** `set` — Save (or overwrite) a binding override for `command`
`crates/storage/src/keyboard_shortcuts.rs:113` **fn** `remove` — Remove the override for `command` (reverts to compile-time default)
`crates/storage/src/notifications.rs:18` **struct** `Notification`
`crates/storage/src/notifications.rs:34` **struct** `Notifications`
`crates/storage/src/notifications.rs:45` **fn** `open`
`crates/storage/src/notifications.rs:51` **fn** `open_in_memory`
`crates/storage/src/notifications.rs:90` **fn** `show` — Показать notification. Если `tag` непустая и для (origin, tag)
`crates/storage/src/notifications.rs:139` **fn** `mark_dismissed`
`crates/storage/src/notifications.rs:152` **fn** `mark_clicked`
`crates/storage/src/notifications.rs:165` **fn** `get`
`crates/storage/src/notifications.rs:182` **fn** `active` — Активные (не dismissed и не clicked) notifications
`crates/storage/src/notifications.rs:207` **fn** `history` — История всех показанных notifications (включая закрытые)
`crates/storage/src/notifications.rs:229` **fn** `delete`
`crates/storage/src/notifications.rs:239` **fn** `delete_older_than`
`crates/storage/src/notifications.rs:253` **fn** `count`
`crates/storage/src/omnibox_aliases.rs:23` **struct** `OmniboxAlias` — One omnibox bang-alias entry
`crates/storage/src/omnibox_aliases.rs:35` **struct** `OmniboxAliases` — SQLite-backed registry of omnibox bang-aliases
`crates/storage/src/omnibox_aliases.rs:47` **fn** `open` — Open persistent alias store at `path`
`crates/storage/src/omnibox_aliases.rs:54` **fn** `open_in_memory` — Open in-memory store (tests / ephemeral sessions)
`crates/storage/src/omnibox_aliases.rs:97` **fn** `set` — Add or replace an alias.  `trigger` must start with `!`
`crates/storage/src/omnibox_aliases.rs:109` **fn** `get` — Look up an alias by its `trigger` (e.g. `"!g"`)
`crates/storage/src/omnibox_aliases.rs:124` **fn** `list_all` — All aliases ordered by trigger
`crates/storage/src/omnibox_aliases.rs:145` **fn** `delete` — Delete an alias by trigger.  No-op if not found
`crates/storage/src/permissions.rs:20` **enum** `PermissionKind` — Известные типы permissions. Произвольные строки тоже допустимы для
`crates/storage/src/permissions.rs:34` **fn** `as_str`
`crates/storage/src/permissions.rs:47` **fn** `parse`
`crates/storage/src/permissions.rs:63` **enum** `PermissionState` — State permission grant
`crates/storage/src/permissions.rs:91` **struct** `PermissionEntry`
`crates/storage/src/permissions.rs:100` **struct** `Permissions`
`crates/storage/src/permissions.rs:111` **fn** `open`
`crates/storage/src/permissions.rs:117` **fn** `open_in_memory`
`crates/storage/src/permissions.rs:146` **fn** `set` — Поставить state для (origin, kind). Перезаписывает существующий
`crates/storage/src/permissions.rs:170` **fn** `query` — Получить текущий state. Если запись есть, но `expires_at < now` —
`crates/storage/src/permissions.rs:199` **fn** `touch` — Обновить last_used_at — вызывается при фактическом использовании
`crates/storage/src/permissions.rs:213` **fn** `revoke` — Удалить grant (revoke)
`crates/storage/src/permissions.rs:227` **fn** `list_for_origin` — Все permissions для одного origin
`crates/storage/src/permissions.rs:249` **fn** `list_all` — Все записи в БД (для UI permissions-manager)
`crates/storage/src/permissions.rs:271` **fn** `clear_expired` — Удалить все expired grants. Возвращает число удалённых
`crates/storage/src/permissions.rs:286` **fn** `clear_origin` — Удалить все permissions для origin (clear site data)
`crates/storage/src/permissions_policy.rs:26` **enum** `PermissionsAllowlist` — Allowlist для одной feature
`crates/storage/src/permissions_policy.rs:38` **fn** `is_blocked` — `true` если allowlist пуст (`()` или `Origins(vec![])`)
`crates/storage/src/permissions_policy.rs:47` **fn** `allows_self` — `true` если разрешено для текущего origin (`(self)` или `*`)
`crates/storage/src/permissions_policy.rs:59` **fn** `parse_permissions_policy` — Парсит Permissions-Policy header
`crates/storage/src/permissions_policy.rs:129` **struct** `PermissionsPolicy`
`crates/storage/src/permissions_policy.rs:138` **struct** `PermissionsPolicies`
`crates/storage/src/permissions_policy.rs:149` **fn** `open`
`crates/storage/src/permissions_policy.rs:155` **fn** `open_in_memory`
`crates/storage/src/permissions_policy.rs:179` **fn** `store`
`crates/storage/src/permissions_policy.rs:196` **fn** `get`
`crates/storage/src/permissions_policy.rs:226` **fn** `delete`
`crates/storage/src/permissions_policy.rs:239` **fn** `count`
`crates/storage/src/plugins.rs:24` **struct** `PluginManifest`
`crates/storage/src/plugins.rs:37` **struct** `Plugins`
`crates/storage/src/plugins.rs:48` **fn** `open`
`crates/storage/src/plugins.rs:54` **fn** `open_in_memory`
`crates/storage/src/plugins.rs:85` **fn** `install` — Установить плагин. Если name уже есть — Error (UNIQUE constraint)
`crates/storage/src/plugins.rs:108` **fn** `update_manifest` — Обновить версию + capabilities (например, после re-install с новой
`crates/storage/src/plugins.rs:128` **fn** `set_enabled`
`crates/storage/src/plugins.rs:142` **fn** `touch` — Обновить last_used_at (вызывается при каждом invocation плагина)
`crates/storage/src/plugins.rs:155` **fn** `get`
`crates/storage/src/plugins.rs:171` **fn** `get_by_name`
`crates/storage/src/plugins.rs:188` **fn** `list_all` — Все установленные плагины (включая disabled). ORDER BY installed_at ASC
`crates/storage/src/plugins.rs:211` **fn** `list_enabled` — Только enabled-плагины — для runtime-loading
`crates/storage/src/plugins.rs:233` **fn** `uninstall`
`crates/storage/src/plugins.rs:243` **fn** `count`
`crates/storage/src/print_prefs.rs:45` **struct** `PrintPrefsSnapshot` — All print preferences as a copyable value type
`crates/storage/src/print_prefs.rs:87` **struct** `PrintPrefs` — Print preferences backed by SQLite
`crates/storage/src/print_prefs.rs:99` **fn** `open` — Open (or create) the SQLite store for print preferences
`crates/storage/src/print_prefs.rs:120` **fn** `load_snapshot` — Load the current snapshot of all print preferences
`crates/storage/src/print_prefs.rs:146` **fn** `save_snapshot` — Persist a snapshot of print preferences to the database
`crates/storage/src/profile_vault.rs:52` **fn** `generate_storage_key` — Generate a cryptographically random 32-byte storage key
`crates/storage/src/profile_vault.rs:102` **fn** `seal` — Seal a 32-byte `storage_key` under `password`
`crates/storage/src/profile_vault.rs:130` **fn** `open` — Open a sealed blob, recovering the 32-byte storage key
`crates/storage/src/profiles.rs:30` **struct** `Profile` — Один профиль пользователя
`crates/storage/src/profiles.rs:49` **struct** `ProfileRegistry`
`crates/storage/src/profiles.rs:60` **fn** `open`
`crates/storage/src/profiles.rs:66` **fn** `open_in_memory`
`crates/storage/src/profiles.rs:111` **fn** `create` — Создать новый профиль. Имя должно быть уникальным
`crates/storage/src/profiles.rs:132` **fn** `get` — Получить профиль по id
`crates/storage/src/profiles.rs:154` **fn** `get_by_name` — Получить профиль по имени
`crates/storage/src/profiles.rs:176` **fn** `list_all` — Все профили. Сортировка по created_at ASC (порядок создания)
`crates/storage/src/profiles.rs:201` **fn** `rename` — Переименовать. Имя уникально — конфликт → Error
`crates/storage/src/profiles.rs:215` **fn** `set_settings` — Обновить settings_json
`crates/storage/src/profiles.rs:230` **fn** `delete` — Удалить профиль. Если он был активным — active становится NULL
`crates/storage/src/profiles.rs:244` **fn** `set_active` — Установить активный профиль. `None` → нет активного
`crates/storage/src/profiles.rs:269` **fn** `active` — Получить активный профиль
`crates/storage/src/profiles.rs:298` **fn** `set_password` — Защитить профиль паролем
`crates/storage/src/profiles.rs:321` **fn** `clear_password` — Снять пароль с профиля
`crates/storage/src/profiles.rs:340` **fn** `unlock` — Разблокировать профиль и получить 32-байтовый ключ хранилища
`crates/storage/src/profiles.rs:363` **fn** `is_encrypted` — Проверить, защищён ли профиль паролем
`crates/storage/src/profiles.rs:382` **fn** `count`
`crates/storage/src/psl.rs:31` **struct** `PslProvider` — Реализация `PublicSuffixList` поверх crate-а `psl` (compiled-in таблица)
`crates/storage/src/psl.rs:35` **fn** `new`
`crates/storage/src/push_subscriptions.rs:20` **struct** `PushSubscription`
`crates/storage/src/push_subscriptions.rs:36` **struct** `PushSubscriptions`
`crates/storage/src/push_subscriptions.rs:47` **fn** `open`
`crates/storage/src/push_subscriptions.rs:53` **fn** `open_in_memory`
`crates/storage/src/push_subscriptions.rs:85` **fn** `subscribe`
`crates/storage/src/push_subscriptions.rs:129` **fn** `get`
`crates/storage/src/push_subscriptions.rs:144` **fn** `get_by_scope`
`crates/storage/src/push_subscriptions.rs:159` **fn** `list_for_origin`
`crates/storage/src/push_subscriptions.rs:180` **fn** `list_all`
`crates/storage/src/push_subscriptions.rs:201` **fn** `unsubscribe`
`crates/storage/src/push_subscriptions.rs:214` **fn** `unsubscribe_origin`
`crates/storage/src/push_subscriptions.rs:228` **fn** `count`
`crates/storage/src/referrer_policy.rs:18` **enum** `ReferrerPolicy`
`crates/storage/src/referrer_policy.rs:43` **fn** `as_str`
`crates/storage/src/referrer_policy.rs:56` **fn** `parse`
`crates/storage/src/referrer_policy.rs:74` **struct** `ReferrerPolicies`
`crates/storage/src/referrer_policy.rs:85` **fn** `open`
`crates/storage/src/referrer_policy.rs:91` **fn** `open_in_memory`
`crates/storage/src/referrer_policy.rs:116` **fn** `set` — Установить policy для origin. Перезаписывает существующую
`crates/storage/src/referrer_policy.rs:135` **fn** `get` — Получить policy для origin. Если нет записи — None
`crates/storage/src/referrer_policy.rs:152` **fn** `get_or_default` — Получить policy с fallback на default (если нет per-origin)
`crates/storage/src/referrer_policy.rs:156` **fn** `delete`
`crates/storage/src/referrer_policy.rs:169` **fn** `list_all`
`crates/storage/src/referrer_policy.rs:193` **fn** `count`
`crates/storage/src/safe_browsing.rs:54` **enum** `ThreatType` — Категория угрозы для записи в Safe Browsing list. Имена совпадают с
`crates/storage/src/safe_browsing.rs:71` **fn** `as_code` — Сериализация в стабильный кодовый идентификатор для БД (lowercase
`crates/storage/src/safe_browsing.rs:84` **fn** `from_code` — Обратный парсинг из кодового id. Неизвестные строки → `Other(s)`,
`crates/storage/src/safe_browsing.rs:112` **fn** `canonical_expression_variants` — Сгенерировать список всех 5×4=20 канонических вариантов `host/path?query`
`crates/storage/src/safe_browsing.rs:131` **fn** `canonical_expression_variants_with_psl` — Версия [`canonical_expression_variants`] с опциональной обрезкой
`crates/storage/src/safe_browsing.rs:266` **fn** `hash_expression` — Хэш канонического expression-а — SHA-256 32 байта. Удобный helper для
`crates/storage/src/safe_browsing.rs:282` **struct** `SafeBrowsingList` — SQLite-backed список Safe Browsing записей
`crates/storage/src/safe_browsing.rs:293` **fn** `open`
`crates/storage/src/safe_browsing.rs:299` **fn** `open_in_memory`
`crates/storage/src/safe_browsing.rs:329` **fn** `add_hash` — Добавить запись по уже-хэшированному значению. `full_hash` обязан
`crates/storage/src/safe_browsing.rs:358` **fn** `add_url` — Удобный wrapper: канонизировать URL → SHA-256 → `add_hash`
`crates/storage/src/safe_browsing.rs:389` **fn** `lookup_hash` — Прямой lookup по полному хэшу (32 байта). Возвращает первое
`crates/storage/src/safe_browsing.rs:415` **fn** `lookup_url` — Главный entry-point фильтрации: проверить URL против всех списков,
`crates/storage/src/safe_browsing.rs:423` **fn** `lookup_url_with_psl` — Версия [`Self::lookup_url`] с опциональной PSL-обрезкой host-suffix
`crates/storage/src/safe_browsing.rs:443` **fn** `clear_list` — Удалить все записи указанного списка. `clear_list("google-v4")` —
`crates/storage/src/safe_browsing.rs:456` **fn** `clear_all` — Удалить все записи во всех списках. Используется при logout/profile
`crates/storage/src/safe_browsing.rs:465` **fn** `count_in` — Сколько записей в конкретном списке
`crates/storage/src/safe_browsing.rs:478` **fn** `count_total` — Сколько всего записей во всех списках
`crates/storage/src/safe_browsing.rs:498` **struct** `SafeBrowsingFilter` — Тонкая обёртка над [`SafeBrowsingList`] для подключения в
`crates/storage/src/safe_browsing.rs:505` **fn** `new`
`crates/storage/src/safe_browsing.rs:513` **fn** `with_psl` — Builder-конструктор с подключённым `PublicSuffixList`. С PSL
`crates/storage/src/search_history.rs:20` **struct** `SearchQuery`
`crates/storage/src/search_history.rs:31` **struct** `SearchHistory`
`crates/storage/src/search_history.rs:42` **fn** `open`
`crates/storage/src/search_history.rs:48` **fn** `open_in_memory`
`crates/storage/src/search_history.rs:80` **fn** `record` — Зафиксировать запрос. Если normalized уже в БД — инкрементит
`crates/storage/src/search_history.rs:104` **fn** `recent` — Последние N запросов по last_used DESC
`crates/storage/src/search_history.rs:126` **fn** `popular` — Самые частые запросы (DESC by frequency, tie-break — last_used DESC)
`crates/storage/src/search_history.rs:149` **fn** `prefix_match` — Запросы, начинающиеся с `prefix` (case-insensitive). Сортировка
`crates/storage/src/search_history.rs:173` **fn** `delete_query`
`crates/storage/src/search_history.rs:186` **fn** `delete_older_than`
`crates/storage/src/search_history.rs:200` **fn** `clear`
`crates/storage/src/search_history.rs:210` **fn** `count`
`crates/storage/src/search_providers.rs:21` **struct** `SearchProviderEntry` — Один поисковый провайдер
`crates/storage/src/search_providers.rs:37` **fn** `build_url` — Подставить query на место `{query}` с URL-encoding по RFC 3986
`crates/storage/src/search_providers.rs:81` **struct** `SearchProviders` — Реестр поисковых провайдеров
`crates/storage/src/search_providers.rs:92` **fn** `open`
`crates/storage/src/search_providers.rs:98` **fn** `open_in_memory`
`crates/storage/src/search_providers.rs:133` **fn** `add` — Добавить провайдера. Имя уникально
`crates/storage/src/search_providers.rs:152` **fn** `get` — Получить провайдера по id
`crates/storage/src/search_providers.rs:169` **fn** `get_by_name`
`crates/storage/src/search_providers.rs:187` **fn** `list_all` — Все провайдеры в порядке создания
`crates/storage/src/search_providers.rs:209` **fn** `delete`
`crates/storage/src/search_providers.rs:221` **fn** `set_default`
`crates/storage/src/search_providers.rs:246` **fn** `default`
`crates/storage/src/search_providers.rs:266` **fn** `count`
`crates/storage/src/service_workers.rs:21` **enum** `UpdateViaCache`
`crates/storage/src/service_workers.rs:32` **fn** `as_str`
`crates/storage/src/service_workers.rs:39` **fn** `parse`
`crates/storage/src/service_workers.rs:50` **struct** `ServiceWorkerRegistration`
`crates/storage/src/service_workers.rs:60` **struct** `ServiceWorkers`
`crates/storage/src/service_workers.rs:71` **fn** `open`
`crates/storage/src/service_workers.rs:77` **fn** `open_in_memory`
`crates/storage/src/service_workers.rs:107` **fn** `register`
`crates/storage/src/service_workers.rs:139` **fn** `touch`
`crates/storage/src/service_workers.rs:152` **fn** `get`
`crates/storage/src/service_workers.rs:169` **fn** `find_for_url` — Найти SW для конкретного URL: scope с самым длинным prefix-match
`crates/storage/src/service_workers.rs:193` **fn** `list_for_origin`
`crates/storage/src/service_workers.rs:214` **fn** `unregister`
`crates/storage/src/service_workers.rs:227` **fn** `unregister_origin`
`crates/storage/src/service_workers.rs:241` **fn** `count`
`crates/storage/src/session_export.rs:26` **struct** `SessionFile` — Portable session file structure
`crates/storage/src/session_export.rs:38` **struct** `ExportedTab` — One tab in a portable session file
`crates/storage/src/session_export.rs:51` **fn** `to_json` — Serialize a [`SessionFile`] to a compact JSON string
`crates/storage/src/session_export.rs:77` **fn** `from_json` — Deserialize a [`SessionFile`] from a JSON string
`crates/storage/src/session_export.rs:139` **fn** `active_tab` — Return the first active tab, or the first tab if none is marked active
`crates/storage/src/session_store.rs:29` **struct** `PersistedTab` — One persisted tab in the saved session
`crates/storage/src/session_store.rs:48` **struct** `SessionStore` — SQLite-backed store holding exactly one session — the tabs open at last close
`crates/storage/src/session_store.rs:60` **fn** `open_in_memory` — Open an in-memory store (data lost when the process exits)
`crates/storage/src/session_store.rs:67` **fn** `open` — Open a persistent on-disk store at `path`
`crates/storage/src/session_store.rs:98` **fn** `save` — Replace the saved session with `tabs`, preserving their order
`crates/storage/src/session_store.rs:130` **fn** `load` — Load all saved tabs in their original left-to-right order
`crates/storage/src/session_store.rs:158` **fn** `clear` — Remove all saved tabs (e.g. user disabled session restore)
`crates/storage/src/session_store.rs:166` **fn** `len` — Number of tabs in the saved session
`crates/storage/src/session_store.rs:175` **fn** `is_empty` — Returns `true` when no session has been saved
`crates/storage/src/site_engagement.rs:22` **struct** `SiteEngagement`
`crates/storage/src/site_engagement.rs:36` **fn** `score` — Engagement score с exponential decay по last_visit. Чем дальше
`crates/storage/src/site_engagement.rs:45` **struct** `SiteEngagementStore`
`crates/storage/src/site_engagement.rs:56` **fn** `open`
`crates/storage/src/site_engagement.rs:62` **fn** `open_in_memory`
`crates/storage/src/site_engagement.rs:91` **fn** `record_visit` — Зафиксировать визит. Инкрементирует visit_count, обновляет last_visit
`crates/storage/src/site_engagement.rs:109` **fn** `add_time` — Добавить time на сайте (foreground seconds)
`crates/storage/src/site_engagement.rs:123` **fn** `get`
`crates/storage/src/site_engagement.rs:142` **fn** `top_by_score` — Топ-N origin-ов по score (decay-нормированному). Алгоритм:
`crates/storage/src/site_engagement.rs:172` **fn** `delete`
`crates/storage/src/site_engagement.rs:185` **fn** `count`
`crates/storage/src/sqlite_store.rs:29` **struct** `SqliteStorage` — Persistent KV-хранилище на SQLite. Создаёт таблицу `kv` при инициализации
`crates/storage/src/sqlite_store.rs:41` **fn** `open` — Открыть БД по пути (файл создаётся при отсутствии)
`crates/storage/src/sqlite_store.rs:49` **fn** `open_in_memory` — Открыть in-memory БД (для тестов и ephemeral session-state)
`crates/storage/src/store.rs:12` **struct** `InMemoryStorage` — In-memory KV-хранилище. Все данные в RAM; `serialize`/`deserialize`
`crates/storage/src/store.rs:77` **fn** `new`
`crates/storage/src/store.rs:82` **fn** `serialize` — Сериализует хранилище в байты (snapshot-формат `LUMEN_KV_V1`)
`crates/storage/src/store.rs:95` **fn** `deserialize` — Десериализует snapshot
`crates/storage/src/store.rs:133` **fn** `save` — Сохраняет snapshot в файл
`crates/storage/src/store.rs:139` **fn** `load` — Загружает snapshot из файла
`crates/storage/src/sw_interceptor.rs:27` **struct** `ServiceWorkerInterceptor` — SQLite-backed SW fetch interceptor
`crates/storage/src/sw_interceptor.rs:41` **fn** `new` — Create an interceptor with cache-only SW interception (Phase 0 behaviour)
`crates/storage/src/sw_interceptor.rs:54` **fn** `with_sw_workers` — Attach a `SwWorkerStore` so that incoming fetch requests are dispatched
`crates/storage/src/sw_store.rs:25` **struct** `SwStore` — Per-origin persistence SW-регистраций поверх общего [`StorageBackend`]
`crates/storage/src/sw_store.rs:35` **fn** `new` — Создать store для конкретного `origin` поверх разделяемого `backend`
`crates/storage/src/tab_groups.rs:30` **struct** `PersistedGroup` — One persisted tab group
`crates/storage/src/tab_groups.rs:46` **struct** `TabGroups` — SQLite-backed store of tab-group metadata
`crates/storage/src/tab_groups.rs:58` **fn** `open` — Open (or create) the store at `path`
`crates/storage/src/tab_groups.rs:65` **fn** `open_in_memory` — Open an ephemeral in-memory store (tests / private sessions)
`crates/storage/src/tab_groups.rs:94` **fn** `create` — Create a group. `position` is auto-assigned as `MAX(existing) + 1`
`crates/storage/src/tab_groups.rs:116` **fn** `get` — Fetch a group by id. `None` if absent
`crates/storage/src/tab_groups.rs:132` **fn** `list_all` — All groups, ordered by `position` ascending
`crates/storage/src/tab_groups.rs:154` **fn** `rename` — Rename a group. Missing id is a no-op
`crates/storage/src/tab_groups.rs:164` **fn** `set_color` — Change a group's colour palette index. Missing id is a no-op
`crates/storage/src/tab_groups.rs:174` **fn** `set_collapsed` — Set the collapsed flag. Missing id is a no-op
`crates/storage/src/tab_groups.rs:184` **fn** `set_position` — Set the display position. Missing id is a no-op
`crates/storage/src/tab_groups.rs:194` **fn** `delete` — Delete a group. Missing id is a no-op
`crates/storage/src/tab_groups.rs:205` **fn** `count` — Number of stored groups
`crates/storage/src/tab_sessions.rs:19` **struct** `TabSession` — Одна вкладка в сохранённой сессии
`crates/storage/src/tab_sessions.rs:40` **struct** `SessionSnapshot` — Снимок сессии — корневая запись для group of tabs
`crates/storage/src/tab_sessions.rs:46` **struct** `TabSessions`
`crates/storage/src/tab_sessions.rs:57` **fn** `open`
`crates/storage/src/tab_sessions.rs:63` **fn** `open_in_memory`
`crates/storage/src/tab_sessions.rs:107` **fn** `create_snapshot` — Создать новый snapshot сессии. Возвращает session_id
`crates/storage/src/tab_sessions.rs:122` **fn** `add_tab` — Добавить вкладку в указанный snapshot
`crates/storage/src/tab_sessions.rs:160` **fn** `update_scroll` — Обновить scroll-позицию (часто меняется)
`crates/storage/src/tab_sessions.rs:174` **fn** `update_form_values` — Обновить form-values (JSON-строка)
`crates/storage/src/tab_sessions.rs:187` **fn** `get_snapshot`
`crates/storage/src/tab_sessions.rs:208` **fn** `list_snapshots` — Все snapshot-ы сессий в порядке created_at DESC (последний — первый)
`crates/storage/src/tab_sessions.rs:236` **fn** `list_tabs` — Все вкладки в snapshot-е
`crates/storage/src/tab_sessions.rs:260` **fn** `delete_snapshot` — Удалить snapshot (cascade удаляет все его вкладки через FK)
`crates/storage/src/tab_sessions.rs:274` **fn** `delete_tab` — Удалить одну вкладку
`crates/storage/src/tab_sessions.rs:285` **fn** `snapshot_count` — Число snapshot-ов
`crates/storage/src/tab_snapshot.rs:95` **struct** `HibernatedTabData` — All data stored on disk for a hibernated tab
`crates/storage/src/tab_snapshot.rs:120` **struct** `TabSnapshotStore` — SQLite-backed store for hibernated tab snapshots
`crates/storage/src/tab_snapshot.rs:132` **fn** `open_in_memory` — Open an in-memory store (data is lost when the process exits)
`crates/storage/src/tab_snapshot.rs:139` **fn** `open` — Open a persistent on-disk store at `path`
`crates/storage/src/tab_snapshot.rs:167` **fn** `store` — Persist a hibernated tab snapshot.  Overwrites any previous entry for
`crates/storage/src/tab_snapshot.rs:191` **fn** `fetch` — Load the hibernated snapshot for `tab_id`
`crates/storage/src/tab_snapshot.rs:222` **fn** `delete` — Remove the snapshot for `tab_id` (called after successful restore)
`crates/storage/src/tab_snapshot.rs:233` **fn** `exists` — Returns `true` if a snapshot exists for `tab_id`
`crates/storage/src/tab_snapshot.rs:263` **struct** `T2SleepData` — Snapshot data persisted when a tab enters T2 (BackgroundOld)
`crates/storage/src/tab_snapshot.rs:285` **struct** `SleepingTabStore` — SQLite-backed store for T2 (BackgroundOld) tab checkpoints
`crates/storage/src/tab_snapshot.rs:297` **fn** `open_in_memory` — Open an in-memory store (data lost on process exit)
`crates/storage/src/tab_snapshot.rs:304` **fn** `open` — Open a persistent on-disk store at `path`
`crates/storage/src/tab_snapshot.rs:340` **fn** `store` — Persist a T2 checkpoint.  Overwrites any previous entry for the same tab
`crates/storage/src/tab_snapshot.rs:367` **fn** `fetch` — Load the T2 checkpoint for `tab_id`
`crates/storage/src/tab_snapshot.rs:406` **fn** `delete` — Remove the checkpoint for `tab_id` (called after successful restore or close)
`crates/storage/src/tab_snapshot.rs:414` **fn** `exists` — Returns `true` if a checkpoint exists for `tab_id`
`crates/storage/src/web_manifest.rs:14` **struct** `WebManifest`
`crates/storage/src/web_manifest.rs:25` **struct** `WebManifests`
`crates/storage/src/web_manifest.rs:36` **fn** `open`
`crates/storage/src/web_manifest.rs:42` **fn** `open_in_memory`
`crates/storage/src/web_manifest.rs:69` **fn** `store`
`crates/storage/src/web_manifest.rs:93` **fn** `set_installed`
`crates/storage/src/web_manifest.rs:106` **fn** `get`
`crates/storage/src/web_manifest.rs:130` **fn** `list_installed` — Все установленные PWA (для UI «Installed apps»)
`crates/storage/src/web_manifest.rs:159` **fn** `delete`
`crates/storage/src/web_manifest.rs:172` **fn** `count`
`crates/storage/src/workspaces.rs:18` **struct** `Workspace`
`crates/storage/src/workspaces.rs:32` **struct** `Workspaces`
`crates/storage/src/workspaces.rs:43` **fn** `open`
`crates/storage/src/workspaces.rs:49` **fn** `open_in_memory`
`crates/storage/src/workspaces.rs:81` **fn** `create` — Создать workspace. Position автоматически = MAX(existing)+1
`crates/storage/src/workspaces.rs:109` **fn** `get`
`crates/storage/src/workspaces.rs:124` **fn** `get_by_name`
`crates/storage/src/workspaces.rs:140` **fn** `list_all` — Все workspace-ы в порядке position ASC
`crates/storage/src/workspaces.rs:161` **fn** `rename`
`crates/storage/src/workspaces.rs:174` **fn** `set_color`
`crates/storage/src/workspaces.rs:187` **fn** `set_icon`
`crates/storage/src/workspaces.rs:200` **fn** `set_position`
`crates/storage/src/workspaces.rs:213` **fn** `delete`
`crates/storage/src/workspaces.rs:223` **fn** `count`

---
*Total: 4500 symbols in 22 crates*
