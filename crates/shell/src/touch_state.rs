//! Трекер активных касаний (TOUCH-1-S4): переводит поток `winit::event::Touch`
//! в последовательность pointer-выходов (`Down`/`Move`/`Up`/`Cancel`) и считает
//! геометрию многопальцевого жеста для будущих pan/pinch.
//!
//! Модуль чистый: не знает ни об окне, ни о `Lumen`. Подключение к
//! `WindowEvent::Touch` — TOUCH-1-S5, жесты — S6/S7.
//!
//! Правила Pointer Events L3, которые держит трекер:
//! - `pointerId = 1` занят мышью, касания получают `2, 3, …`; номер не
//!   переиспользуется вообще (счётчик монотонный), поэтому пересечения с ещё
//!   активным касанием быть не может;
//! - первичный указатель — касание, начавшееся при отсутствии других активных.
//!   Когда он поднят, остальные пальцы первичными не становятся; новый
//!   первичный появляется только после того, как поднялись все.

// Подключается TOUCH-1-S5.
#![allow(dead_code)]

use winit::event::TouchPhase;

/// `pointerId` мыши; касания начинают с `FIRST_TOUCH_POINTER_ID`.
pub const MOUSE_POINTER_ID: u32 = 1;
const FIRST_TOUCH_POINTER_ID: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchOutKind {
    Down,
    Move,
    Up,
    Cancel,
}

/// Одно pointer-событие, порождённое входным касанием.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchOut {
    pub kind: TouchOutKind,
    pub pointer_id: u32,
    pub is_primary: bool,
    pub pos: (f32, f32),
}

#[derive(Debug, Clone, Copy)]
struct ActiveTouch {
    /// Идентификатор касания от winit.
    id: u64,
    pointer_id: u32,
    pos: (f32, f32),
}

#[derive(Debug)]
pub struct TouchTracker {
    /// Активные касания в порядке начала.
    active: Vec<ActiveTouch>,
    /// `pointerId` первичного касания; живёт, пока активен хотя бы один палец.
    primary: Option<u32>,
    next_pointer_id: u32,
}

impl Default for TouchTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TouchTracker {
    pub fn new() -> Self {
        Self { active: Vec::new(), primary: None, next_pointer_id: FIRST_TOUCH_POINTER_ID }
    }

    /// Число активных касаний.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Центроид всех активных касаний; `None`, если пальцев меньше двух.
    pub fn centroid(&self) -> Option<(f32, f32)> {
        if self.active.len() < 2 {
            return None;
        }
        let n = self.active.len() as f32;
        let (sx, sy) = self.active.iter().fold((0.0, 0.0), |(x, y), t| (x + t.pos.0, y + t.pos.1));
        Some((sx / n, sy / n))
    }

    /// Расстояние между двумя первыми по порядку начала касаниями; `None`,
    /// если пальцев меньше двух.
    pub fn distance(&self) -> Option<f32> {
        let (a, b) = (self.active.first()?, self.active.get(1)?);
        Some((a.pos.0 - b.pos.0).hypot(a.pos.1 - b.pos.1))
    }

    /// Обработать входное касание. Неизвестный id в `Moved`/`Ended`/`Cancelled`
    /// игнорируется; повторный `Started` с активным id отменяет старое касание
    /// и начинает новое.
    pub fn on_touch(&mut self, id: u64, phase: TouchPhase, pos: (f32, f32)) -> Vec<TouchOut> {
        match phase {
            TouchPhase::Started => {
                let mut out = Vec::with_capacity(2);
                if let Some(cancel) = self.remove(id, TouchOutKind::Cancel, None) {
                    out.push(cancel);
                }
                let pointer_id = self.next_pointer_id;
                self.next_pointer_id = pointer_id.wrapping_add(1).max(FIRST_TOUCH_POINTER_ID);
                if self.active.is_empty() {
                    self.primary = Some(pointer_id);
                }
                self.active.push(ActiveTouch { id, pointer_id, pos });
                out.push(TouchOut {
                    kind: TouchOutKind::Down,
                    pointer_id,
                    is_primary: self.primary == Some(pointer_id),
                    pos,
                });
                out
            }
            TouchPhase::Moved => {
                let Some(t) = self.active.iter_mut().find(|t| t.id == id) else {
                    return Vec::new();
                };
                t.pos = pos;
                vec![TouchOut {
                    kind: TouchOutKind::Move,
                    pointer_id: t.pointer_id,
                    is_primary: self.primary == Some(t.pointer_id),
                    pos,
                }]
            }
            TouchPhase::Ended => self.remove(id, TouchOutKind::Up, Some(pos)).into_iter().collect(),
            TouchPhase::Cancelled => {
                self.remove(id, TouchOutKind::Cancel, Some(pos)).into_iter().collect()
            }
        }
    }

