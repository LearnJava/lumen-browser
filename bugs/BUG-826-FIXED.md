# BUG-826 — `<link rel=preload|modulepreload|prefetch>` не грузится вообще: хинт пишется в stderr и выбрасывается, событий `load`/`error` нет

**Статус:** FIXED 2026-08-25 (P1)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 20 — 37 TIMEOUT остатка, механизм `preload-hint-never-fetched`)
**Область:** `crates/shell/src/main.rs:285` (единственный потребитель `Event::SubresourceHintFound` — логгер stderr), `crates/shell/src/main.rs:6807` (`dispatch_preload_hints`, доккоммент прямо говорит «в Phase 0 sink логирует в stderr; в будущем запустит fetch через HttpClient»), `crates/engine/html-parser/src/preload_scanner.rs:144` (`rel`-токены: знает `stylesheet`/`preload`/`preconnect`/`dns-prefetch`, `modulepreload` и `prefetch` не распознаются вовсе), `crates/js/src/dom.rs` (`_lumen_resource_track` — белый список `script`/`link`, но грузится только `rel=stylesheet`; см. собственный тест-охранник `dynamic_preload_link_fires_nothing`)
**Владелец:** P1/P3 (шелл + `lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.
**Починка:** `crates/js/src/dom.rs` (`_lumen_link_hint_prepare` и соседи, `_lumen_link_hints_scan`, `relList`), `crates/engine/html-parser/src/preload_scanner.rs` (токены `modulepreload`/`prefetch`), `crates/shell/src/main.rs` (два новых плеча `dispatch_preload_hints` + правка доккоммента).

## Симптом

Ссылка-хинт не приводит ни к одному запросу и не сообщает о себе странице
ничем. Ни `load`, ни `error`, ни таймаут — тишина:

```js
// shadow-dom/declarative/…/shadowrootadoptedstylesheets-modulepreload-basic.html
const link = document.createElement("link");
link.rel = "modulepreload";
link.href = cssUrl;
const loadPromise = new Promise((resolve, reject) => {
  link.onload = resolve;
  link.onerror = reject;
});
document.head.appendChild(link);
await loadPromise;          // ← дальше этой строки тест не проходит никогда
```

Особенно обманчива парсерная форма: в stderr **есть** строка
`⤷ preload js [medium] http://…/psag-asset.js?parsed-preload`, то есть по
логу выглядит так, будто хинт отработал. Запроса при этом не было.

## Прямое измерение

`tests/wpt/verify_preload_script_audio_gaps.py` (2026-08-22, коммит
`79f7df91a`, `--seconds 5`, все пробы живы — по 9 тиков). Колонка «сервер
видел» — независимая половина замера: пробный http-сервер записывает каждый
запрошенный путь, поэтому «запроса не было» не зависит от того, что о себе
сообщает страница.

| проба | маркеры страницы | сервер видел |
|---|---|---|
| `link-stylesheet` (контроль) | `link-load rel=stylesheet` | `/psag-asset.css?stylesheet` |
| `link-preload-script` | только `link-appended` | ничего |
| `link-preload-style` | ничего | ничего |
| `link-modulepreload` | только `link-appended` | ничего |
| `link-prefetch` | ничего | ничего |
| `link-preload-parsed` | только `parsed-link rel=preload` (при этом в stderr `⤷ preload js [medium] …`) | ничего |
| `link-preload-404` | ничего | ничего |

Контроль важен вдвойне: `rel=stylesheet`, созданный тем же
`createElement`, и грузится, и стреляет `load` (BUG-722) — значит дефект не
в «созданных скриптом ссылках» и не в машинерии событий как таковой, а
именно в видах хинтов.

## Причина (локализована чтением кода)

Цепочка обрывается на первом же шаге. Preload-сканер
(`preload_scanner.rs`) находит хинт и складывает `PreloadHint`;
`dispatch_preload_hints` (`main.rs:6807`) резолвит URL, сортирует по
приоритету и эмитит `Event::SubresourceHintFound`. Единственный потребитель
этого события во всём воркспейсе — `match` в `main.rs:285`, который печатает
`⤷ preload …` в stderr. Fetch-а нет: доккоммент функции честно фиксирует это
как Phase 0. То есть preload-сканер сегодня — украшение лога.

Три независимые грани сверху:

1. **Скриптовый путь.** `_lumen_resource_track` пропускает `script`/`link`,
   но реально грузится только `rel=stylesheet`; в `dom.rs` для этого есть
   собственный тест-охранник `dynamic_preload_link_fires_nothing`, который
   *утверждает* нынешнее поведение («A `rel=preload` link must not be
   fetched behind the page's back — no event either way»). Починка обязана
   его переписать, а не обойти.
2. **Сканер не знает половины `rel`.** `modulepreload` и `prefetch` не
   попадают ни в один вариант `match` (`preload_scanner.rs:144`), поэтому
   для них нет даже строки в логе.
3. **`error` тоже никогда.** Проба `link-preload-404` не получила ничего:
   тесты, которые ждут отказа (`*_deny` в `connection-allowlist`,
   `modulepreload-failure`, CSP-шные `font-*-blocked`), виснут ровно так же,
   как ждущие успеха.

## Масштаб

Механизм `preload-hint-never-fetched` забирает **37 id** остатка снимка
WPT-RUN-5 (крупнейший механизм среза 20), а по всему снимку — 45 id, считая
те, что раньше висели на более слабых стадиях. Состав: `preload/*` 12,
`connection-allowlist/tentative` 10 (все 10 остатка категории — они опрашивают
серверный key-value store в `while (true)` до появления преload-нутого URL),
`shadow-dom/declarative/…/shadowrootadoptedstylesheets-modulepreload-*` 6,
`html/semantics/scripting-1/…/modulepreload-referrer*` 2,
`content-security-policy/font-src` 2, `resource-timing` 2, остальное россыпью.

Оценка снизу: реф-тесты вида `css/css-backgrounds/background-attachment-353.html`
(`<link rel=preload as=image onload="takeScreenshot()">`) сюда не входят —
у них нет harness-вывода вообще.

Вне WPT цена та же: сайт, который преload-ит шрифт или скрипт (обычная
практика), на Lumen не получает ни ускорения, ни события; а `modulepreload`
плюс `import` того же URL приводит к повторной загрузке.

## Направление починки (не предписание)

Провести `SubresourceHintFound` до сетевого слоя: тот же путь, по которому
шелл уже грузит `<link rel=stylesheet>`, с кэшированием ответа под URL, чтобы
последующий реальный запрос (`<script src>`, `import`, `<img>`) забирал тело
из preload-кэша, как того требует HTML LS §4.6.7 «link type preload».
Отдельно — уведомить JS-сторону: `load`/`error` на элементе `<link>` идут по
той же машинерии `_lumen_resource_*`, что уже работает для `rel=stylesheet`.
Минимальный первый шаг, дающий больше половины охвата: `preload` и
`modulepreload` (и токены в `preload_scanner.rs`), `prefetch` — потом.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_preload_script_audio_gaps.py
   --variant link-preload-script --variant link-modulepreload
   --variant link-preload-404 --variant link-preload-parsed` — в колонке
   «сервер видел» появляются запрошенные файлы, а страница печатает
   `link-load …` (и `link-error 404` для несуществующего).
2. WPT: `run_report.py --all --root preload --recursive` и
   `--root connection-allowlist` — семейства перестают висеть.

## Перезамер 2026-08-23 (WPT-RUN-6, срез 27): форма заголовка ответа

Раньше замерялся только элемент `<link rel=preload>`. Заголовок ответа
`Link:` — вторая форма того же хинта (HTML LS §4.6.6, «Link headers») и
работает не лучше: `tests/wpt/verify_callback_import_preload_gaps.py
--variant link-header` отдаёт `Link: <…>; rel=preload; as=script` и на самом
документе, и на подключённом им `.css`, а сервер пробы не видит запроса ни
за одним из двух указанных файлов:

```
[server saw: GET /vcip-linked.css]     ← только сама таблица стилей
lh-rt-entries []
lh-checked
```

Элементная форма на той же странице подтверждает старый замер: `<link
rel=preload as=script>` не даёт ни запроса, ни `load`, ни `error`.
Поведение `rel=preconnect` при этом случайно правильное — событий он не
шлёт, чего спека и требует, но по той же причине (хинты не обрабатываются
вовсе).

Цена по остатку WPT-RUN-5: `preload/link-header-on-subresource.html`,
`preload/cross-origin-link-header-on-subresource.sub.html`,
`preload/link-header-preload-imagesrcset.html` (заголовочная форма) и
`preload/preconnect-onerror-event.html` (элементная — тест ждёт `load` от
`rel=preload`, чтобы отличить его от `rel=preconnect`).

## Починка 2026-08-25 (P1)

Fetch живёт **в JS-шиме на самом элементе**, а не в шелле — по той же причине,
по которой там уже жил путь `rel=stylesheet` (BUG-703/BUG-722): `load`/`error`
принадлежат элементу, а у шелла нет по-узлового сигнала завершения, который он
мог бы переслать. Цена — ранний старт, ради которого preload-сканер и
существует: запрос теперь уходит после разбора DOM, а не пока HTML ещё течёт
из сети (см. «Остаток»).

Что сделано, по граням заявки:

1. **Скриптовый путь.** `_lumen_link_prepare` (крючок вставки для элементов,
   созданных `createElement`) теперь помимо ветки stylesheet зовёт
   `_lumen_link_hint_prepare` — независимо, потому что `rel='preload stylesheet'`
   это два типа ссылки на одном элементе. Тест-охранник
   `dynamic_preload_link_fires_nothing`, который *утверждал* прежнее поведение,
   переписан (блок из девяти тестов, `dom.rs`).
2. **Сканер знает все токены.** `preload_scanner.rs`: `modulepreload` и
   `prefetch` — два новых варианта `PreloadHint`, разобранных в
   `dispatch_preload_hints`. Это только строка сетевого лога: реальный запрос
   делает шим.
3. **Парсерная форма.** `<link>`, написанный парсером, через крючок вставки не
   проходит вовсе, поэтому добавлен проход по документу
   (`_lumen_link_hints_scan`) в `_lumen_apply_ready_state('interactive')`.
   По-узловой флаг `_lumen_link_hint_done` держит один fetch на элемент, когда
   оба пути пересекаются (ссылка, добавленная скриптом из `<head>`).
4. **`error` на 404** приходит там же, где `load` — обе половины семейств
   `*_allow`/`*_deny` перестают висеть одинаково.

Спека соблюдена в её *молчаливой* части (§4.6.7), и это не косметика: WPT
`preload/onload-event.html` прямо проверяет, что при `as` вне состояния
(отсутствует или неизвестное слово), при непопадающем `media` и при `type`,
который destination не переваривает, ресурс не берётся и не приходит **ни
одно** событие. Для `modulepreload` правило другое: destination вне
script-подобного множества (плюс `style`/`json`) — это `error`, а неизвестное
слово откатывается к `script` и грузится. JS-типизированное тело
`modulepreload` регистрируется в module map, чтобы последующий `import` того же
URL не качал повторно; тело не-JS типа сознательно не регистрируется — иначе
отказ по типу у будущего импорта превратился бы в синтаксическую ошибку
(BUG-896).

Попутно добавлен `link.relList`/`a.relList` (DOMTokenList над `rel` +
`supports()`): без него ни один тест семейства даже не доходил до своего
предмета — `preload_helper.js::verifyPreloadAndRTSupport` открывается строкой
`link.relList.supports("preload")`. `DOMTokenList` для этого обобщён из
`_lumen_make_class_list` в `_lumen_make_attr_token_list(nid, attrName)`.

### Замер после починки

Та самая проба из раздела «Как проверить фикс», dev-release, Windows,
2026-08-25, `--seconds 6`. Колонка «сервер видел» — независимая половина:

| проба | маркеры страницы | сервер видел |
|---|---|---|
| `link-stylesheet` (контроль) | `link-load rel=stylesheet` | `/psag-asset.css?stylesheet` |
| `link-preload-script` | `link-load rel=preload` | `/psag-asset.js?preload` |
| `link-modulepreload` | `link-load rel=modulepreload` | `/psag-module.js?modulepreload` |
| `link-prefetch` | `link-load rel=prefetch` | `/psag-asset.js?prefetch` |
| `link-preload-parsed` | `link-load parsed` | `/psag-asset.js?parsed-preload` |
| `link-preload-404` | `link-error 404` | `/psag-missing.js?preload-404` |

Все шесть страниц живы (5–9 тиков), то есть «маркер пришёл» не спутан с
«страница умерла».

### Остаток (не входило в починку)

* **Заголовочная форма `Link: <…>; rel=preload`** (перезамер срезом 27 ниже) —
  не сделана: заголовки ответа документа до JS-стороны не доходят, это отдельная
  работа в шелле. Заведена как **[BUG-906](BUG-906-OPEN.md)**.
* **Preload-кэш.** Тела не переиспользуются: `<link rel=preload as=script>` плюс
  последующий `<script src>` того же URL — два запроса. Исключение —
  `modulepreload` с JS-телом (module map). Заведено как **[BUG-907](BUG-907-OPEN.md)**.
* **Ранний старт.** Запрос уходит на `readyState='interactive'`, а не из
  streaming-сканера; выигрыш по параллелизму, ради которого сканер писался, не
  получен. Часть BUG-907.
* **Resource Timing** по этим запросам по-прежнему пуст
  ([BUG-839](BUG-839-OPEN.md)), поэтому тесты вида `modulepreload.html` и
  `dynamic-adding-preload.html`, которые считают записи
  `performance.getEntriesByName`, теперь падают на этой проверке вместо того,
  чтобы висеть.
* `rel=preconnect`/`dns-prefetch` по-прежнему ничего не открывают заранее —
  событий они и не должны слать, так что видимого дефекта нет.
