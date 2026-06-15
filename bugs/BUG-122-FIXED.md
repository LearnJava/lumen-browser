# BUG-122

**Статус:** FIXED 2026-06-15
**Компонент:** test/paint
**Файл:** `crates/engine/paint/src/compositor.rs:938`

## Описание

flaky: compositor::tests::compositor_thread_wakes_on_commit_faster_than_full_frame (и иногда compositor_thread_flushes_pending_asynchronously) падали под нагрузкой. Корень — тесты гнали wall-clock дедлайн (50/200 мс), фактически проверявший планировщик ОС, а не движок: под параллельными сессиями ОС не успевала разбудить compositor-поток за дедлайн. Fix: idle fallback tick инъецируется через CompositorThread::spawn_with_tick(); тесты ставят его в 1 час, поэтому flush в пределах генерозного 10 с дедлайна доказывает именно notify-wakeup (idle-таймер отключён), а jitter планировщика больше не валит тест. 5× стресс-прогон стабилен
