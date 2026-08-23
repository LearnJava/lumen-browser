# BUG-882 — `<script>`, однажды подключённый с не-JS `type`, больше не исполняется никогда: перетипизация и повторная вставка ничего не дают

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, вариант `script-reinsert`)
**Область:** `crates/js/src/dom.rs:6530` — ветка «не-JS тип (importmap, application/json, speculationrules, …)»: элемент помечается один раз и при последующем подключении заново не готовится; флага «already started» (HTML LS §4.12.1) в шиме нет
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
