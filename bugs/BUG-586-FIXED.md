# BUG-586 — `document.domain` not implemented — getter is `undefined`, setter never validates or throws

**Статус:** FIXED 2026-09-15
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js`, `crates/js/src/shim/web_api_shim_mid_b.js`)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом (исходный)

```
assert_equals: document.domain is a string expected "string" but got "undefined"
assert_throws_dom: function "() => { document.domain = document.domain }" did not throw
assert_throws_dom: function "() => { document.implementation.createHTMLDocument().domain = document.domain }" did not throw
```

## Уточнение при разборе

Исходное заведение бага предполагало, что `document.domain = document.domain`
обязан бросать `SecurityError` — это неверно: перечитка HTML LS
§"relaxing the same-origin restriction" и живого теста
`tests/wpt/html/browsers/windows/resources/document-domain-setter.html`
(`document.domain = document.domain;`, используется как штатный кросс-фреймовый
хелпер и не должен бросать) показала обратное — присвоение того же значения
эффективному домену является штатной идиомой релаксации и обязано молча
успевать. Путь `html/browsers/origin/inheritance/document-domain-*.html`,
на который ссылался исходный багрепорт, в текущем вендоринге WPT отсутствует;
реальное покрытие — `webstorage/document-domain.html`,
`html/browsers/windows/document-domain-nested*.window.js`,
`xhr/send-after-setting-document-domain.htm` и смежные.

## Причина

Свойства `document.*` в Lumen собираются как обычный JS-объект-литерал в шиме
(`crates/js/src/shim/web_api_shim_mid.js`), а не как нативные V8-аксессоры —
паттерн уже виден на `title`/`cookie`/`URL`. `domain` в этом литерале просто
не было объявлено вовсе (ни на живом `document`, ни в
`_lumen_build_detached_document`, используемом `createHTMLDocument`/
`createDocument`), поэтому чтение давало `undefined`, а запись создавала
обычный expando-property без какой-либо проверки.

## Фикс

- `crates/js/src/shim/web_api_shim_mid_b.js`: добавлена переменная
  `_lumen_document_domain`, отдельная от `_lumen_loc_parts.hostname` —
  инициализируется хостом страницы и расходится с ним в момент релаксации
  домена (`location` обязан по-прежнему отдавать реальный хост).
- `crates/js/src/shim/web_api_shim_mid.js`: на живом `document` — геттер
  `domain`, отдающий `_lumen_document_domain`, и сеттер, реализующий
  упрощённый алгоритм релаксации (HTML LS): опаковый origin
  (`!_lumen_loc_parts.hasAuthority`) → `SecurityError`; новое значение,
  совпадающее с текущим эффективным доменом, — не-op успех; новое значение,
  являющееся его registrable-суффиксом (`effective` кончается на
  `"." + newValue`) — принимается; всё остальное — `SecurityError`. На
  detached-документе (`_lumen_build_detached_document`, используется
  `createHTMLDocument`/`createDocument`) — геттер, всегда отдающий `''`, и
  сеттер, всегда бросающий `SecurityError` (нет browsing context).
- Публичный список публичных суффиксов (public suffix list) не подключён —
  проверка ограничена структурным суффиксным сравнением с точечной границей,
  без знания, что `"com"` сам по себе публичный суффикс и не должен приниматься
  как цель релаксации. Не встречено ни в одном из найденных вендоренных тестов;
  если понадобится — отдельный срез.

### Проверка фикса

`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
чист (правка только в `.js`-шимах, Rust не тронут). Живая проба через
`lumen --dump-layout` с инлайн-скриптом (`console.log` внутри try/catch):

```
REPORT typeof_domain = "string"
REPORT initial_domain = "<host>"
REPORT set_same_value = "<host>"                 // не бросает
REPORT set_unrelated THREW SecurityError: ...     // бросает на несуффиксе
REPORT detached_domain = ""
REPORT detached_set_throws THREW SecurityError: ...
```

Все шесть срезов ведут себя по спеке; `scripts/scoped-test.sh` до `lumen-js`
не доходит из-за независимого сломанного гейта [BUG-805](BUG-805-OPEN.md)
(`lumen-network` виснет/осыпается) — не связано с этой правкой.

## Осознанно не сделано

Реальная кросс-фреймовая релаксация origin (два поддомена становятся
same-origin после того, как оба выставили одинаковый `document.domain`,
что открывает синхронный доступ к DOM друг друга через `window`/`contentWindow`)
не проверялась и не трогалась — она требует межконтекстной проверки origin в
WindowProxy/security-check слое, которого этот срез не касается. Вендоренные
`document-domain-nested*.window.js` тесты именно это и проверяют и остаются
непройденными за пределами голого getter/setter-контракта, зафиксированного
здесь.
