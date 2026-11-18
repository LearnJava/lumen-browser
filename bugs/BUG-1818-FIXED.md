# BUG-1818 — 12 файлов `a-tag` в `referrer-policy/4K` — TIMEOUT без единого сабтеста

**Статус:** FIXED 2026-11-18 (P3)
**Тип:** дефект реализованного кода — скриптовая активация `<a target=…>.click()` игнорировала `target` и навигировала текущий документ вместо именованного фрейма.
**Заведён:** 2026-11-15 (P3, срез 1 [BUG-1235](BUG-1235-OPEN.md))
**Область:** js (`web_api_shim_tail_b.js` — ветка A/AREA `_lumen_run_activation_behavior`), shell (`app/about_to_wait.rs` — `scripted_link_navigate`), `NavigateRequest::Link`.

## Симптом

`referrer-policy/4K` (2026-11-15, dev-release, Windows, `run_report.py --all --root referrer-policy/4K --recursive --processes 7`): **все 12** `a-tag.http.html` этого корня — harness TIMEOUT, ни одного `test_status`:

```
4K/gen/top.http-rp/{no-referrer-when-downgrade,strict-origin-when-cross-origin,unsafe-url,
                    origin-when-cross-origin,same-origin,unset}/a-tag.http.html
4K/gen/top.meta/{same-origin,no-referrer-when-downgrade,unsafe-url,unset,
                 origin-when-cross-origin,strict-origin-when-cross-origin}/a-tag.http.html
```

## Причина

`requestViaNavigable` (`common/security-features/resources/common.sub.js`) создаёт `<iframe name=guid>` (без `src`), затем `<a target=guid href=document.py?…>` и зовёт `a.click()`, ожидая `message` от фрейма (`event.source === iframe.contentWindow`).

Скриптовая активация гиперссылки (`HTMLElement.prototype.click()` → `_lumen_perform_click` → `_lumen_run_activation_behavior`, ветка `A`/`AREA` в `web_api_shim_tail_b.js`) читала `el.href` и звала `_lumen_navigate_or_fragment(targetHref, false)` — тот же вход, что `location.href =`, — **не глядя на атрибут `target`**. Поэтому клик навигировал ТОП-уровневый документ на `document.py` вместо именованного `<iframe>`: testharness исчезал вместе со страницей, `message` от фрейма не приходил, `promise_test` висел до harness-TIMEOUT (без единого сабтеста — страница уже другая).

Нативный клик (реальная мышь) тот же `target` обрабатывает правильно (`click.rs::click_activate`: `find_frame_by_name`/`find_tab_by_window_name`/`_blank`), так что это дефект именно скриптового пути активации, а не отсутствующая функциональность.

## Фикс

1. **JS-шим** (`web_api_shim_tail_b.js`, ветка `A`/`AREA`): читает `el.target`; `_self`/`_top`/`_parent`/пустой — прежний путь `_lumen_navigate_or_fragment`. Непустое имя сперва резолвится в дочерний фрейм **текущего документа** через `document.getElementsByName(name)` (HTML LS §7.3.2 «the rules for choosing a navigable», ветка child browsing context) и ставит ему `src = targetHref` синхронно — тот же путь, что `iframe.src =`, поэтому сканер фреймов видит итоговый URL в том же тике (фрейм не успевает загрузиться как `about:blank`). `_blank` и имена без совпадения в текущем документе уходят в shell через `_lumen_navigate_target`.
2. **`NavigateRequest::Link { url, target }`** (js→shell) + native `_lumen_navigate_target` (`v8_runtime/install/storage.rs`): shell резолвит `_blank` (новая вкладка), именованный фрейм, которого шим не видит (вложенный, уже спавненный — `find_frame_by_name`), именованную вкладку (`find_tab_by_window_name`) или новую вкладку.
3. **`scripted_link_navigate`** (`app/about_to_wait.rs`) — тот же порядок ветвления, что нативный клик в `click.rs`. `shortcut: opener/rel/referrerpolicy атрибуты ссылки сюда не доезжают — они нужны только ветке `_blank`, которую referrer-policy a-tag не использует; добавить при первом тесте со scripted `_blank`-кликом.`

## Проверка

- `cargo test -p lumen-js --test all --features v8-backend link_activation` — 12/12 (новые `scripted_anchor_click_with_named_iframe_target_sets_src`, `…_named_target_emits_a_link_request`, `…_blank_target_emits_a_link_request`, `…_self_target_still_pushes`).
- `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` — чисто.
- `cargo clippy -p lumen-shell --all-targets -- -D warnings` — чисто.
- WPT `referrer-policy/4K` не перепрогонялся (12 файлов a-tag ожидаются не-TIMEOUT после фикса — навигация фрейма идёт тем же путём, что `iframe-tag`, который в 4K уже зелёный; полный прогон и регенерация baseline — в [BUG-1235](BUG-1235-OPEN.md)).
