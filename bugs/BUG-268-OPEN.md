# BUG-268

**Статус:** OPEN
**Компонент:** shell (загрузчик стилей) + css-parser (media)
**Симптом:** на страницах, подключающих отдельный print-таблицу стилей через `<link rel="stylesheet" media="print" href="...">`, экранный рендер применяет её правила. На `www.w3.org` это `a::after { content: " <" attr(href) ">" }` — после каждой ссылки печатается её URL в угловых скобках («standards and guidelines <https://www.w3.org/standards/>»). Страница выглядит как print-версия.

---

## Первопричина

Каскад **правильно** отсекает блоки `@media print` внутри таблицы: `MediaContext.media_type` = `"screen"` (`crates/engine/css-parser/src/parser.rs:1366`), а `MediaCondition::MediaType` матчится как `t == "all" || t == ctx.media_type` (:1426) — `print` не совпадает со `screen`. Тест `crates/driver/tests/cases/test_44.rs:60` это подтверждает.

Дыра **не в каскаде, а в загрузчике шелла**: `collect_link_hrefs()` (`crates/shell/src/main.rs:3573–3598`) собирает href всех `<link rel=stylesheet>` **без проверки атрибута `media`**:
```rust
if rel.split_ascii_whitespace().any(|r| r.eq_ignore_ascii_case("stylesheet"))
    && !href.is_empty()
{
    out.push(href.to_owned());   // media не смотрим
}
```
Скачанный print-only лист целиком вливается в каскад. Его правила НЕ обёрнуты в `@media print` (весь лист «print» за счёт атрибута `media` на `<link>`), поэтому каскад их не фильтрует — media-гейт на уровне `<link>` потерян.

## Как чинить

1. В `collect_link_hrefs()` (и в стриминговом `feed_preload_and_emit`, где собираются CSS-хинты) читать атрибут `media` у `<link>`. Если он есть и не матчит текущий экранный контекст — **не** добавлять лист в экранный каскад.
2. Матчинг переиспользовать из css-parser: media-строку `<link>` парсить тем же парсером media-query и проверять против `MediaContext { media_type: "screen", .. }` (тот, что строит `media_context_from_viewport`). Не писать второй матчер — вынести/вызвать существующий.
3. `media` отсутствует или `media="all"`/`media="screen"` (и подходящие `@media`-фичи вроде `min-width`) → грузить как сейчас.
4. **Печать (см. также BUG вокруг print-pipeline):** при генерации PDF (`do_print_to_pdf`) контекст должен быть `media_type: "print"`, тогда print-листы наоборот включаются. Сейчас print-путь тоже жёстко `"screen"` (`style.rs:17975`) — печать не применяет `@media print` вовсе. Правку media-гейта делать так, чтобы `MediaContext` передавался параметром, а не хардкодился, — и экранный загрузчик, и print-пайплайн используют один и тот же гейт с разным `media_type`.

## Валидация

- Репро: `lumen --screenshot w3.png https://www.w3.org/` — URL в угловых скобках после ссылок должны исчезнуть.
- Юнит: `<link rel=stylesheet media=print>` не попадает в экранный набор листов; `media=screen`/`all`/без атрибута — попадает.
- Регресс: `python graphic_tests/run.py` без изменений (в тестах нет print-листов, но проверить, что обычные `<link>` грузятся).
- Печать: PDF применяет `@media print` (проверить страницей с `@media print { body { color: red } }`).
