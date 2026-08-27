//! Тела методов winit-цикла (`ApplicationHandler<LoadEvent> for Lumen`).
//!
//! Дорожка SPLIT (`docs/tasks/p1-monolith-split-queue.md`, батч SH-2). Сам
//! trait-impl остаётся в `main.rs`: реализацию трейта нельзя разложить по
//! нескольким блокам, а её тело — 5 138 строк, то есть втрое больше потолка
//! `scripts/check_file_sizes.py`. Поэтому метод трейта становится переходником
//! в одну строку, а его тело переезжает сюда как `pub(crate) fn on_<метод>`
//! в `impl Lumen` — обычный inherent impl, который дробить как раз можно.
//!
//! Перенос механический: глубина вложенности `fn` в `impl Lumen` та же, что и
//! в `impl ApplicationHandler`, поэтому тела перенесены без дедента и ни одна
//! строка внутри строковых литералов не тронута.

pub(crate) mod about_to_wait;
pub(crate) mod resumed;
pub(crate) mod user_event;
