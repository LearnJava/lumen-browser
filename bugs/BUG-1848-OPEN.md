# BUG-1848 — 9 тестов `lumen-js` падают детерминированно (`--features v8-backend`): documentPictureInPicture read-only, showPicker, RTC-шейм

**Статус:** OPEN
**Заведён:** 2026-11-26 (P6, попутно гейту MCP-TOTAL-S1a)
**Область:** js (`crates/js/src/document_pip.rs` + `dom.rs`-установка API; тесты `dom/tests/document_pip.rs`, `v8_events_cache.rs`, `v8_form_constraint_validation.rs`, `v8_idle_message_clipboard.rs`)

## Симптом

На дереве без единой правки `crates/js` (ветка `p6-mcp-total-s1a`, js-код идентичен
`main`) девять тестов падают **и по одному** (`--exact --test-threads=1`), не только
в полном параллельном прогоне:

- `document_pip::tests::*` — все 5; каждый печатает
  `v8: document_pip::install_document_pip_api_v8 failed: JS runtime error: Cannot assign to read only property 'documentPictureInPicture' of object '#<Window>'`
  → инсталляция API не проходит, ассерты «класс/свойство существуют» падают;
- `dom::tests::v8_events_cache::sw_registration_inherits_per_api_members` —
  тот же v8-шум, ассерт `bool_eval` (`v8_events_cache.rs:1052`);
- `dom::tests::v8_idle_message_clipboard::rtcpeerconnection_interface_shape` —
  тот же v8-шум, ассерт `bool_eval` (`v8_idle_message_clipboard.rs:629`);
- `dom::tests::v8_form_constraint_validation::show_picker_throws_for_text_type`
  и `show_picker_fires_click_for_color` — ассерт `bool_eval` без v8-шума
  (`v8_form_constraint_validation.rs:342` и сосед).

Проба:

```
cargo test -p lumen-js --features v8-backend --lib "document_pip::tests::document_pip_picture_in_picture_event_class_exists" -- --exact --test-threads=1
```

Дополнение 2026-11-26 (гейт MCP-TOTAL-S7a, js-код по-прежнему идентичен `main`):
в полном прогоне `scoped-test.sh` те же девять плюс ещё пять из той же семьи
(установка/состав глобала):

- `dom::tests::v8_bug568_document_write::written_external_script_load_and_error_events`
  и `…_blocks_later_written_scripts`;
- `frame_bridge::tests::frame_registration_supersedes_the_initial_frame_runtime`;
- `internal_globals::tests::only_known_internal_names_remain_on_the_global`;
- `shared_worker::tests_v8::v8_shared_worker_top_level_failure_reaches_the_first_client`.

Единичный прогон (`--exact --test-threads=1`) для этих пяти не снят; полный
прогон в тот раз оборвался внутри `lumen-js` (cargo считает краш тест-бинаря
не «фейлом» и останавливает остаток), хвост замыкания прогнан отдельно. Если
корень у пятерки не общий с `document_pip` — развести по багам.

Отдельно от этого: падения `navigator_clipboard_read_asks_and_returns_items` и
`perf9_code_cache_hit_preserves_semantics` в полном прогоне проходят по одному —
это известный [BUG-1677](BUG-1677-OPEN.md), в этот баг не входит.

## Причина

Не локализована, не бисектилась. Подозреваемый — «BUG-753 срез 2: внутренний
контейнер вместо глобала (725 → 1 имя)» (`1e266b376b`), верхний коммит по
`document_pip.rs`: свойство `documentPictureInPicture` на `Window` стало
read-only, инсталляция API падает на присваивании. Падения `show_picker_*` и
`rtcpeerconnection` могут быть того же корня (переопределение глобалов) или
самостоятельными — не разделено.

## Что делать

1. Бисект падения `document_pip` (репро — проба выше) до коммита-виновника.
2. Починить установку `documentPictureInPicture` (определение свойства, не
   присваивание — ср. способ инсталляции остальных API), погнать
   `cargo test -p lumen-js --features v8-backend --lib` целиком.
3. Разделить/починить оставшиеся четыре падения; если корень общий — закрыть
   одним срезом, нет — развести по багам.

## Как проверить

`cargo test -p lumen-js --features v8-backend --lib` — все девять зелёные;
полный `bash scripts/scoped-test.sh` красный вне этого бага только на
BUG-1677 и BUG-1849.
