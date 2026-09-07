# BUG-1027: на Linux фикс BUG-987 не действовал — живое окно умирало на 200 вложенных `<div>`

**Статус:** FIXED 2026-09-07
**Дата:** 2026-09-07
**Компонент:** shell (`crates/shell/build.rs`, `crates/shell/src/main.rs`,
`crates/shell/src/app/user_event.rs`, `crates/shell/src/window_mode.rs`),
js (`crates/js/src/v8_runtime/runtime.rs`), core (`crates/core/src/deep_stack.rs`)
**Найден:** 2026-09-07 по жалобе пользователя «падает программа при запуске S2 и S3»

## Симптом

Процесс исчезает мгновенно. Ни паники, ни бэктрейса, `RUST_BACKTRACE=full`
ничего не добавляет — рантайм ловит переполнение стека своим обработчиком
сигнала и зовёт `abort()`:

```
thread '<unknown>' (490174) has overflowed its stack
fatal runtime error: stack overflow, aborting
```

Снаружи это выглядит по-разному в зависимости от того, кто запускал браузер:
в прогоне WPT (`run_report.py --binary target/dev-release/lumen`, инстансы
`lumen --bidi-port N`) — как `[bidi] frame error: io: failed to fill whole
buffer` и оборванный прогон (класс [BUG-1006](BUG-1006-OPEN.md)); в живом
окне — как пропавшее окно без единой строки в логе.

## Механизм

`build_box_or_reuse` → `build_box` → `build_box_inner`
(`crates/engine/layout/src/box_tree/build.rs:166/242/1066`) — три кадра на
один уровень DOM. Замер по core dump (`gdb`, `info frame` на кадре #12
относительно кадра вызывающего): **10 640 байт стека на уровень
вложенности**. Отсюда потолок каждого потока = его стек / 10.4 КБ.

[BUG-987](BUG-987-FIXED.md) (FIXED 2026-09-04) поднял стеки, но ровно в двух
местах, и оба не покрывают Linux целиком:

1. `crates/shell/build.rs` — `cargo:rustc-link-arg=/STACK:134217728` под
   `#[cfg(target_os = "windows")]`. Это PE-заголовок; на Linux размер стека
   главного потока берётся из `RLIMIT_STACK` (штатно 8 МиБ) и на этапе
   линковки не задаётся вовсе — Linux игнорирует `PT_GNU_STACK.p_memsz`.
2. `engine_thread.rs` — `stack_size(128 MiB)` у потока `lumen-engine`.

Мимо прошли три потока, каждый из которых спускается по дереву:

- **`app/user_event.rs:211`** — финальный pipeline (BUG-171 этап 2:
  `render_bytes` → `parse_and_layout` → `layout_page`), голый
  `std::thread::spawn`, штатные 2 МиБ. Это он убивает живое окно на **200**
  вложенных `<div>` (2 МиБ / 10.4 КБ ≈ 190). Безымянный — отсюда бесполезное
  `thread '<unknown>'` в сообщении.
- **главный поток на Unix** — streaming-layout (`paint_partial_dom`),
  синхронный relayout, chrome, все dump-режимы: 8 МиБ ≈ **790** уровней.
- **`lumen-v8`** (`v8_runtime/runtime.rs:281`) — `dom_helpers::import_node`,
  рекурсивный импорт разобранного фрагмента при `innerHTML`/
  `insertAdjacentHTML`: те же 2 МиБ.

Замеры порогов до фикса (`<div>`×N, статическая страница без JS; debug и
dev-release совпадают до уровня — размер кадра не зависит от оптимизации):

| путь | поток | стек | проходит | падает |
|---|---|---|---|---|
| живое окно | безымянный (`user_event.rs:211`) | 2 МиБ | 150 | **200** |
| `--screenshot`, `--dump-layout` | `main` | 8 МиБ | 700 | **800** |
| `innerHTML` через BiDi | `lumen-v8` | 2 МиБ | — | краш 02:41, core dump |

Воспроизводимость — 100 % (3 из 3 прогонов на N=200 в debug, dev-release —
так же), порог детерминированный.

## Что сделано

- `lumen_core::deep_stack` (новый модуль): константа
  `DEEP_TREE_STACK_BYTES = 128 МиБ` — единая точка правды вместо литерала на
  каждом спавне — и `run_on_deep_stack(name, f)`, синхронный хоп на scoped-поток
  с этим стеком (паника пробрасывается `resume_unwind`, поведение вызывающего
  не меняется).
- `main.rs`: на Unix, кроме macOS, вся работа (`cli_args::run_cli`) уходит на
  поток `lumen-main` с этим стеком. macOS исключён — там event loop обязан
  жить на главном потоке процесса. Отказ ОС в создании потока — не фатален:
  логируем и работаем на штатном стеке.
- `window_mode.rs`: `with_any_thread(true)` обоим Unix-бэкендам winit
  (Wayland и X11) — event loop теперь строится не на главном потоке процесса,
  и без явного разрешения winit это запрещает.
- `app/user_event.rs`: поток финального pipeline получил имя `lumen-pipeline`
  и `DEEP_TREE_STACK_BYTES`; отказ спавна логируется (иначе `RenderDone` не
  придёт никогда и страница молча застынет на streaming-кадре).
- `v8_runtime/runtime.rs`: `lumen-v8` получил тот же стек.
- `page_load.rs`: streaming-поток назван `lumen-stream`. Стек штатный —
  поток читает сокет и гоняет preload-сканер, деревом не спускается.
- `build.rs`: комментарий теперь прямо говорит, что флаг Windows-only и где
  живёт Unix-эквивалент.

## Проверка

`crates/shell/tests/deep_nesting.rs` — `<div>`×2000 (выше обоих исторических
потолков) через `--dump-layout`, `--screenshot` и `innerHTML`+`--screenshot`
(последний бьёт по `import_node` на `lumen-v8`). Живое окно тестом не
покрыто — нужны event loop и GPU-поверхность; проверено вручную:
до фикса `lumen <div×200>` умирал за ~0.5 с, после — окно живёт.

## Остатки

- Итеративный обход вместо рекурсии — [LAYOUT-1](../ROADMAP.md) (done) /
  [LAYOUT-2](../ROADMAP.md) (in progress) для layout/paint; `import_node` в
  их объём не входит и заведён как [BUG-1028](BUG-1028-OPEN.md).
- 10.4 КБ стека на уровень — сам по себе повод посмотреть на размер кадров
  `build_box_inner`; вынесено в тот же LAYOUT-2.
- Осиротевшие процессы `lumen` после прогонов WPT, дважды за 2026-09-07
  доводившие машину до OOM-kill, — [BUG-1029](BUG-1029-OPEN.md); к этому
  дефекту отношения не имеют, но в тот же день выглядели одинаково
  («браузер пропал»).

## Урок

Фикс, зависящий от флага линковщика или от `#[cfg(target_os)]`, закрывает
баг **только на той платформе, где его проверяли**. BUG-987 закрыт
2026-09-04 с формулировкой «после фикса `--screenshot` на 5000 уровнях
проходит» — на Windows это было правдой, на Linux порог остался 200/790, и
три дня прогонов WPT списывали эти смерти на тулинг.
