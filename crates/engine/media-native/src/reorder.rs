//! Метки показа кадров, вышедших из асинхронного декодера (COMPAT-MEDIA-OSDEC-LINUX).
//!
//! Конвейер GStreamer работает в своих потоках и отдаёт кадры в порядке показа,
//! но без наших меток: пакеты уходят в `appsrc` без временных штампов (структуры
//! `GstBuffer` читать не нужно, ABI остаётся минимальным). Кадр не может выйти
//! раньше, чем декодер получил все пакеты, показываемые до него, поэтому метка
//! очередного кадра — наименьшая из ещё не розданных меток отправленных пакетов.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Очередь меток отправленных пакетов.
#[derive(Debug, Default)]
pub(crate) struct PtsQueue {
    pending: BinaryHeap<Reverse<i64>>,
    last: i64,
}

impl PtsQueue {
    /// Пакет с меткой `pts_us` ушёл в декодер.
    pub(crate) fn sent(&mut self, pts_us: i64) {
        self.pending.push(Reverse(pts_us));
    }

    /// Метка вышедшего кадра. Если кадров вышло больше, чем отправлено пакетов
    /// (декодер размножил кадр), берётся последняя выданная метка.
    pub(crate) fn frame(&mut self) -> i64 {
        if let Some(Reverse(pts)) = self.pending.pop() {
            self.last = pts;
        }
        self.last
    }

    /// Сколько пакетов отправлено и ещё не дало кадра.
    pub(crate) fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// Перемотка: забыть всё.
    pub(crate) fn clear(&mut self) {
        self.pending.clear();
        self.last = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_come_out_in_display_order() {
        // Порядок декодирования I P B B → показа: I B B P (метки 0, 3, 1, 2).
        let mut q = PtsQueue::default();
        for pts in [0, 3, 1, 2] {
            q.sent(pts);
        }
        assert_eq!(q.in_flight(), 4);
        assert_eq!((q.frame(), q.frame(), q.frame(), q.frame()), (0, 1, 2, 3));
        assert_eq!(q.in_flight(), 0);
    }

    #[test]
    fn a_frame_cannot_take_a_label_that_was_not_sent_yet() {
        let mut q = PtsQueue::default();
        q.sent(0);
        q.sent(3);
        assert_eq!(q.frame(), 0);
        // Кадр с меткой 1 выйдет после отправки пакета 1, а не раньше.
        q.sent(1);
        assert_eq!(q.frame(), 1);
        assert_eq!(q.frame(), 3);
    }

    #[test]
    fn surplus_frames_repeat_the_last_label_and_clear_resets() {
        let mut q = PtsQueue::default();
        q.sent(40_000);
        assert_eq!(q.frame(), 40_000);
        assert_eq!(q.frame(), 40_000);
        q.sent(80_000);
        q.clear();
        assert_eq!(q.in_flight(), 0);
        assert_eq!(q.frame(), 0);
    }
}
