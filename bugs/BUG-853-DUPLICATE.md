> **Дубль.** Тот же дефект тремя срезами раньше описан в
> [BUG-838](BUG-838-FIXED.md) (заведён 2026-08-22, WPT-RUN-6 срез 21): та же
> строка `dom.rs`, та же пара id, та же спековая ссылка. По конвенции выживает
> первый по дате — этот файл оставлен только как след замера среза 24, маркер
> `script-empty-src` и разбор починки перенесены в BUG-838. Починено
> 2026-08-25 (P1), перепроверено этой пробой 2026-08-25 — см. «Перезамер» ниже.

# BUG-853 — `<script src="">` не порождает ни `load`, ни `error`: пустой `src` молча трактуется как «внешнего скрипта нет», хотя спека требует немедленного `error`

**Статус:** DUPLICATE → [BUG-838](BUG-838-FIXED.md) (закрыт 2026-08-25)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 24 — живой замер, маркер `script-empty-src`)
**Область:** `crates/js/src/dom.rs:6533`–`6534` (`src = trim(); if (src !== '') { _lumen_script_load_external(...); return; }` — пустая строка уходит в ветку «источник — тело элемента»)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var s = document.createElement('script');
s.onload  = () => console.log('load');   // не зовётся
s.onerror = () => console.log('error');  // не зовётся
s.setAttribute('src', '');
document.body.appendChild(s);            // тишина
```

HTML LS §4.12.1 «prepare the script element», шаг 30: если атрибут `src`
присутствует и его значение — пустая строка, нужно поставить задачу и
диспатчить `error` на элементе. Тест, который ждёт этого `error`, висит до
таймаута.

## Прямое измерение

`tests/wpt/verify_frame_load_media_gaps.py --variant script-empty-src`
(2026-08-22, dev-release, Linux, коммит `c583a90b4`, `--seconds 5`, страница
жива — 9 тиков):

| ожидалось | получено |
|---|---|
| `script-empty-error` | тишина |
| контроль `script-good-load ran=1` | `script-good-load ran=1` |

Контрольный `<script src=…>` на той же странице грузится и стреляет `load`,
то есть механизм событий у элемента исправен — отличается только пустой URL.
На сервере пробы запроса с пустым путём тоже нет (и не должно быть).

## Причина (локализована чтением кода)

```js
// dom.rs:6533
src = (src === null) ? '' : String(src).trim();
if (src !== '') { _lumen_script_load_external(nid, src, isModule); return; }
// `src` wins over the inline body; with no `src` the body is the source.
```

Отсутствующий и пустой `src` неразличимы: обе ветки ведут к «источник — тело
элемента», а тело пустое, поэтому не выполняется и не сообщается ничего.
Спека же различает их — присутствующий пустой `src` это ошибка загрузки.

## Масштаб

Маркер `script-empty-src` в `tests/wpt/timeout_audit.py` — **2 id** остатка
снимка WPT-RUN-5:
`html/semantics/scripting-1/the-script-element/fetch-src/empty.html` и
`empty-with-base.html`. Оба — `async_test`, ждущий `error`.

## Направление починки (не предписание)

Различить «атрибута нет» и «атрибут есть, значение пустое» (`_lumen_has_attr`
до `trim`) и во втором случае поставить задачу, диспатчащую `error` через
`_lumen_resource_fire`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_frame_load_media_gaps.py
   --variant script-empty-src` — ожидается `script-empty-error`.
2. WPT: `run_report.py --all --root html/semantics/scripting-1/the-script-element/fetch-src`.

---

## Перезамер (P1, 2026-08-25) — закрыт починкой BUG-838

Проба этого файла, тот же вариант, `main` = `49046b7dc`, dev-release,
Windows, `--seconds 6`, страница жива (4 тика):

| ожидалось | было (2026-08-22) | стало |
|---|---|---|
| `script-empty-error` | тишина | `script-empty-error` |
| контроль `script-good-load ran=1` | `script-good-load ran=1` | `script-good-load ran=1` |
| запрос с пустым путём на сервере пробы | нет | нет (`server saw: /vflm-asset.js?good=1`) |

То есть закрыто не «похожей» правкой, а той же самой: обе заявки указывали на
одну ветку `_lumen_script_prepare`, и разошлись только номера строк
(`dom.rs:6533` здесь против `:5844` там) — файл сдвигался между срезами.
Ветка BUG-838 покрывает вход этой заявки (`createElement` + `setAttribute`) и
сверх него парсерный, которого замер среза 24 не касался.

Кода эта заявка не добавила; что она добавила поверх BUG-838 — маркер
`script-empty-src` в `tests/wpt/timeout_audit.py` и явное имя двух id.
Ссылка маркера переведена на BUG-838 (по уроку среза 26: неверный `ref`
читается как «у механизма есть владелец» в каждом отчёте инструмента).
