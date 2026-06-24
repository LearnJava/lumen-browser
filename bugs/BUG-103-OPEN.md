# BUG-103

**Статус:** OPEN (KNOWN_DEBTOR, класс «артефакт тайминга захвата Edge» — как BUG-199/BUG-126)
**Компонент:** js / shell
**Файл:** `crates/js/src/view_transitions.rs`, `crates/shell/src/main.rs` (view_transition cross-fade)

## Описание

View Transitions API L1 **реализован и рендерится** (ревизия F2-4, 2026-06-22):
`document.startViewTransition(callback)` возвращает корректный `ViewTransition`
(updateCallbackDone/ready/finished/skipTransition), шелл снимает `old_dl` на `Begin`
и кросс-фейдит его над новым display-list на `End` (300 ms). Проверено CPU-снимком
(`--screenshot`) и детерминированным `--ipc`-прогоном: страница рисуется правильно.

**Прошлый «99.53%» был артефактом** — пустой gdigrab-захват (белый кадр, регион во весь
экран = сигнатура blank-capture). Реальный детерминированный diff (`run.py --only 61 --ipc`)
= **10.71%**, и тот распадается на:

1. **Тайминг захвата Edge (доминирует, ~8.5% страницы).** По спеку update-callback
   `startViewTransition` асинхронный (выполняется на шаге «update the rendering»). Edge
   headless `--screenshot` снимает кадр ДО срабатывания callback → видна СТАРАЯ DOM
   (card1 «Before» active) + уже выполненный синхронный код (зелёный лог). Lumen рендерит
   **устоявшееся** состояние после callback (card2 «After» active) — спек-корректно и
   полезнее как инструмент рендера. Воспроизвести кадр Edge можно лишь намеренно
   ПРОПУСКАЯ view-transition callback в снимке (тогда ЛЮБАЯ страница с startViewTransition
   рисовала бы устаревшее состояние) — это хуже, чем один debtor. Тот же класс, что
   TEST-71 (BUG-199) и TEST-77 (BUG-126).
2. **Font-parity текста (rule 3).** Заголовок + строка лога рендерятся Inter vs Edge UI-шрифт.

**gdigrab для этой страницы flaky** (то ~11% хороший кадр, то ~100% пустой) — мерить надо
детерминированным `--ipc`. Baseline в `KNOWN_DEBTORS` оставлен 99.53 (gdigrab-blank-safe).

**Опционально (XL, не валидируется TEST-61):** полный L1 — захват NEW-снимка + именованные
группы `view-transition-name` (морф) + проводка псевдо `::view-transition*` в paint с учётом
авторского `animation-duration`. Текущая реализация — упрощённый root cross-fade.
