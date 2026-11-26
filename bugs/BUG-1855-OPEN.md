# BUG-1855 — livelock `v8_perf_typedom_node`: квота узлов не бросает `QuotaExceededError`, тесты наполняют DOM вечно

**Статус:** OPEN
**Заведён:** 2026-11-26 (P6, попутно гейту MCP-TOTAL-S7a)
**Область:** js (`crates/js/src/dom/tests/v8_perf_typedom_node.rs` — тесты `dom_create_element_throws_quota_exceeded_when_full`, `dom_create_text_node_throws_quota_exceeded_when_full`, `dom_create_comment_throws_quota_exceeded_when_full`, `dom_create_element_still_throws_when_limit_is_all_live`; прод-код квоты/trampoline — под бисектом)

## Симптом

`cargo test -p lumen-js --lib -- dom::tests::v8_perf_typedom_node -- --test-threads=1`
висит дольше 4 минут, убит по таймауту — детерминировано, соло. В полном
прогоне (`--test-threads=4`) те же 4 теста стоят «running for over 60 seconds»
и дальше не завершается ни один. В 16-поточном прогоне эффекта хуже: после
`v8_bug1293_motion_overlay::a_keyframes_…` **ни один** тест не завершается
80+ минут, при этом бинарь непрерывно жжёт CPU (~15 CPU-с на 12 wall-с,
29 нитей, 657 МБ RSS) — остальные тесты блокированы на общем ресурсе (очевидно,
лок V8-платформы/рантайма), пока «горящий» тест бесконечно создаёт узлы.

Модуль `v8_bug1293_motion_overlay` (тесты непосредственно перед ним по
алфавиту) соло проходит за 2 с — клин именно в этой четвёрке.

## Причина (предварительная)

Тесты наполняют лимит узлов документа и ждут `QuotaExceededError`, который
больше не приходит — живлока неограниченного `createElement`/`createTextNode`.
Регрессия сегодня: на гейте этого же диффа раньше прогон замыкания доходил до
shell-тестов (ROADMAP MCP-TOTAL-S7a, «scoped-test зелёный»; BUG-1854 найден в
таком прогоне). В окно 13:40–16:54 UTC+3 легли коммиты lumen-js — первичный
подозреваемый `7e06d0dc49` (stack-buffer аргументов нативного trampoline),
альтернативы `1b11a83cf6` (`_css_an_update_all`), `958e7c7078` (execution-timing
083/084). Бисект нужен.

Тот же прогон поймал вторую, возможно однорневую, регрессию — [BUG-1856](BUG-1856-OPEN.md)
(`documentPictureInPicture` внезапно read-only).

## Что делать

1. Бисект по окну 13:40–16:54, починить корень (квота узлов или trampoline).
2. Тесты должны **падать**, а не висеть: счётчик попыток в цикле наполнения с
   паникой по превышению разумного предела — зависший тест блокирует весь
   бинарь через общий лок и маскирует остальные результаты.

## Как проверить

`cargo test -p lumen-js --lib -- dom::tests::v8_perf_typedom_node` — завершается
(хоть FAIL, хоть ok, не hang); полный `cargo test -p lumen-js` доходит до
конца за разумное время.
