# BUG-459: внешний `<script type=module src>` теряет свой реальный URL — относительный/динамический `import()` из него резолвится от адреса страницы, а не от адреса скрипта

**Статус:** OPEN
**Дата:** 2026-07-30
**Компонент:** js (V8 ESM, `crates/js/src/v8_esm.rs` + вызывающий код в `crates/shell/src/main.rs`)
**Найден:** P1 при проверке [BUG-303](BUG-303-FIXED.md) (github.com JS-hang)

## Симптом

На github.com каждый top-level `<script type="module" src="https://github.githubassets.com/assets/X.js">`
(это внешний, абсолютный URL) при попытке любого `import`/динамического
`import()` изнутри своего тела резолвит спецификатор **от адреса страницы**
(`https://github.com/…`), а не от собственного `src`:

```
stderr: module error: JS runtime error: module 'https://github.com/wp-runtime-b5235be89f8471a4.js' not found
```

— обратите внимание на домен: `github.com`, а не `github.githubassets.com`,
хотя реальный скрипт лежит на втором.

## Причина

`crates/shell/src/main.rs` собирает тела внешних `<script type=module src>`
(уже зафетченные, HTML LS §8.1.3.1) в один вектор `module_scripts: Vec<String>`
вперемешку с телами инлайновых `<script type=module>` — реальный `src`-URL
теряется на этом шаге (main.rs:7190 и далее, оба v8/quickjs блока: main.rs:7328
для V8, main.rs:7246 для QuickJS вызывают `rt.eval_module(src)` только с
текстом, без спецификатора).

`V8JsRuntime::eval_module` (`crates/js/src/v8_runtime.rs:4567`) передаёт текст
в `v8_esm::evaluate_entry_module` (`crates/js/src/v8_esm.rs:408`), который явно
документирован как «entry ES module of an **inline** `<script type=module>`» —
он регистрирует источник под виртуальным `lumen://inline-N`
(`register_inline`, v8_esm.rs:114) и назначает базой для относительных
импортов `page_url` (см. `resolve_specifier_with` fallback). Для инлайновых
скриптов это корректно (у них и правда нет собственного URL), но тот же путь
сейчас используется и для **внешних** модулей, у которых `src` — известный
абсолютный URL и обязан быть базой для их собственных `import()`/relative
imports (ровно как в браузерах: модуль-скрипт резолвит импорты от своего
собственного URL, HTML LS §8.1.3.8).

## Почему это не подчистило BUG-303 целиком

BUG-303 был про зависание (не про корректность JS) — оно исчезло как
побочный эффект переписывания ESM-стека на V8 (S12b-23). Эта находка —
самостоятельный, не связанный с зависанием дефект резолвинга, который делает
webpack-сплиттинг (динамический `import()` чанков) битым на любом сайте,
использующем internal chunk-loading по относительному URL от своего скрипта
(webpack, Vite, любой bundler с code-splitting) — то есть заметная доля
современных SPA.

## Предлагаемый фикс

1. Прокинуть реальный `src`-URL внешних `<script type=module>` от
   `main.rs` (где он уже есть при фетче — см. комментарий на main.rs:7193
   «module_scripts receives … bodies (BUG-164)») до `eval_module`: либо
   новый метод `eval_module_external(specifier: &str, source: &str)` в
   `JsRuntime`, либо `Vec<(Option<String>, String)>` вместо `Vec<String>` для
   `module_scripts`.
2. В `v8_esm.rs` — путь, который для известного `specifier` делает то же, что
   `evaluate_entry_module`, но **не** генерирует `lumen://inline-N`, а
   регистрирует+компилирует источник сразу под реальным URL (переиспользовать
   `register_source` + `module_for`/`load_and_evaluate`, только с другой
   стартовой точкой, не `register_inline`).
3. `import.meta.url` внешнего модуля должен стать его собственным `src`
   (сейчас, судя по документации `evaluate_entry_module`, у инлайновых он =
   `page_url` — так и должно остаться только для инлайновых).
4. QuickJS-путь (`main.rs:7246`) **не трогать** — движок в процессе полного
   сноса (CLAUDE.md: «никогда не таргетить фиксы на rquickjs»).

## Тест

Регресс-тест: внешний модуль А (зарегистрирован под `https://cdn.test/a.js`)
делает `import('./b.js')` — должен резолвиться в `https://cdn.test/b.js`,
не в `<page-url>/b.js`. Аналогично для `import.meta.url` внутри А.
