# BUG-1620 — navigator.gpu в штатной сборке отдаёт фиктивный адаптер

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** js (`crates/js/src/webgpu.rs:825`)

## Симптом и место в коде

Найдено сверх исследования. Настоящий бэкенд WebGPU (нативы _lumen_webgpu_*) включается фичей lumen-js/webgpu (crates/js/Cargo.toml:113-120). Эту фичу не включает ни одна фича lumen-shell (shell/Cargo.toml:33: v8 = lumen-js/v8-backend), что признаёт и комментарий в js/Cargo.toml. Поэтому в штатной сборке navigator.gpu — чистый JS-шим: requestAdapter() резолвит фиктивный GPUAdapter (webgpu.rs:825-827), canvas.getContext('webgpu') отдаёт GPUCanvasContext (web_api_shim_mid.js:9760-9768), а submit/draw/dispatch ничего не делают (шапка webgpu.rs: «Phase 0 … no-ops»). Сайт выбирает ветку WebGPU и рисует пустой холст. По спецификации requestAdapter может вернуть null («адаптера нет»), так поступает и Chrome на неподдерживаемых GPU. В CAPABILITIES.md:179 WebGPU назван среди API, которые «actually do work»; в :169 оговорка «feature webgpu» есть, но о том, что в поставке фича выключена, не сказано. Неверный ответ на запрос возможностей плюс дрейф документации — баг. По домену баги ведёт P3, его нет в списке ролей, поэтому указан P6.

## Где смотреть

- crates/js/Cargo.toml:113-120
- crates/shell/Cargo.toml:33
- crates/js/src/webgpu.rs:20-24
- crates/js/src/webgpu.rs:825-827
- crates/js/src/webgpu.rs:905-911
- crates/js/src/shim/web_api_shim_mid.js:9757-9768
- CAPABILITIES.md:179

## Объём и приёмка

Баг (сверх исследования). lumen-js/webgpu не включена ни одной фичей shell (crates/js/Cargo.toml:113-120, crates/shell/Cargo.toml:33), поэтому в штатной сборке navigator.gpu.requestAdapter() резолвит фиктивный GPUAdapter (crates/js/src/webgpu.rs:825), а draw/submit — no-op: сайт выбирает WebGPU и рисует пустой холст; CAPABILITIES.md:179 пишет, что WebGPU работает. Объём: без нативов _lumen_webgpu_* requestAdapter → null (как Chrome без адаптера), правка CAPABILITIES; реальный бэкенд — отдельное решение (P3-gpusandbox). Приёмка: штатный lumen.exe, await navigator.gpu.requestAdapter() === null. Источник: docs/research/site-compat/report.md §8.1 (класс)

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
