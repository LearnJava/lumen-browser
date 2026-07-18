# BUG-292 — Адблок блокирует топ-левел навигацию: узкое easylist-правило с regex накрывает любой голый домен `<слово≥5>.com`

**Статус:** FIXED 2026-07-18
**Компонент:** network (`crates/network/src/filter/easylist.rs`, `crates/network/src/lib.rs` — filter context) + `lumen-core::ext::RequestContext`
**Найден:** 2026-07-16, при ручном открытии `https://example.com` во второй вкладке (через `window.open()`)

## Симптом

Навигация верхнего уровня на `https://example.com` блокируется собственным адблоком:

```
✗ https://example.com/ (easylist)
Ошибка загрузки https://example.com: network error: blocked: easylist
```

Затронут не только example.com: правило (см. ниже) совпадает с **любым** голым доменом
`https://<5+ букв/цифр>.com/` без поддомена — `github.com`, `google.com` и т.п. (с `www.`-префиксом
не совпадает, поэтому часть сайтов «спасает» редирект). Блокируется именно главный документ —
вкладка показывает ошибку загрузки.

## Причина

Совпавшее правило easylist (единственное, при исключённом host-индексе; проверено моделированием
матчера по скачанному `data/adblock/lists/easylist.txt`):

```
/^https?:\/\/[0-9a-z]{5,}\.com\/.*/$script,third-party,xmlhttprequest,domain=1cloudfile.com|…(~140 стриминговых сайтов)
```

В uBlock/ABP это правило узкое: только скрипты/XHR, только third-party, только на перечисленных
в `domain=` сайтах. В Lumen все три ограничения снимаются одновременно:

1. **Нет типа ресурса «главный документ».** `ResourceType` (`lumen-core::ext`) моделирует только
   субресурсы (`script/image/…/subdocument/other`); у топ-левел навигации
   `resource_type: None` (`crates/network/src/lib.rs`, `filter_ctx`). А `RuleOptions::matches`
   трактует неизвестный контекст как «удовлетворяет любые ограничения» (осознанный
   conservative-block, `easylist.rs`). В ABP-семантике правило с типовыми опциями (`$script,xhr`)
   **не должно** применяться к main-frame документу вообще — документ блокируют только явные
   `$document`-правила.
2. **`third-party` тоже снимается:** у топ-левел навигации `top_level_site` нет →
   `third_party: None` → ограничение удовлетворено.
3. **`domain=` — parsed-and-ignored:** правило сознательно не сужается на немоделируемый
   модификатор (принцип «no over-allow»), что для документной навигации превращается
   в масштабный over-block.

Каждый пункт по отдельности — задокументированный консерватизм; вместе они дают блокировку
обычных сайтов на дефолтных подписках.

## Repro

1. `cargo run -p lumen-shell -- https://example.com` (подписки easylist должны быть скачаны —
   происходит автоматически при старте).
2. Вкладка показывает «Ошибка загрузки … blocked: easylist».
3. Быстрая проверка правила без браузера: regex `^https?:\/\/[0-9a-z]{5,}\.com\/.*` против
   `https://example.com/`.

## Что нужно для закрытия

Минимально: ввести понятие документного запроса в фильтр-контекст (например,
`ResourceType::Document` или явный флаг `is_top_level` в `RequestContext`) и в ABP-семантике
**не применять** к нему правила с типовыми `$`-опциями (документ блокируется только явным
`$document`, которого в модели пока нет — до тех пор топ-левел навигацию корректно вовсе не
фильтровать типизированными правилами). Отдельно оценить: правила с `domain=` для документной
навигации — кандидат на «не применять» вместо «игнорировать модификатор» (пересмотреть
trade-off no-over-allow против over-block). Регрессионный тест: `https://example.com`
(или любой `<слово>.com`) с реальным easylist-правилом выше должен открываться; блокировка
`$script`-субресурса с matching-доменом — сохраниться.

## Исправление (2026-07-18, P3)

Введён явный признак навигации верхнего уровня вместо моделирования `$document`:

1. `lumen-core::ext::RequestContext` получил поле `is_top_level: bool` (default `false`
   через `Default`; `unknown()` даёт `false`).
2. `crates/network/src/lib.rs` `fetch_with_redirect` протянут параметром `is_top_level`.
   Он `true` только у top-level-путей — `fetch_page`, `fetch_page_streaming` и
   `NetworkTransport::fetch` (последний используется для скачиваний/standalone-HTML, не для
   субресурсов страницы — те идут через `fetch_subresource`), и распространяется по
   redirect-hop'ам (редиректнутая навигация — та же навигация). Все субресурсные/CORS/range/
   conditional/JS-`fetch`-пути передают `false`. Значение кладётся в `filter_ctx.is_top_level`.
3. `RuleOptions::matches`: при `ctx.is_top_level && self.types.is_some()` правило НЕ
   применяется — по ABP-семантике типовые `$`-опции описывают только субресурсы, а главный
   документ блокирует лишь явный `$document` (в модели не выражен как type-bit). Untyped-
   правила (`||host^`, без type-опций) по-прежнему покрывают документ — осознанная доменная
   блокировка сохраняется.

`domain=` намеренно оставлен parsed-and-ignored: type-guard уже снимает over-block
у реального правила (у него есть `$script,xhr`), а трогать trade-off `no-over-allow`
для чисто `$domain=`-правил — отдельный вопрос вне этого фикса.

Регрессия: `crates/network/src/filter/easylist.rs::tests::`
`top_level_navigation_not_blocked_by_typed_rule` (example.com/github.com не блокируются
реальным regex-правилом с `$script,xhr`; `$script`-субресурс всё ещё блокируется) и
`top_level_navigation_still_blocked_by_untyped_rule` (`||malware.net^` блокирует документ).
