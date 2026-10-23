# BUG-1697 — IPC-снимок (`Screenshot`) не видит правок DOM, сделанных таймерами: TEST-61 (view-transitions) FAIL 10.81 %

**Статус:** OPEN
**Заведён:** 2026-10-23 (P6, DEVX-22)
**Область:** shell (`render_current_page_to_png`, IPC/automation-режим), js (таймеры, `startViewTransition`)

## Симптом

Полный `python graphic_tests/run.py --continue-on-fail` (профиль `debug`, 2026-10-23):

- **TEST-61** (`view-transitions`) — DEBTOR 3.26 % → **FAIL 10.81 %** против прогона `5b8345773`.
- Бинарь от 2026-10-20 даёт 3.26 %, любой от 2026-10-22 и позже — 10.81 %. Граница — `d30f7bec7` (VT-API, 2026-10-21): колбэк `document.startViewTransition(cb)` теперь исполняется через `setTimeout(0)`.
- IPC-снимок (`render_current_page_to_png` берёт `self.display_list`) этот колбэк не видит: страница остаётся в состоянии «до» перехода.
- Не специфично для VT: страница с обычным `setTimeout(800, () => { изменить DOM })` в IPC-режиме не показывает изменения в снимке даже через 2.5 с (проба — страница с логом таймера). Гипотеза: перерасклад и пересборка display list идут только в `redraw_requested`, а окно в IPC-режиме скрыто, поэтому после таймерной правки DOM `display_list` не обновляется.

## Проба

```
python graphic_tests/run.py --only 61        # профиль debug или dev-release
```

и минимальная страница: `setTimeout(() => document.body.style.background = 'red', 800)`, снимок через IPC `Screenshot` через 2.5 с — фон белый.

## Что делать

Перед `Screenshot` в IPC-режиме тикнуть таймеры и дренировать VT-события, затем при изменённом DOM сделать relayout и пересобрать display list (то же, что делает `redraw_requested`). Только после этого решать про baseline TEST-61 в `KNOWN_DEBTORS_IPC` (`graphic_tests/run.py`): вернуть к ~3.26 % или подтвердить новую цифру.

## Как проверить

`python graphic_tests/run.py --only 61` → не FAIL; проба с `setTimeout(800)` показывает красный фон.
