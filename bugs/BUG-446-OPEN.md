# BUG-446 — граф модулей страницы не подгружается: `import './helper.js'` со страницы всегда «module not found»

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/main.rs` — `run_scripts_with_dom`, `collect_scripts_ordered`, `resolve_script_sources`), js (`crates/js/src/v8_esm.rs` — реестр исходников)
**Найден:** P1, срез P3-v8-s12b-23 (2026-07-29), при сдаче [BUG-350](BUG-350-FIXED.md)

## Симптом

Инлайновый или внешний `<script type="module">`, который импортирует другой
модуль по URL, падает:

```
module error: JS runtime error: module 'file://.tmp/nope.js' not found
```

Воспроизведение (`target/dev-release/lumen.exe --dump-layout page.html`):

```html
<p id=o>before</p>
<script type="module">
  import { x } from "./helper.js";   // helper.js лежит рядом, но не грузится
  document.getElementById("o").textContent = x;
</script>
```

Текст остаётся `before`. Импорт зарегистрированного модуля при этом работает —
`V8JsRuntime::register_module_source` исправен и покрыт тестами
(`v8_esm.rs::v8_eval_module_imports_registered_module`), просто его никто не
зовёт из шелла.

## Причина

Разрешение спецификаторов и загрузка модуля — разные вещи, и вторая не
реализована ни на одном движке.

`run_scripts_with_dom` (`main.rs`) получает уже загруженные **тела** модульных
скриптов (`Vec<String>`), полученные из `resolve_script_sources`, и прогоняет
каждое через `rt.eval_module(src)`. Ни `register_module_source`, ни какая-либо
подгрузка импортируемых модулей нигде не вызываются (`grep -rn
register_module_source crates/shell` — ноль совпадений). Поэтому реестр
исходников на JS-потоке всегда пуст, и `ResolveModuleCallback` честно бросает
`module '<resolved>' not found` на первом же `import` в графе.

Следствия помимо самой загрузки:

1. **Внешние модульные скрипты теряют свой URL.** Тело `<script type="module"
   src="app.js">` вычисляется как инлайновое — под виртуальным спецификатором
   `lumen://inline-N`, — поэтому его относительные импорты и `import.meta.url`
   разрешаются относительно URL **страницы**, а не `app.js`. При соседних
   файлах разницы нет, при `src="js/app.js"` — есть.
2. Импорт-мапа работает только как переименование: спецификатор превращается в
   URL, который затем всё равно некому загрузить.

Это не регрессия V8-порта: на rquickjs-пути `LumenLoader` точно так же читал
только предзаполненный `ModuleRegistry`, а шелл его не заполнял.

## Масштаб

Все страницы, где модуль импортирует модуль. Прямо задевает WPT: 80
вендоренных `tests/wpt/**/*.html` используют `type="module"`, и типичный
шаблон там — `import {...} from "./resources/....js"`. `graphic_tests/`
модульных скриптов не содержит вовсе, так что графический гейт этого не ловит.

## Предлагаемый фикс

Предзагрузить граф модулей в шелле, до вычисления (сеть на JS-потоке внутри
V8-callback не нужна и опасна — `ResolveModuleCallback` синхронный):

1. Публичный сканер статических спецификаторов в `lumen-js` (лексер в
   `import_attributes.rs` уже умеет отличать `import` от строк/комментариев/
   шаблонов — переиспользовать его, а не писать второй).
2. В шелле после `resolve_script_sources` пройти BFS: для каждого модульного
   тела вытащить спецификаторы, разрешить через `esm::resolve_specifier_with`
   (page URL + import-мапа), загрузить тем же синхронным путём подресурсов, что
   и `<script src>`, зарегистрировать `register_module_source(resolved, body)`,
   повторить для новых тел. Ограничить глубину/число, чтобы кривая страница не
   зациклила загрузку.
3. Заодно провести URL внешнего модульного скрипта до места вычисления
   (`ScriptSource::External` знает `src`): зарегистрировать его тело под
   разрешённым URL и вычислять как `import '<url>';`, чтобы относительные
   импорты и `import.meta.url` считались от него, а не от страницы.

Владелец — P1/P3 (шелл + js). После фикса имеет смысл перепроверить
вендоренные WPT-категории с `type="module"`.