    /// Снять касание и вернуть завершающий выход `kind`. Позиция выхода —
    /// `pos` либо последняя известная. Когда пальцев не осталось, первичный
    /// указатель сбрасывается.
    fn remove(&mut self, id: u64, kind: TouchOutKind, pos: Option<(f32, f32)>) -> Option<TouchOut> {
        let idx = self.active.iter().position(|t| t.id == id)?;
        let t = self.active.remove(idx);
        let out = TouchOut {
            kind,
            pointer_id: t.pointer_id,
            is_primary: self.primary == Some(t.pointer_id),
            pos: pos.unwrap_or(t.pos),
        };
        if self.active.is_empty() {
            self.primary = None;
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TouchOutKind::*;
    use TouchPhase::*;

    fn kinds(out: &[TouchOut]) -> Vec<TouchOutKind> {
        out.iter().map(|o| o.kind).collect()
    }

    #[test]
    fn single_tap_is_down_up_with_primary_pointer_2() {
        let mut t = TouchTracker::new();
        let down = t.on_touch(7, Started, (10.0, 20.0));
        assert_eq!(
            down,
            [TouchOut { kind: Down, pointer_id: 2, is_primary: true, pos: (10.0, 20.0) }]
        );
        let mv = t.on_touch(7, Moved, (12.0, 22.0));
        assert_eq!(
            mv,
            [TouchOut { kind: Move, pointer_id: 2, is_primary: true, pos: (12.0, 22.0) }]
        );
        let up = t.on_touch(7, Ended, (12.0, 22.0));
        assert_eq!(
            up,
            [TouchOut { kind: Up, pointer_id: 2, is_primary: true, pos: (12.0, 22.0) }]
        );
        assert_eq!(t.active_count(), 0);
    }

    #[test]
    fn two_overlapping_fingers_only_first_is_primary() {
        let mut t = TouchTracker::new();
        let a = t.on_touch(1, Started, (0.0, 0.0))[0];
        let b = t.on_touch(2, Started, (100.0, 0.0))[0];
        assert_eq!((a.pointer_id, a.is_primary), (2, true));
        assert_eq!((b.pointer_id, b.is_primary), (3, false));
        // Поднимаем первичный: второй палец первичным не становится.
        let up_a = t.on_touch(1, Ended, (0.0, 0.0))[0];
        assert!(up_a.is_primary);
        let mv_b = t.on_touch(2, Moved, (101.0, 0.0))[0];
        assert!(!mv_b.is_primary);
        let up_b = t.on_touch(2, Ended, (101.0, 0.0))[0];
        assert_eq!((up_b.pointer_id, up_b.is_primary), (3, false));
        // Все подняты — следующее касание снова первичное.
        let c = t.on_touch(9, Started, (5.0, 5.0))[0];
        assert!(c.is_primary);
    }

    #[test]
    fn cancelled_emits_cancel_and_frees_the_slot() {
        let mut t = TouchTracker::new();
        t.on_touch(1, Started, (1.0, 1.0));
        let out = t.on_touch(1, Cancelled, (1.0, 1.0));
        assert_eq!(kinds(&out), [Cancel]);
        assert_eq!(out[0].pointer_id, 2);
        assert!(out[0].is_primary);
        assert_eq!(t.active_count(), 0);
        // После Cancel тот же id — уже неизвестный.
        assert!(t.on_touch(1, Moved, (2.0, 2.0)).is_empty());
    }

    #[test]
    fn unknown_id_is_ignored_for_every_non_start_phase() {
        let mut t = TouchTracker::new();
        assert!(t.on_touch(42, Moved, (1.0, 1.0)).is_empty());
        assert!(t.on_touch(42, Ended, (1.0, 1.0)).is_empty());
        assert!(t.on_touch(42, Cancelled, (1.0, 1.0)).is_empty());
        assert_eq!(t.active_count(), 0);
        // Счётчик pointerId не сдвинулся.
        assert_eq!(t.on_touch(1, Started, (0.0, 0.0))[0].pointer_id, 2);
    }

    #[test]
    fn repeated_started_cancels_old_and_downs_new() {
        let mut t = TouchTracker::new();
        t.on_touch(1, Started, (1.0, 1.0));
        let out = t.on_touch(1, Started, (5.0, 6.0));
        assert_eq!(kinds(&out), [Cancel, Down]);
        assert_eq!(out[0].pointer_id, 2);
        assert_eq!(out[0].pos, (1.0, 1.0));
        assert_eq!(out[1].pointer_id, 3);
        assert_eq!(out[1].pos, (5.0, 6.0));
        // Единственный палец остался единственным, значит новый — первичный.
        assert!(out[1].is_primary);
        assert_eq!(t.active_count(), 1);
    }

    #[test]
    fn repeated_started_of_secondary_keeps_primary() {
        let mut t = TouchTracker::new();
        t.on_touch(1, Started, (0.0, 0.0));
        t.on_touch(2, Started, (10.0, 0.0));
        let out = t.on_touch(2, Started, (20.0, 0.0));
        assert_eq!(kinds(&out), [Cancel, Down]);
        assert!(!out[0].is_primary);
        assert!(!out[1].is_primary);
        assert!(t.on_touch(1, Moved, (1.0, 0.0))[0].is_primary);
    }

    #[test]
    fn centroid_and_distance_need_two_fingers() {
        let mut t = TouchTracker::new();
        assert_eq!((t.centroid(), t.distance()), (None, None));
        t.on_touch(1, Started, (0.0, 0.0));
        assert_eq!((t.centroid(), t.distance()), (None, None));
        t.on_touch(2, Started, (30.0, 40.0));
        assert_eq!(t.centroid(), Some((15.0, 20.0)));
        assert_eq!(t.distance(), Some(50.0));
        t.on_touch(2, Moved, (60.0, 80.0));
        assert_eq!(t.centroid(), Some((30.0, 40.0)));
        assert_eq!(t.distance(), Some(100.0));
        t.on_touch(1, Ended, (0.0, 0.0));
        assert_eq!((t.centroid(), t.distance()), (None, None));
    }

    #[test]
    fn third_finger_joins_centroid_but_distance_uses_first_two() {
        let mut t = TouchTracker::new();
        t.on_touch(1, Started, (0.0, 0.0));
        t.on_touch(2, Started, (30.0, 0.0));
        t.on_touch(3, Started, (0.0, 30.0));
        assert_eq!(t.centroid(), Some((10.0, 10.0)));
        assert_eq!(t.distance(), Some(30.0));
        // Первый поднят: «двумя первыми» становятся бывшие второй и третий.
        t.on_touch(1, Ended, (0.0, 0.0));
        assert_eq!(t.distance(), Some(30.0f32.hypot(30.0)));
    }

    #[test]
    fn pointer_ids_are_never_reused_while_active() {
        let mut t = TouchTracker::new();
        let mut seen = Vec::new();
        for id in 0..5u64 {
            seen.push(t.on_touch(id, Started, (0.0, 0.0))[0].pointer_id);
        }
        assert_eq!(seen, [2, 3, 4, 5, 6]);
        // Освободили середину — новый палец получает свежий номер.
        t.on_touch(2, Ended, (0.0, 0.0));
        assert_eq!(t.on_touch(10, Started, (0.0, 0.0))[0].pointer_id, 7);
    }

    #[test]
    fn pointer_id_counter_never_returns_to_mouse_id() {
        let mut t = TouchTracker::new();
        t.next_pointer_id = u32::MAX;
        assert_eq!(t.on_touch(1, Started, (0.0, 0.0))[0].pointer_id, u32::MAX);
        assert_eq!(t.on_touch(2, Started, (0.0, 0.0))[0].pointer_id, FIRST_TOUCH_POINTER_ID);
    }
}
