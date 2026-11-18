# BUG-1817 — `Referer`/`document.referrer` не обновлялись после `history.replaceState`

**Статус:** FIXED
**Тип:** дефект реализованного кода — `history.replaceState` URL меняет, но источник referrer-а оставался снимком URL на момент загрузки страницы.
**Заведён:** 2026-11-15 (P3, срез 1 [BUG-1235](BUG-1235-OPEN.md))
**Закрыт:** 2026-11-18 (P3)

## Симптом

`location.href` после `history.replaceState(null, null, "/AAAA…")` менялся штатно, а исходящий `Referer` сабресурсов (и, соответственно, `document.referrer` у получателя) оставался от URL загрузки документа.

## Причина

Источник referrer-а — `HttpClient.document_context` (`with_document_context`), в который на момент загрузки страницы клался **снимок** URL документа (`ResourceBase::url()`). Same-document rewrite вёл за собой `self.source` (`PageSource::retarget_url`, BUG-1780), но не ячейку, из которой `HttpClient` строил `Referer`/`Origin`/`Sec-Fetch-*`. Поэтому и JS-`fetch()`/XHR-клиент (построенный один раз в `page_pipeline.rs`), и подресурсы (`<img>`/`<script src>`/`<iframe>`/worker), чьи клиенты строятся из `frame_env.page_base`, отправляли старый URL.

## Фикс

- `ResourceBase` получил поле `live_url: Option<Arc<Mutex<String>>>` — живую ячейку URL документа. `url_base` её заводит, клоны базы разделяют её (`Arc`), поэтому `frame_env.page_base` и построенный из неё JS-клиент видят одну величину.
- `ResourceBase::url()` читает `live_url`, если она есть (иначе — откат на `kind`); добавлен `ResourceBase::retarget_url(&str)`.
- `HttpClient.document_context` теперь хранит `(Arc<Mutex<String>>, ReferrerPolicy)`; новый `with_document_context_cell` берёт ячейку наружу, `document_url()` читает её на каждом запросе. `document_host` (EasyList `$domain=`) остаётся снимком — хост same-origin-константен, менять его `replaceState` не может.
- `resource_base::http_client_for_subresource_with_policy` подключает ячейку через `with_document_context_cell`.
- Дренаж same-document rewrite в `about_to_wait.rs` (pushState / replaceState / перехваченная `navigation.navigate()`) после `self.source.retarget_url` переписывает и `frame_env.page_base.retarget_url`.

## Проверка

- `cargo test -p lumen-network referer` — 7/7 (новый `js_fetch_referer_follows_retargeted_document_url_cell`: два запроса до/после переписывания ячейки, `Referer` = `page.html` → `AAAA`).
- `cargo test -p lumen-shell resource_base` — 34/34 (новые `retarget_url_updates_url_for_clones_sharing_the_cell`, `retarget_url_is_noop_for_file_base`).
- `cargo clippy -p lumen-network -p lumen-shell --all-targets -- -D warnings` — чисто.
- WPT `referrer-policy/4K` не перепрогонялся (172 сабтеста формы из карточки — ожидается PASS после фикса; регенерация baseline — в [BUG-1235](BUG-1235-OPEN.md)).
