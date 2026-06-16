# BUG-170

**Статус:** OPEN
**Компонент:** shell + font (font-display, FOUT)
**Приоритет:** высокий (−~0.5–2.4 с с критического пути первого paint на тяжёлых страницах)

## Корень

`parse_and_layout` блокирует весь конвейер страницы на загрузке `@font-face`
шрифтов: `load_font_faces` (`crates/shell/src/main.rs:3067` в теле
`parse_and_layout`) скачивает и декодирует ВСЕ web-шрифты ДО первого
`layout_measured_hyp`. Текст не появляется на экране, пока не докачается
последний woff2.

Замер диагноза (lenta.ru, 2026-06-16): 7 woff2 = 2.4 с последовательно. После
параллелизации fetch'а (коммит f9ee8b46) это ~0.5 с, но даже 0.5 с — это
блокировка первого paint латентностью самого медленного шрифта, чего по спеке
быть не должно.

По CSS Fonts L4 §6 `font-display` поведение по умолчанию (`auto`, на практике
≈`block` с коротким block-периодом ~100 мс, затем swap) и явный `swap` требуют:
**рисовать текст немедленно фолбэк-шрифтом, подменять на web-шрифт когда он
догрузится** (FOUT — Flash Of Unstyled Text). Сейчас движок реализует
поведение `block` с бесконечным block-периодом — худший случай.

`font-display` УЖЕ парсится: `FontFaceRule.display` заполняется в
`crates/engine/css-parser/src/parser.rs:2199` (`auto|block|swap|fallback|optional`).
Значение просто никем не используется в конвейере загрузки — это чистая
shell-задача, НЕ P4 (дескриптор уже распарсен).

## Что надо

Убрать web-шрифты с критического пути первого paint. Layout и первый кадр
должны строиться на фолбэк-измерителе (bundled Inter + уже доступные
local()-шрифты), а web-шрифты — догружаться в фоне и подменяться с relayout'ом.

## Как реализовать

1. **Не блокировать `parse_and_layout` на сетевых `@font-face`.**
   В `parse_and_layout` (`main.rs:3067`) разделить `load_font_faces` на две части:
   - синхронно: `local()`-источники (мгновенно, читаются из системных шрифтов) —
     остаются в первом layout;
   - асинхронно: `url()`-источники — НЕ ждать, отдать список
     `(family, weight, style, display, url)` наружу для фоновой загрузки.

   Первый `layout_measured_hyp` строится на `MultiFontMeasurer`, где web-семьи
   ещё не зарегистрированы → измеритель падает на Inter-фолбэк (механизм уже есть,
   `register_family_with_ranges` просто не вызывается для незагруженных).

2. **Фоновая догрузка + событие подмены.**
   - Спавнить поток (как `start_streaming_load`, `main.rs:~5860`), который через
     уже добавленный `parallel_map` качает+декодирует web-шрифты и шлёт в
     event loop новое событие `LoadEvent::FontLoaded { family, weight, style, bytes }`
     (добавить вариант в `enum LoadEvent`, `main.rs:100`).
   - Передавать байты shared-каналом / `EventLoopProxy<LoadEvent>` (как
     `CssLoaded` в `feed_preload_and_emit`).

3. **Обработчик `FontLoaded` на UI-потоке.**
   - Зарегистрировать шрифт в `FontRegistry` + `MultiFontMeasurer`
     (`register_family_with_ranges`, учитывая `unicode-range` как в `main.rs:3096`).
   - Обновить `document.fonts` статус FontFace → `Loaded`.
   - Запустить relayout текущей страницы тем же путём, что и resize/relayout
     (см. `apply_loaded_page` / существующий relayout в shell), и repaint.
     Это и есть «swap»: метрики текста меняются (layout shift / FOUT — допустимо).

4. **Тайминги `font-display` (Phase 1 — упрощённо).**
   Реализовать единое поведение «swap для всех» как первый шаг:
   - `swap`, `auto`, `block`, `fallback` → рисуем фолбэк сразу, подменяем по
     приходу (block-period = 0).
   - `optional` → если шрифт не пришёл к первому paint, можно вообще не
     подменять (не делать relayout) — экономит FOUT-дёрганье; Phase 1 допустимо
     трактовать как `swap`.
   Полные таймлайны (block-period 100 мс, swap-period 3 с) — отдельной задачей,
   не блокируют этот баг.

## Затронутые файлы

- `crates/shell/src/main.rs`:
  - `parse_and_layout` ~3065–3100 (расщепить `load_font_faces`),
  - `load_font_faces` 3308 (вернуть web-источники отдельно от local),
  - `enum LoadEvent` :100 (+`FontLoaded`),
  - обработчик событий ~6369 (+`LoadEvent::FontLoaded` → register + relayout),
  - фоновый поток догрузки (новый, рядом со streaming-загрузкой).
- `crates/paint` — `MultiFontMeasurer` (регистрация на лету уже поддержана).

## Критерий приёмки

- На тяжёлой странице первый paint не ждёт web-шрифтов: текст появляется в
  фолбэке немедленно, затем без перезагрузки подменяется на web-шрифт.
- Замер: время до первого paint на lenta.ru падает на величину загрузки
  шрифтов (≥0.5 с параллельно, больше если шрифт медленный).
- `cargo test -p lumen-shell` зелёный; regression-тест на разделение
  local/url-источников в `load_font_faces`.

## Связано

- Зависит от инфраструктуры из коммита f9ee8b46 (`parallel_map`, параллельный fetch).
- Часть плана перф-оптимизации тяжёлых страниц (диагноз 2026-06-16, п.2).
- Смежно с BUG-171 (вынос всей загрузки с UI-потока) — этот баг решает шрифты
  точечно, BUG-171 — фриз окна целиком.
