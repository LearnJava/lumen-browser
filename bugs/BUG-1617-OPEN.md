# BUG-1617 — getTimezoneOffset=0 противоречит getHours и Intl

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** js (`crates/js/src/navigator_bindings.rs:372`, `temporal_api.rs:585`)

## Симптом и место в коде

Шим навигатора ставится на каждую страницу (v8_runtime.rs:1113) и заменяет Date.prototype.getTimezoneOffset JS-функцией, которая возвращает константу timezone_offset (navigator_bindings.rs:367-373). По умолчанию это 0 (shell/src/config.rs:237, navigator_bindings.rs:66), у Tor тоже 0 (config.rs:280). Подменён только этот метод: нативные локальные getHours/getDate/toString V8 и нативный Intl.DateTimeFormat().resolvedOptions().timeZone по-прежнему берут пояс хоста. Комментарий теста v8_runtime/tests/mod.rs:1353-1356 это подтверждает: без BiDi-override там пояс хоста. В итоге на МСК-машине getTimezoneOffset() = 0, а getHours() показывает местное время, и toString содержит GMT+0300. Сайты, которые считают местное время через offset или отправляют tz=0 на сервер, сдвигают время на 3 ч. Антифрод и детекторы подмены видят противоречие. В воркерах подмены нет: grep getTimezoneOffset находит только navigator_bindings.rs, так что страница и воркер расходятся. Temporal.Now использует getTimezoneOffset (temporal_api.rs:585) и тоже сдвинут. Подмена — обычная JS-функция, её видно через toString. ADR-007:81 разрешает UTC только для Strict, а код включает его в профиле по умолчанию (Chrome). Ограничение: BiDi setTimezoneOverride оборачивает только Intl.DateTimeFormat (v8_runtime/overrides.rs:108-131), локальные методы Date не трогает.

## Где смотреть

- crates/js/src/navigator_bindings.rs:367-373
- crates/js/src/navigator_bindings.rs:66
- crates/shell/src/config.rs:237
- crates/shell/src/config.rs:280
- crates/js/src/v8_runtime.rs:1113
- crates/js/src/v8_runtime/tests/mod.rs:1353-1356
- crates/js/src/temporal_api.rs:585
- crates/js/src/v8_runtime/overrides.rs:108-131
- docs/decisions/ADR-007-anti-detection-stack.md:81

## Объём и приёмка

getTimezoneOffset подменён константой 0 на каждой странице обычного профиля (crates/js/src/navigator_bindings.rs:372, умолчание shell/src/config.rs:237), а getHours/toString и нативный Intl берут пояс хоста (v8_runtime/tests/mod.rs:1353): на МСК-машине getHours локальный, offset 0, сайты сдвигают время на 3 ч; в воркерах подмены нет; Temporal.Now сдвинут (temporal_api.rs:585). ADR-007:81 разрешает UTC только для Strict. Фикс: без подмены в обычном профиле, UTC — только Анонимный/Tor и согласованно во всех API. Приёмка: на МСК-машине getTimezoneOffset() === -180 как в Edge, страница и воркер согласны. Источник: docs/research/site-compat/report.md §8.1 COMPAT-ID-05.

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
