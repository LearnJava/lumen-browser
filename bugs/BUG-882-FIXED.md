# BUG-882 — `<script>`, однажды подключённый с не-JS `type`, больше не исполняется никогда: перетипизация и повторная вставка ничего не дают

**Статус:** FIXED 2026-09-24 (P3)
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, вариант `script-reinsert`)
**Область:** `crates/js/src/shim/web_api_shim_mid.js` (`_lumen_script_prepare`, `_lumen_resource_try_prepare`) — с рефакторингом dom.rs/SPLIT ветка «не-JS тип» переехала сюда; на момент заведения флага «already started» (HTML LS §4.12.1) в шиме не было вовсе, элемент считался обработанным навсегда после первого прохода через `_lumen_resource_pending`, независимо от того, реально ли алгоритм дошёл до шага 12
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var s = document.createElement('script');
s.type = 'importmap';          // не-JS тип
document.head.appendChild(s);
document.head.removeChild(s);
s.type = 'text/javascript';
s.innerText = "…";
document.head.appendChild(s);  // не исполняется
```

Тело не исполняется ни в случае, когда элемент был подключён с **пустым**
importmap-содержимым (спека требует исполнения: флаг «already started»
ставится только для непустого), ни когда с непустым (тут молчание
случайно совпадает со спекой). Свежесозданный `<script>` с тем же телом
исполняется нормально, то есть дело именно в повторной подготовке
однажды подключённого элемента.

## Прямое измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant script-reinsert`
(2026-08-23, dev-release, Linux, `main` = `34cbefd25`):

```
sr-empty-reappended type=text/javascript
sr-nonempty-reappended
sr-plain-ran            ← контроль: обычный созданный скрипт исполняется
sr-plain-appended
sr-checked
```

Ни `sr-empty-retyped-ran`, ни `sr-nonempty-retyped-ran` не напечатаны.

## Цена по WPT

`import-maps/dynamic-module-map-key.html` — файл держит сабтест `The Already
Started flag is set when a non-empty <script> tag is connected.`: он ждёт
`done()` ровно из такого перетипизованного скрипта. Отдельно стоит отметить,
что к самим import maps этот сабтест отношения не имеет — соседний дефект
карты (`[BUG-879](BUG-879-OPEN.md)`) в этом файле до дела не доходит.

## Что дальше

HTML LS §4.12.1 «prepare a script» держит на элементе флаг «already
started», выставляемый только при фактическом старте, и заново готовит
элемент при каждом «becomes connected». Нужны оба шага; сейчас нет ни
флага, ни повторной подготовки.

## Срез 1 (P3, 2026-09-24): флаг «already started» и повторная подготовка при реконнекте

**Фикс:** `_lumen_resource_pending` (карта «однажды подключённый элемент
ещё не обработан») подходит для `<link>`/`<track>`/`<source>`/`<style>`/
`<embed>`/`<object>` — их фактически можно приготовить/загрузить только
один раз, поэтому запись безусловно удалялась при первом проходе через
`_lumen_resource_try_prepare`. Для `<script>` это неверно: шаг 5 алгоритма
(«нет `src` и текст пуст — return, флаг не трогать») может завернуть
элемент, не выставив «already started», и по спеке он обязан быть
подготовлен заново при следующем «becomes connected» — ровно случай
`type="importmap"` без содержимого.

Добавлен отдельный персистентный флаг `_lumen_script_started` (nid → 1),
который `_lumen_script_prepare` выставляет сам, ровно на шаге 12 алгоритма
(после проверки src/тела и после того, как тип разрешился в
classic/module/importmap — «шаг 10» отсекает данные-блоки вроде
`application/json`, которые никогда не запускаются и никогда не
«стартуют»). `_lumen_resource_try_prepare` для `kind === 'script'` больше не
удаляет запись из `_lumen_resource_pending` безусловно — только когда
`_lumen_script_started[nid] === 1` действительно выставлен, иначе элемент
остаётся отслеживаемым и получает повторный вызов `_lumen_script_prepare`
при следующей вставке в дерево.

**Замер после фикса:** `tests/wpt/verify_callback_import_preload_gaps.py
--variant script-reinsert` печатает `sr-plain-ran` и `sr-empty-retyped-ran`,
не печатает `sr-nonempty-retyped-ran` — ровно ожидаемая по спеке комбинация
(пустой importmap не «стартовал», непустой — «стартовал» и остаётся
неисполняемым навсегда). Полный прогон файла (24 варианта) — без изменений
маркеров ни на одном из остальных, включая `importmap`/`importmap-absolute`/
`currentscript`, которые тоже проходят через `_lumen_script_prepare`.
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` чист.

Обработка самого содержимого import map (`{"imports": {...}}`) из
динамически вставленного `<script type=importmap>` по-прежнему не
реализована — к сабтесту заявки это отношения не имеет (см. «Цена по
WPT» выше), отдельная возможность.
