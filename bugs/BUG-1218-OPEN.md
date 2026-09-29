# BUG-1218 — падение процесса: паника потока движка (BUG-986, `forms.rs:1240`) → `PoisonError` в главном потоке

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/style/matching/forms.rs:1240`), shell (`crates/shell/src/frame_dynamic_load.rs:49`)
**Найден:** 2026-09-29, живой стенд bankruptcy-platform, сборка `main` 22a782d55 (dev-release)

## Симптом

Окно закрывается само посреди серии переходов между страницами. В stderr:

```
thread 'lumen-engine' panicked at crates\engine\layout\src\style\matching\forms.rs:1240:23:
BUG-986: NodeId 336 вне арены документа (len 244) — устаревший/чужой идентификатор, переживший навигацию …
thread 'main' panicked at crates\shell\src\frame_dynamic_load.rs:49:28:
called `Result::unwrap()` on an `Err` value: PoisonError { .. }
[mcp] connection error: EOF
```

Первая паника — устаревший `NodeId` (336 при 244 узлах в арене) пережил навигацию. Вторая — `unwrap()` на отравленном
мьютексе в главном потоке превращает падение одного потока в падение всего браузера.

## Воспроизведение

Недетерминированное: 1 раз за 3 сессии. Стенд, вход `test_au`, затем ~10 переходов дашборд ↔ дела ↔ документооборот
(клик по пунктам сайдбара, клиентская навигация Next.js) с `LUMEN_FRAME_LOG=1 LUMEN_PROFILE_TREE=1`; падение пришло во
время загрузки `/dashboard`, когда шли подгрузки шрифтов (`FontLoaded: «Manrope»/«Onest»/«Inter»`).

## Что делать

1. Найти, откуда в `forms.rs:1240` берётся `NodeId` прошлой навигации (задание на движковом потоке, пережившее смену документа).
2. `frame_dynamic_load.rs:49`: не `unwrap()` на `lock()`; отравленный мьютекс — не повод ронять UI-поток.
3. Возможная связь: «голая» страница без стилей на скриншоте пользователя (сайдбар в serif, синие ссылки) — состояние
   после смерти потока движка. Не доказано.
