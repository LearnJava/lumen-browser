# BUG-164

**Статус:** FIXED 2026-06-15
**Компонент:** shell / js
**Файл:** `crates/shell/src/main.rs` (`collect_scripts_ordered` + `resolve_script_sources`)

## Исправление (2026-06-15)

Сбор скриптов разделён на два этапа. Новый `collect_scripts_ordered` обходит DOM
в порядке документа и классифицирует каждый `<script>` в classic/module списки,
записывая внешние `<script src>` как `ScriptSource::External` (а не пропуская их,
как делал `collect_inline_scripts`). `resolve_script_sources` дозагружает тела
внешних скриптов через subresource-фетчер (зеркало `load_linked_stylesheets`,
`RequestDestination::Script`, CORS/mixed-content как у CSS) и возвращает уже
разрешённые тела в порядке документа. `run_scripts_with_dom` теперь принимает
готовые `scripts`/`module_scripts` вместо внутреннего `collect_inline_scripts` и
прогоняет их через тот же QuickJS-путь (classic → module → extension).
`src` побеждает inline-тело (HTML LS §4.12.1); не-JS блоки (`importmap`,
`application/ld+json`, `application/json`, `speculationrules`, шаблоны) считаются
данными и не исполняются. То же дозагружается на пути восстановления вкладки из
hibernation (`tab_lifecycle/hibernate.rs`).

Регресс-тесты: `collect_scripts_ordered_records_external_in_order`,
`collect_scripts_ordered_external_module`, `collect_scripts_ordered_skips_non_js_types`,
`collect_scripts_ordered_src_wins_over_inline_body`,
`resolve_script_sources_passes_inline_through`. Функциональная проверка: локальная
страница с `<script src=ext_main.js>` → инъецированный `<p>` появляется в
display list (`Загружен скрипт: …ext_main.js`).

Остаётся вне рамок (как и помечено в задаче): полный рендер React-страниц требует
более широкого DOM/Web-API слоя — отдельная работа.

---

## Описание

Внешние `<script src="...">` не скачиваются и не исполняются. Сборщик
скриптов берёт только инлайновое тело `<script>` (текстовые дети узла) и
явно пропускает скрипты с атрибутом `src` (комментарий: *"Both skip
`<script src="...">` (external-only) and empty inline bodies"*).

Следствие: на сайтах, где логика вынесена в бандлы (почти все современные),
JS фактически не работает. На lenta.ru не скачиваются `owlBundle.js`,
`indexPageOwl.js`, `vendorsOwl.js`, `capirs_async.js` — в логе нет ни одного
`GET ...js`. Это первопричина BUG-163 (preload-картинки) и неполного рендера
SPA-страниц.

## Воспроизведение

```
./target/release/lumen.exe https://lenta.ru
```

В stderr есть `⤷ preload js [medium] ...owlBundle.js` (хинт), но нет
`→ GET ...owlBundle.js` (фактической загрузки) и нет исполнения.

## Что нужно

1. Собирать `<script src>` в порядке документа (с учётом `defer`/`async`,
   HTML LS §8.1.3.1 — порядок исполнения classic-скриптов).
2. Дозагружать тело через subresource-фетчер (как картинки/CSS),
   с учётом CORS/mixed-content.
3. Прогонять через тот же QuickJS-путь, что и инлайны (`run_scripts_with_dom`).

## Зависимости / контекст

- Парный к BUG-163: без внешних скриптов SPA-контент (и его `<img>`) не строится.
- Полноценный рендер React-страниц этим не закрывается — отдельно нужен
  более полный DOM/Web-API слой (текущие инлайны уже дают ошибки
  `no setter for property`, `not a function`, `expecting ';'`,
  `WebCodecs/SVG DOM API init failed`). Это отдельная большая работа.
