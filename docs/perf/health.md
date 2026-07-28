# Журнал здоровья сессии (PERF-6)

Локальный **privacy-first журнал проблем**, которые браузер поймал во время
реального использования, и его агрегатор
[`scripts/health_report.py`](../../scripts/health_report.py). Отвечает на вопрос,
который разовые находки не покрывают: **что ломается чаще всего у живого
пользователя** — чтобы P3 чинил баги по частоте встречаемости, а не по случайным
находкам (аналогия: graphic_tests ловят пиксельные регрессии, здесь единица —
«сколько раз это укусило»).

В отличие от остальной дорожки PERF (PERF-2/3/4/5 — чисто-тулинговые, 0 кода в
движке), тут **есть** движковая часть: расширение поверхности `--activity-log`
([`crates/shell/src/health_log.rs`](../../crates/shell/src/health_log.rs)). Сам
отчёт кода в движок не добавляет — движок эмитит сырые записи, скрипт агрегирует.

## Что записывается

Движок пишет `health.log` (JSON Lines, рядом с рабочим каталогом — как
`activity.log`) под флагом `--health-log`, `--activity-log`/`--click-log` или
`LUMEN_HEALTH_LOG=1`. Каждая строка — самодостаточный JSON-объект с полем `kind`;
журнал усекается при старте (каждая сессия — с чистого листа) и содержит **только
проблемы**, а не полный лог навигации:

| `kind` | Когда | Поля |
|---|---|---|
| `panic` | паника Rust на любом потоке (panic-hook, цепляется к прежнему) | `detail` (message), `location`, `backtrace`, `url` (страница на момент падения) |
| `console_error` | страница вызвала `console.error(...)` | `detail` (текст), `url` |
| `load_error` | навигация не загрузилась (сеть/TLS/декод/render) | `detail` (ошибка), `url` |
| `broken_render` | страница загрузилась, но **ничего не отрисовала** при содержательном DOM | `dom_nodes`, `layout_boxes`, `rendered_units`, `url` |

**Эвристика белого экрана** (`broken_render`): фиксируется, только когда
`dom_nodes ≥ 20` (страница намеревалась что-то показать) **и** `rendered_units == 0`
(в layout-дереве нет ни одного печатаемого символа inline-текста и ни одного
replaced-элемента `<img>/<canvas>/<video>/<iframe>`). Требование строгого нуля
держит ложные срабатывания низкими. `rendered_units`/`layout_boxes` считаются в
шелле (`count_rendered_units`/`count_layout_boxes`), `dom_nodes` = размер арены
DOM (`Document::node_count`).

## Приватность

Всё остаётся на машине (принцип [privacy.md](../plan/privacy.md)). Движок пишет
локальный файл; отчёт только читает его и ничего не отправляет.

## Честные ограничения

1. **`console_error`/`broken_render` ловятся только в живом окне.** Хуки висят на
   frame-loop дренаже консоли и на `apply_loaded_page` — headless `--screenshot`
   их не проходит (там другой путь `render_source_to_png`). Панику panic-hook
   ловит в любом режиме, `health.log` со `session_start` создаётся при старте
   всегда.
2. **Белый экран через CSS-фон не ловится.** Страница, у которой единственный
   видимый контент — CSS `background-image` без текстовых/replaced боксов, даст
   `rendered_units == 0` → ложный `broken_render`. Принятое, задокументированное
   ограничение (текст/replaced покрывают подавляющее большинство реальных страниц).
3. **`dom_nodes` = вся арена, включая осиротевшие узлы.** Арена append-only, так
   что оторванные поддеревья раздувают счётчик; на срабатывание эвристики это
   влияет консервативно-в-плюс (больше узлов → легче преодолеть порог 20), но
   `rendered_units == 0` остаётся сильным якорем.

## Запуск

```bash
cargo build -p lumen-shell --profile dev-release
# Живая сессия с журналом здоровья (браузьте как обычно):
target/dev-release/lumen.exe --health-log https://example.com
# Отчёт по накопленному health.log:
python scripts/health_report.py                 # приоритизация по частоте
python scripts/health_report.py --top 20
python scripts/health_report.py --kind panic    # только паники
python scripts/health_report.py --json          # машинный вывод
python scripts/health_report.py --selftest      # проверка без браузера (ворота)
```

## Как читать отчёт

Отчёт даёт три блока:

* **By kind** — сколько всего событий каждого типа за сессию.
* **Most problematic hosts** — хосты, взвешенные по серьёзности
  (`panic`=10, `broken_render`=5, `load_error`=3, `console_error`=1): куда смотреть
  в первую очередь.
* **Top recurring issues** — сигнатуры (`kind` + хост + нормализованный текст:
  числа→`#`, URL→`<url>`), ранжированные по `повторы × вес`. Это и есть очередь
  P3-багфиксов «сначала то, что бьёт чаще».

## Хронология

| Дата | Commit | Заметка |
|---|---|---|
| 2026-07-18 | (PERF-6) | Первый срез. Движковая часть `health_log.rs` + агрегатор `health_report.py` (`--selftest` в воротах). Фикстура [`scripts/perf-fixtures/health.html`](../../scripts/perf-fixtures/health.html): 24 `display:none`-узла + `console.error` → одновременно `broken_render` и `console_error` для дымового прогона живого окна. |
