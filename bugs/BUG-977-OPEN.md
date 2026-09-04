# BUG-977: `overflow: clip`, установленный тем же синхронным скриптом, не зануляет уже запрошенный скролл в ЖИВОМ окне

**Статус:** OPEN (ДОРАБОТКА → [CSSOM-7](../ROADMAP.md))
**Тип:** ДОРАБОТКА — симметричный синхронный style/layout-флаш для живого (не headless) шелла, тот же класс задачи, что CSSOM-4/BUG-493, но для другого исполнителя флаша.
**Компонент:** js (`crates/js/src/v8_runtime/install/platform.rs::install_scroll_state` — `_lumen_request_scroll`'s `is_clip` check) / js (`crates/js/src/v8_runtime/style_flush.rs::FlushHandles::maybe_flush`)
**Найден:** P3, 2026-09-04, при живой проверке фикса [BUG-975](bugs/BUG-975-OPEN.md) части 2 (`tests/wpt/verify_bug504_vertical_rl_clip.py`, интерактивное окно `--mcp-live-port`).

## Симптом

В интерактивном окне (не в headless-драйвере `InProcessSession`) скрипт,
который в ОДНОМ синхронном ходе сначала переключает `overflow` контейнера
на `clip`, а затем сразу запрашивает/читает скролл того же контейнера, не
видит зануления — `_lumen_request_scroll` применяет запрошенное значение
как есть:

```js
scroller.style.overflow = 'clip';
scroller.scrollTo(-60, 70);
console.log(scroller.scrollLeft, scroller.scrollTop); // печатает -60 70, ожидается 0 0
```

Живой прогон `verify_bug504_vertical_rl_clip.py` после фикса BUG-975 части
2 (2026-09-04): первый шаг (`scrollTo` на `overflow:hidden`, ДО всякого
`clip`) теперь корректно даёт `[-40, 50]`, но три последующих —
`scrollTo`/`scrollBy`/прямое присваивание `scrollLeft`/`scrollTop` ПОСЛЕ
`scroller.style.overflow = 'clip'` в том же ходе — отдают запрошенные (не
занулённые) значения:

```
after_overflow_clip:       [-40, 50]   (ожидание [0, 0])
after_scrollTo_on_clip:    [-60, 70]   (ожидание [0, 0])
after_scrollBy_on_clip:    [-70, 90]   (ожидание [0, 0])
after_direct_assign_on_clip: [-25, 35] (ожидание [0, 0])
```

**Это не регрессия BUG-975 части 2.** До неё `scroll_states` в этом
конкретном (полностью синхронном, без единого relayout) репро был вообще
не заполнен ни для одного узла — все пять чтений отдавали дефолтный `[0,
0]`, и четыре «clip»-проверки проходили по совпадению ожидаемого с
дефолтным, а не потому что зануление реально сработало (см. BUG-975-OPEN.md,
срез 2026-09-04: «без фикса тоже 4 из 5»). Часть 2 чинит только вставку
отсутствующей записи (BUG-975's узкий остаток); она не трогает и не могла
трогать is_clip-проверку — просто впервые сделала возможным получить
НЕ-дефолтное значение на этом репро вообще, чем и обнажила данный
независимый дефект.

## Причина

`_lumen_request_scroll`'s зануление по `overflow: clip` читает
`computed_styles` (тот же `Arc`, что `FlushHandles::maybe_flush`
пересобирает синхронно — но **только в headless-контексте**). В
интерактивном окне `FlushHandles::stylesheet` никогда не заполняется
(`crates/js/src/v8_runtime/style_flush.rs`, doc-комментарий модуля:
«The interactive shell never calls `update_stylesheet`, so
`FlushHandles::stylesheet` stays `None` there and
`FlushHandles::maybe_flush` is a no-op... extending coverage to the shell
needs its own stylesheet-push call site... tracked as a follow-up slice,
not attempted here» — это уже известный и осознанно отложенный пробел
CSSOM-4/BUG-493, просто до сих пор не задокументированный как отдельный
баг с конкретным репро).

Вместо синхронного пересчёта, `computed_styles` в живом окне обновляется
только асинхронно, после того как реальный relayout завершится
(`crates/shell/src/relayout.rs:690`, `js.update_computed_styles(styles)`) —
то есть на СЛЕДУЮЩЕМ тике event loop, не в том же синхронном ходе, где
скрипт поменял `style.overflow`. Пока этот тик не прошёл, `is_clip`
внутри `_lumen_request_scroll` видит СТАРОЕ значение `overflow` (или
вообще пустой кэш, если релэйаута ещё не было ни одного) и пропускает
зануление.

## Масштаб

Затрагивает любой скрипт в интерактивном окне, который переключает
`overflow` на `clip`/`hidden`/обратно и синхронно (без ожидания кадра)
проверяет/меняет скролл того же элемента — не специфично для
`vertical-rl` или для этого конкретного WPT-файла, тот является лишь
случаем, в котором дефект обнаружен.

## Предлагаемая правка (не сделана)

Симметричный аналог CSSOM-4 (BUG-493) для интерактивного окна: нужен
собственный синхронный путь пересчёта style/computed_styles для
живого шелла (или хотя бы точечный пересчёт `overflow-x`/`overflow-y` для
узла, участвующего в `_lumen_request_scroll`) — заранее очерченный как
отдельная работа в `style_flush.rs`'s doc-комментарии (решение по
dark-mode/forced-colors/web-fonts thread-locals, риск дедлока с движковым
потоком при вызове из нативы). Не точечный P3-фикс.

## Ревизия P3 2026-09-04

Проверена и отклонена дешёвая альтернатива предложенной правке: читать
инлайн `style`-атрибут узла напрямую из `flush.doc` (живой DOM,
`Arc<Mutex<lumen_dom::Document>>`), минуя `computed_styles`, раз симптом
специфичен именно для `el.style.overflow = '...'`. Раскопка показала, что
`style`-атрибут в Rust-структуре узла — сырая строка, уже
JS-сериализованная шимом (`_lumen_serialize_style`,
`crates/js/src/shim/web_api_shim_mid.js:1440`), а Rust-парсера одиночного
инлайн-объявления в `lumen_css_parser` нет — пришлось бы либо дублировать
JS-логику разбора на Rust, либо звать обратно в JS. Хуже: такое чтение
видело бы только инлайн-мутацию и молча пропустило бы `overflow: clip`,
пришедший из CSS-класса/внешнего листа — второй, рассогласованный с
`computed_styles` источник истины вместо архитектурного фикса. Переведён в
ДОРАБОТКА → [CSSOM-7](../ROADMAP.md); дедлока между потоками при чтении
`doc` из этой native-функции не обнаружено (`route_query_js` уже дропает
`doc_guard` до пересечения на JS-поток, `crates/shell/src/relayout.rs:676`),
это не было препятствием.

## Repro

```bash
python tests/wpt/verify_bug504_vertical_rl_clip.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
```

Первая проверка (`after_scrollTo_hidden`) теперь зелёная (BUG-975 часть 2);
оставшиеся четыре — этот баг.

## Residual found working BUG-506 (P3, 2026-09-04)

Тот же no-op `FlushHandles::maybe_flush` на `--bidi-port`-пути бьёт не
только `_lumen_request_scroll`'s `is_clip`-проверку — общая
`getComputedStyle()`/`HTMLStyleElement.sheet` под живым окном ловят его
тоже, без единого upstream'а `overflow`/`clip`. Живой прогон 5 файлов
`css/css-logical` из [BUG-506](BUG-506-OPEN.md) (реальный wptrunner-
пайплайн, `LumenTestharnessExecutor` через `--bidi-port`) показывает:
`addDiv(t)`-вставленный `<div>`, прочитанный тем же ходом через
`getComputedStyle`, отдаёт `""` для любого свойства (не только
геометрии) — `computed_styles` не содержит записи для узла вовсе, тот же
механизм, что и здесь, только через другой native (`_lumen_get_computed_
style` вместо `_lumen_request_scroll`). Второй вход в ту же дыру:
`extraStyle.sheet` (`HTMLStyleElement.sheet`) остаётся `null` сразу после
`document.head.appendChild()` тем же ходом — `testcommon.js`'s
`addStyle()` падает `TypeError: Cannot read properties of null (reading
'insertRule')`. Подтверждает, что CSSOM-7 закрывает не только
scroll/clip-кейс, а весь класс «живой шелл не флашит стиль/layout
синхронно перед JS-чтением» — см. также остаточную запись в
[BUG-493](BUG-493-OPEN.md).
