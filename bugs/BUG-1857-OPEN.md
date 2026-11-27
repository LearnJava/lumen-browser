# BUG-1857 — `nav_post_body::fetch_page_sends_post_body_and_content_type` изредка падает: ОС выдала мок-серверу bad-port (5061), Fetch §3.9 блокирует навигацию

**Статус:** OPEN
**Заведён:** 2026-11-27 (P6, попутно гейту MCP-TOTAL-S7d)
**Область:** network (тест-инфра `crates/network/tests/cases/nav_post_body.rs:38`; прод-код не трогается)

## Симптом

В полном прогоне `scoped-test.sh` (гейт MCP-TOTAL-S7d, network-код вне диффа) тест
падает: `post navigation: Network("blocked port: 5061")` (`nav_post_body.rs:94`).
Мок-сервер теста биндит сырой `TcpListener::bind("127.0.0.1:0")`, ОС выдала 5061 —
порт из списка «bad ports» Fetch §3.9 (`bad_port.rs:93`, sips), и навигация на
мок-URL честно блокируется клиентом. Тот же класс, что [BUG-911](BUG-911-FIXED.md):
там для юнит-тестов крейта заведён `bind_ephemeral_listener` (`src/lib.rs:62`,
перебирает бинды до не-bad-порта, ~4 % прогонов на Windows), но он `#[cfg(test)]`
внутри крейта и интеграционному тесту недоступен — в `tests/cases/` пять файлов
сидят на сыром бинде: `nav_post_body.rs:38`, `accept_language.rs:41`,
`lumen_resolve.rs:25`, `bug785_extra_ca.rs:109`, `osroots_trust.rs:43`.

Соло-прогон зелёный (ОС выдала обычный порт) — флейк на пересечении эфемерного
диапазона ОС со списком запрещённых портов, не регрессия.

## Что делать

1. Поднять `bind_ephemeral_listener` до интеграционных тестов (test-only хелпер
   в общем модуле `tests/`), заменить пять сырых биндов на него.
2. Полный `scoped-test.sh` не должен краснеть на `lumen-network` (при прочих
   известных кластерах — BUG-1848, BUG-1849, BUG-1854).
