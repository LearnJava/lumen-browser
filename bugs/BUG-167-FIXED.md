# BUG-167

**Статус:** FIXED 2026-06-19
**Компонент:** shell

## Описание

При входе в полноэкранный режим через Fullscreen API (`element.requestFullscreen()` →
`window.set_fullscreen(Borderless)`) окно ОС растягивается на весь десктоп, но вьюпорт
страницы не пересчитывался под новые размеры fullscreen-окна.

Наблюдалось: страница продолжала раскладываться в исходном вьюпорте (~1024×720),
`vw`/`vh` и `auto`-центрирование считались от старых размеров — контент не растягивался
во весь экран, снизу/справа оставалась пустая область фона окна.

Ожидаемо (WHATWG Fullscreen): при `fullscreenchange` на вход вьюпорт принимает
размеры fullscreen-области, страница перелейаучивается, `window.innerWidth/innerHeight`
и `vw`/`vh` отражают новый размер.

## Корень

`set_fullscreen` применяет новый размер **асинхронно**: сразу после вызова
`window.inner_size()` ещё возвращает старый размер, поэтому не было откуда взять
финальные размеры для resize+relayout. Обычный путь `WindowEvent::Resized`
прилетал бы позже, но fullscreen-toggle не гарантированно его порождает на всех
платформах, и вьюпорт оставался старым.

## Исправление

Добавлена отложенная reconciliation размера (`crates/shell/src/main.rs`):

- Поле `Lumen.fullscreen_resize_pending: Option<(u32, u32, u8)>` — физический
  inner-size окна **до** toggle + бюджет опросов.
- `arm_fullscreen_resize(prev)` вооружает reconciliation сразу после
  `set_fullscreen(..)` (на входе через `take_fullscreen_requests` и на выходе по
  `Escape`), будит loop через `request_redraw`.
- `poll_fullscreen_resize()` вызывается из `about_to_wait` каждую итерацию: как
  только `inner_size()` отличается от pre-toggle размера — прогоняет тот же путь,
  что `WindowEvent::Resized` (`renderer.resize` + `relayout` + доставка
  `ResizeObserver`). Бюджет 240 опросов (~4 с при 60 fps) защищает от спина при
  no-op toggle.
- Чистое решение вынесено в `decide_fullscreen_poll(prev, cur, attempts) ->
  FullscreenPoll` (`Apply`/`Wait`/`Done`), покрыто 6 юнит-тестами без реального окна.

## Как воспроизвести (до фикса)

1. Открыть любую страницу, вызвать `document.documentElement.requestFullscreen()`.
2. Окно уходит в borderless fullscreen на весь экран.
3. Контент остаётся в исходном вьюпорте, не растягивается.

После фикса контент растягивается на fullscreen-область; на выходе из fullscreen
вьюпорт возвращается к оконному размеру.
