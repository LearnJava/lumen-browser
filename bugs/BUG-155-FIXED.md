# BUG-155

**Статус:** FIXED 2026-06-15
**Компонент:** js
**Файл:** `crates/js/src/dom.rs (perf_observer_lcp_entry)`

## Описание

`perf_observer_lcp_entry` тест падает с "index out of bounds: len is 9 but index is 42": NodeId-индекс 42 выходит за пределы тестового Document (9 нод в make_doc()). Тест создаёт LCP-entry с произвольным element_nid=42, который не существует в тест-документе. Фикс: использовать реальный NodeId из make_doc() или убрать обращение к DOM в тесте
