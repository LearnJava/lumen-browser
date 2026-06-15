# BUG-164

**Статус:** OPEN
**Компонент:** shell / js
**Файл:** `crates/shell/src/main.rs:3602` (`collect_inline_scripts`)

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
