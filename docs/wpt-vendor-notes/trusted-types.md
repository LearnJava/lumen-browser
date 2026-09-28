# WPT vendor notes — `trusted-types`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/trusted-types/`, 339 файлов: `META.yml`, root-level конструктор/атрибутные/CSP-тесты, `resources/`, `support/`). Прогон `run_report.py --all --root trusted-types --recursive`, ~24 мин, 232 отобранных id: **98/230 harness OK, 576/2465 сабтестов**. Заведён [BUG-689](../../bugs/BUG-689-FIXED.md): весь `Attr`/`NamedNodeMap`-подраздел DOM §4.9 отсутствует — `Element.attributes` не `NamedNodeMap` (`undefined`), `document.createAttribute()`/`createAttributeNS()` и `getAttributeNode(NS)`/`setAttributeNode(NS)` бросают `TypeError` — известный по формулировке [BUG-309](../../bugs/BUG-309-FIXED.md) («no Attr node objects») пробел, ранее нигде не заведённый отдельным номером; 447× `Cannot read properties of undefined (reading 'length')` (общий хелпер `support/attributes.js::findAttribute` итерирует `element.attributes.length`) + 324× `createAttributeNS is not a function` доминируют в сигнале прогона, объясняя большинство из 1889 unexpected сабтестов. Второстепенный сигнал — реконфирмация: 12×/10× `document.write`/`.writeln is not a function` ⇒ уже открытый [BUG-568](../../bugs/BUG-568-FIXED.md); 19 `.https.`-файлов TIMEOUT на документированном TLS-гэпе `UnknownIssuer` (тот же класс, что `top-level-storage-access-api`/`shape-detection`). Один новый номер бага, подтверждён живой пробой (`--mcp-live-port`)

## BUG-689 закрыт (2026-09-28, P3)

`document.createAttribute(NS)` и Attr-узлы с идентичностью появились — общий хелпер
`support/attributes.js` больше не падает на первом `element.attributes.length`. На 16
файлах, трогающих Attr-API, 432→744/1192 сабтестов; их `.ini` переписаны ратчетом
(список — `grep -l` по `createAttribute|AttributeNode|NamedItem|support/attributes.js`
плюс хелперы `*_set_ns` из `support/helper.sub.js`). Остальные 218 файлов категории
не перепрогонялись: полный `--update-expected` подтягивал и чужой дрейф (worker-тесты
TIMEOUT→FAIL), который к этой правке не относится. SVG `<script href>` не считается
sink'ом — было [BUG-1213](../../bugs/BUG-1213-FIXED.md), fixed 2026-09-29.
