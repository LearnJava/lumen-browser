# BUG-698 — `EyeDropper.open()` не проверяет transient user activation (спека требует `NotAllowedError`)

**Статус:** FIXED 2026-09-24 (P3)
**Компонент:** js (`crates/js/src/eye_dropper.rs` — тело шима `open()`)
**Найден:** P3, при фиксе [BUG-365](BUG-365-FIXED.md), 2026-08-09

## Симптом

По спеке WICG Eye Dropper API, `open()` без предшествующего пользовательского
жеста (transient activation) обязан немедленно отклоняться `NotAllowedError`.
В Lumen активация не проверяется вовсе — `open()` всегда идёт до конца своей
логики (сейчас — до `#ffffff`-фолбэка, см. BUG-365) независимо от того, был ли
вызов инициирован пользовательским жестом.

Этот дефект был замаскирован фиксируемым в BUG-365 `ReferenceError`м — любой
вызов `open()` падал раньше, чем мог бы дойти до отсутствующей проверки
активации. После фикса BUG-365 маскировка снята.

## Причина

В кодовой базе нет инфраструктуры отслеживания transient user activation ни
для одного Web API — проверено `grep`-ом по `user activation`/`UserActivation`/
`transient activation`/`hasTransientActivation` в `crates/js/src`, ноль
попаданий. Тот же класс пробела уже отдельно заведён на:

- [BUG-390](BUG-390-FIXED.md) — `requestFullscreen()`;
- [BUG-655](BUG-655-FIXED.md) — `requestPointerLock()`;
- [BUG-667](BUG-667-FIXED.md) — `getScreenDetails()` (permission-state вариант того же класса).

## Масштаб

Единственный WPT-тест, который проверял бы это (`eye-dropper-abort-signal.tentative.https.html`),
недоступен исполнителю: он синтезирует клик через `test_driver.Actions()`,
а текущий минимальный executor не реализует эту команду (см. `docs/wpt-status.md`
запись `eyedropper`). Практический масштаб — любой сайт может вызвать
`new EyeDropper().open()` из фонового скрипта без ведома пользователя; сейчас
это просто резолвится безобидным `#ffffff` (нет реального пикера), но после
появления платформенной интеграции это станет реальной проблемой приватности.

## Фикс

К моменту этой сессии общая инфраструктура transient-activation-трекинга
(GAP-USERACT/BUG-751, `install_user_activation` в
`crates/js/src/v8_runtime/install/platform.rs`, `_lumen_mark_user_activation`/
`_lumen_consume_user_activation`/`_lumen_user_activation_is_active`) уже была
построена и используется `window_management.rs`/`local_font_access.rs` —
описанный выше «Возможный фикс» больше не требовался. `EyeDropper.open()`
переведён на тот же гейт: если `navigator.userActivation.isActive === false`,
`open()` бросает `NotAllowedError` до всякой прочей логики; при успешной
проверке зовёт `_lumen_consume_user_activation()`. `navigator`/`activation`
undefined (нет полного шима, например в собственных unit-тестах модуля)
остаётся permissive — тот же паттерн, что у соседних гейтов.

Остальные API того же класса пробела: [BUG-390](BUG-390-FIXED.md) и
[BUG-667](BUG-667-FIXED.md) уже FIXED; [BUG-655](BUG-655-FIXED.md)
(`requestPointerLock()`) всё ещё OPEN — не в скоупе этого бага.
