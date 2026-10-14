# BUG-1616 — SameSite и разделение cookie не работают в браузере (top_level_site=None)

**Статус:** FIXED 2026-10-14 (P6) — SameSite; разбиение по сайту вынесено в [BUG-1628](BUG-1628-OPEN.md)
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** shell/network (`page_source.rs:266`, `resource_base.rs:206`, `network/src/lib.rs:2802`, `storage/src/cookies.rs:678`)

## Симптом и место в коде

Попутная находка (к пункту Storage Access). Единственный сеттер HttpClient::top_level_site — with_cookie_jar (crates/network/src/lib.rs:3810-3817), и все вызовы шелла передают None (page_source.rs:266, :394; resource_base.rs:206). В браузере из-за этого: is_cross_site всегда false (lib.rs:2802-2805), поэтому SameSite=Strict/Lax не применяются ни к одному запросу (cookies.rs:678-686); Total Cookie Protection выключена вопреки docs/plan/privacy.md:102 (Standard = TCP) и CAPABILITIES.md:211 («✅ SameSite, partitioning»); у EasyList third-party нет контекста, поэтому правило действует на все запросы (lib.rs:2626-2631: «None fields leave the rule unrestricted»). Отчёт §8.8 COMPAT-CRIT-07 («межсайтовый запрос отбрасывает все Lax-cookie», cookies.rs:681) описывает поведение библиотеки, а не браузера. Похожего открытого бага в BUGS.md нет.

## Где смотреть

- crates/shell/src/page_source.rs:266
- crates/shell/src/page_source.rs:394
- crates/shell/src/resource_base.rs:206
- crates/network/src/lib.rs:2802
- crates/network/src/lib.rs:2631
- crates/network/src/lib.rs:3816
- crates/storage/src/cookies.rs:678
- docs/plan/privacy.md:102
- CAPABILITIES.md:211

## Сюда же: SameSite и разбиение cookie не подключены в проде

В хранилище по умолчанию Lax: enum SameSite #[default] Lax (crates/storage/src/cookies.rs:58-60), при разборе Set-Cookie без атрибута тоже Lax (cookies.rs:475), из БД неизвестное значение читается как Lax (cookies.rs:78-79). SameSite=None без Secure отбрасывается (cookies.rs:757-760). Проверка в get_for_request (cookies.rs:673-686) при is_cross_site отбрасывает Strict и Lax, причём Lax режется и на top-level GET-переходе: комментарий прямо говорит «Phase 0 conservative», исключения Lax+POST нет. Однако is_cross_site вычисляется только из top_level_site (network/src/lib.rs:2802-2805), а все боевые клиенты получают with_cookie_jar(.., None): навигация (shell/src/page_source.rs:264-267, 392-395), subresource, fetch и XHR (resource_base.rs:204-207), document.cookie (js/src/v8_runtime/install/storage.rs:420-431). BUG-1146 прямо фиксирует: «в проде он везде None». Поэтому в проде SameSite и разбиение по сайту верхнего уровня (Total Cookie Protection) не работают вообще: все cookie, включая явные SameSite=Strict, уходят в любой межсайтовый subresource, iframe и POST. Фактически это семантика None для всех cookie. 3DS сегодня из-за SameSite не рвётся, зато защиты от CSRF через SameSite и от сторонних cookie нет. Неверны CAPABILITIES.md:211 («SameSite, partitioning» ✅) и строка «Lumen сейчас» в отчёте §8.8 COMPAT-CRIT-07. Для фикса пригодится with_document_context (хост документа-инициатора, BUG-1146). Целевая семантика cookie без атрибута — решение Р11 (по исследованию None).

## Объём и приёмка

Шелл передаёт top_level_site=None во все HttpClient (page_source.rs:266, :394, resource_base.rs:206), поэтому в живом окне is_cross_site всегда false (network/src/lib.rs:2802): SameSite=Strict/Lax не применяются (cookies.rs:678-686), разделение cookie выключено вопреки privacy.md:102 и CAPABILITIES.md:211, а правила EasyList с third-party без контекста действуют на все запросы (lib.rs:2626-2631). Фикс: передавать eTLD+1 вкладки и родителя фрейма; cookie без атрибута — по Р11 (None до корпуса 3DS). Приёмка: тест — Strict-cookie не уходит на межсайтовый подресурс, Lax уходит при верхнеуровневой навигации. docs/research/site-compat/report.md §8.8 COMPAT-CRIT-07

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.

## Исправлено (2026-10-14, P6)

SameSite теперь работает в живом окне; Total Cookie Protection сознательно оставлена выключенной и заведена отдельно ([BUG-1628](BUG-1628-OPEN.md)).

- **Сайт документа доходит до `HttpClient`.** Навигация (`page_source.rs` `initiator_site`) передаёт сайт инициатора из уже обработанного `Referer`; подресурсы, `fetch()`/XHR и iframe-документы (`resource_base.rs` `http_client_for_subresource_with_policy`) — сайт документа-владельца (`config::connection_site`, eTLD+1 по PSL). Для `file:`, `about:blank`, `data:` и навигации без инициатора (адресная строка, история, `no-referrer`) — `None`: запрос считается same-site.
- **Контекст запроса — трёхзначный** (`lumen_core::ext::CookieSiteContext`, `CookieProvider::get_for_request_in_context`): `SameSite`, `CrossSiteTopLevelSafe` (межсайтовая top-level навигация методом GET: `Strict` не уходит, `Lax` уходит), `CrossSite` (подресурс, фрейм, POST-навигация: уходят только `None` и cookie без атрибута). Булев `get_for_request` остался для старых реализаций.
- **Cookie без атрибута `SameSite`** (и с нераспознанным значением) — `SameSite::Unspecified`, ведёт себя как `None` (решение Р11: Lax-по-умолчанию рвёт возврат с 3-D Secure) и, в отличие от явного `None`, не требует `Secure`. Прежние строки БД с `Lax` остаются `Lax`.
- **Граница сайта по точке:** `evilexample.com` больше не «внутри» `example.com` (раньше `ends_with` без точки) — и в SameSite, и в third-party для EasyList (`$third-party` теперь получает реальный контекст).
- **Ключ раздела jar-а — `None`** (`network::COOKIE_PARTITION`): `top_level_site` клиента служит только SameSite и third-party, а не ключом БД. Иначе cookie, выставленные HTTP-ответом фрейма или `document.cookie`, оказались бы в другом разделе.
- **Известные границы:** вложенный iframe судится по сайту ближайшего родителя, а не верхнего документа; подресурсы самого iframe — по его собственному сайту (в этих случаях ограничение мягче, чем в Chrome, но не жёстче); `NetworkTransport::fetch` помечен как top-level навигация (BUG-292) и поэтому пропускает `Lax` межсайтово.

Тесты: `cookies.rs` (`provider_samesite_filters_by_request_context`, `provider_legacy_cross_site_flag_is_subresource_context`, `parse_samesite_variants`), `network/src/lib.rs` (`cookie_site_context_follows_document_site_and_request_kind`, `host_in_site_respects_label_boundary`, `cookie_site_context_unsafe_navigation_is_cross_site`), `page_source.rs` (`initiator_site_*`).
