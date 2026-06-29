# RP-2 — Relayout под живой размер окна (убрать хардкод 1024×720)

**Developer:** P1 · **Ветка:** `p1-rp-2-resize-viewport` · **Размер:** M · **Крейты:** `lumen-shell` (+ возможно `lumen-layout` для vw/vh)

> Roadmap: `ROADMAP.md` строка `RP-2` (родитель `RP`).
> Capability gap: `CAPABILITIES.md:174` — «no relayout-on-resize (viewport hardcoded 1024×720)».

---

## Контекст

**Не greenfield.** Обработчик `WindowEvent::Resized` (main.rs:8361) уже:
1. ресайзит renderer (`r.resize(size.width, size.height)`),
2. вызывает `self.relayout()`,
3. доставляет ResizeObserver-записи.

То есть каркас «resize → relayout» **существует**. Реальный gap — **layout-viewport не
отслеживает живой размер окна**: по коду рассыпаны константы `Size::new(1024.0, 720.0)` и
fallback'и `self.window.as_ref().map_or((1024, 720), …)`, а высота окна фиксируется как
`720 + TAB_BAR_HEIGHT` (main.rs:7365-7367, «чтобы graphic tests получали ровно 720»). Нужно
аккуратно: graphic-тесты требуют детерминированных 1024×720 в headless/тестовом пути, но
**интерактивное окно** должно раскладываться под фактический `inner_size`.

Задача: чтобы `relayout()` и проход рендера брали CSS-viewport из **живого**
`window.inner_size()` (делённого на scale_factor), а не из константы; `vw`/`vh`/`%`-высоты и
`@media (width/height)` следовали за окном. Хардкод оставить **только** в headless-путях
(`--screenshot`, `--dump-*`, IPC-таб) и в graphic-test harness.

## Пред-запуск

- [ ] Прочитать main.rs:8361-8390 — обработчики `Resized` и `ScaleFactorChanged`.
- [ ] Прочитать main.rs:7355-7370 — где задаётся initial размер окна (1024 × 720+tabbar).
- [ ] Прочитать main.rs:1825-1832 + 2222-2227 — трейт-метод `update_viewport_size` и его реализация.
- [ ] Прочитать `fn relayout` (grep `fn relayout` в main.rs) — какой `viewport: Size` он подаёт в layout.
- [ ] Найти все `Size::new(1024.0, 720.0)` и `map_or((1024, 720)` (grep ниже) и классифицировать
      каждое как **headless/тест** (оставить) или **живое окно** (заменить на inner_size).

```bash
grep -n "1024.0, 720.0\|map_or((1024, 720)\|SCREENSHOT_VP_W\|SCREENSHOT_MIN_H" crates/shell/src/main.rs
```

## Ключевые точки (реальные file:line)

- `crates/shell/src/main.rs:8361` — `WindowEvent::Resized` (renderer.resize + relayout + observers).
- `crates/shell/src/main.rs:8341/8378` — `ScaleFactorChanged` (DPI; не пересоздаёт surface).
- `crates/shell/src/main.rs:7365-7367` — initial окно = 1024 × (720 + TAB_BAR_HEIGHT).
- `crates/shell/src/main.rs:804-808` — `SCREENSHOT_VP_W=1024`, `SCREENSHOT_MIN_H=720` (headless, НЕ трогать).
- `crates/shell/src/main.rs:1829` / `:2222` — `update_viewport_size(w,h)` (прокидывает vw/vh в JS,
  дёргает `_lumen_deliver_resize_observers` + media-query).
- `crates/shell/src/main.rs:6292/6717/7286` — места, где `js.update_viewport_size(viewport.…)`
  уже зовётся после layout — сверить, что `viewport` там живой.
- `crates/shell/src/main.rs:1303-1309`, `6657`, `6807`, `7551`, `9688`, `10441`… — кандидаты на
  замену `1024×720` на живой `inner_size` (классифицировать каждый!).

## Подводные камни

- **Graphic-тесты должны остаться детерминированными 1024×720.** Менять только путь живого окна;
  headless (`--screenshot`/`--dump`/`--ipc-server`) и тест-хелперы — оставить фикс. Если живое окно
  при тестах через gdigrab должно оставаться 1024×720 — initial size при запуске пусть остаётся
  1024 × (720+tabbar), задача в том, чтобы **последующий resize пересчитывал** viewport, а не
  игнорировал его.
- **CSS-viewport = inner_size / scale_factor** (CSS px), web-контент = окно минус высота tab-strip
  (`tabs::strip::TAB_BAR_HEIGHT`). Не путать physical px и CSS px.
- `vw`/`vh` уже резолвятся через `viewport: Size` (style.rs использует vp) — главное подать
  правильный `Size` в layout-проход; отдельная правка lumen-layout вероятно не нужна.
- Минимизация окна шлёт `Resized(0,0)` — ранний `return` уже есть (main.rs:8365), сохранить.

## Шаги

1. Ветка + worktree (`p1-rp-2-resize-viewport`).
2. Ввести единый источник CSS-viewport для живого окна: метод `Lumen::live_viewport() -> Size`,
   считающий `inner_size().to_logical(scale_factor)` минус tab-strip по высоте. Заменить им
   живые `Size::new(1024,720)` / `map_or((1024,720))` (НЕ headless).
3. `relayout()` подаёт `live_viewport()` в layout; после него уже зовётся
   `update_viewport_size` — проверить порядок (media-query после viewport, main.rs:1908).
4. На `Resized`: пересчитать `live_viewport()`, relayout (уже есть) + `update_viewport_size`
   с новыми размерами, чтобы JS `innerWidth`/`matchMedia`/vw/vh обновились.
5. Прогнать `graphic_tests` (`run.py --only 00` + пара) — убедиться, что детерминизм 1024×720
   не сломан.

## Тесты

- shell unit: `live_viewport_tracks_inner_size` (mock inner_size 1280×800, scale 1.0 →
  viewport 1280 × (800−tabbar)).
- shell unit: `live_viewport_divides_by_scale_factor` (inner 2048×1440 @2.0 → 1024×(720−tabbar/…)).
- Ручная проверка: запустить `cargo run -p lumen-shell -- samples/page.html`, потянуть окно —
  контент перетекает, не обрезается.
- Регресс: `python graphic_tests/run.py --only 00` остаётся PASS (детерминизм).

## Definition of done

- Растягивание/сжатие окна перекладывает страницу под новый CSS-viewport; `vw`/`vh`/`%`-высоты и
  `@media (max-width)` следуют за окном.
- Headless-пути и graphic-тесты остаются детерминированными 1024×720.
- `CAPABILITIES.md:174` — убрать «no relayout-on-resize; viewport hardcoded 1024×720».
- Удалить указатель `ROADMAP.md:180` из `STATUS-P1.md`; `RP-2` → `done`.
