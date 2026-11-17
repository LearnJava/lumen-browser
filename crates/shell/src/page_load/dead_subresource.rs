//! LOAD-7: мёртвый подресурс не держит `load` окна.
//!
//! Запрос картинки/фона/`<object>`/`<iframe>`, не получивший ответа, висел до
//! `FETCH_READ_TIMEOUT` (60 с) — и всё это время `Lumen::pending_window_load`
//! ждал его, уводя `ready_s` на десятки секунд вперёд `final_applied_s`
//! (база LOAD-0). Дедлайн считается от старта сетевого запроса (отметка в
//! [`crate::dynamic_image_hook::ImageRequestLedger::started`]) или постановки
//! фрейма в [`crate::Lumen::pending_new_frames`]. По истечении запрос считается
//! проваленным: `<img>` получает `error`, `load` окна уходит. Сам сетевой
//! запрос не отменяется — пришедшие позже байты по-прежнему рисуются, а у
//! фрейма остаётся бронь (повторной загрузки того же хоста не будет).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::Lumen;

/// Порядок `STREAM_CSS_DEADLINE` (LOAD-2): десяток секунд. База LOAD-0 видела
/// сетевые таймауты подзапросов кратными 10 с, а `FETCH_READ_TIMEOUT` в 60 с
/// держит `load` недопустимо долго.
pub(crate) const DEAD_SUBRESOURCE_DEADLINE: Duration = Duration::from_secs(10);

/// `LUMEN_NO_LOAD_DEADLINE=1` — поведение до LOAD-7 (живой A/B-рычаг).
pub(crate) fn load_deadline_disabled() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("LUMEN_NO_LOAD_DEADLINE").is_some())
}

/// LOAD-7: пережил ли фрейм дедлайн своей загрузки.
pub(crate) fn frame_deadline_passed(started: Instant, now: Instant) -> bool {
    now >= started + DEAD_SUBRESOURCE_DEADLINE
}

/// LOAD-7: URL, пережившие дедлайн и ещё не осевшие (ни размер, ни ошибка), и
/// ближайший живой дедлайн — его `about_to_wait` кладёт в `WaitUntil`.
pub(crate) fn expired_images(
    started: &HashMap<String, Instant>,
    sizes: &HashMap<String, (u32, u32)>,
    errors: &HashSet<String>,
    now: Instant,
) -> (Vec<String>, Option<Instant>) {
    let mut expired = Vec::new();
    let mut next: Option<Instant> = None;
    for (url, started) in started {
        if sizes.contains_key(url) || errors.contains(url) {
            continue;
        }
        let at = *started + DEAD_SUBRESOURCE_DEADLINE;
        if now >= at {
            expired.push(url.clone());
        } else {
            next = Some(next.map_or(at, |t: Instant| t.min(at)));
        }
    }
    (expired, next)
}

impl Lumen {
    /// LOAD-7: снять с ожидания `load` картинки, пережившие дедлайн; возвращает
    /// момент следующего дедлайна. Сетевой запрос продолжается — просроченный URL
    /// попадает в `stream_image_errors`, и `apply_stream_intrinsic_sizes` дошлёт
    /// ждущим его `<img>` `error`.
    pub(crate) fn expire_dead_images(&mut self) -> Option<Instant> {
        if load_deadline_disabled() {
            return None;
        }
        let (expired, next) = {
            let ledger = self.stream_images_requested.lock().unwrap_or_else(|e| e.into_inner());
            expired_images(&ledger.started, &self.stream_image_sizes, &self.stream_image_errors, Instant::now())
        };
        if !expired.is_empty() {
            self.stream_image_errors.extend(expired);
            self.stream_image_sizes_dirty = true;
        }
        next
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page_load::{unsettled_images, window_load_due};

    #[test]
    fn deadline_expires_only_unsettled_and_names_next() {
        let t0 = Instant::now();
        let started: HashMap<String, Instant> = [
            ("dead".to_string(), t0),
            ("slow".to_string(), t0 + Duration::from_secs(5)),
            ("done".to_string(), t0),
        ]
        .into();
        let sizes: HashMap<String, (u32, u32)> = [("done".to_string(), (1, 1))].into();
        let (expired, next) = expired_images(&started, &sizes, &HashSet::new(), t0 + DEAD_SUBRESOURCE_DEADLINE);
        assert_eq!(expired, vec!["dead".to_string()]);
        assert_eq!(next, Some(t0 + Duration::from_secs(5) + DEAD_SUBRESOURCE_DEADLINE));
    }

    /// Ворота LOAD-7: висящая картинка держит `load` только до дедлайна.
    #[test]
    fn hanging_image_does_not_hold_window_load_past_deadline() {
        let t0 = Instant::now();
        let requested: HashSet<String> = ["dead".to_string()].into();
        let started: HashMap<String, Instant> = [("dead".to_string(), t0)].into();
        let (sizes, mut errors) = (HashMap::new(), HashSet::new());
        assert!(
            !window_load_due(true, false, unsettled_images(&requested, &sizes, &errors), false, false, false),
            "до дедлайна load ждёт картинку"
        );
        let (expired, _) = expired_images(&started, &sizes, &errors, t0 + DEAD_SUBRESOURCE_DEADLINE);
        assert_eq!(expired, vec!["dead".to_string()]);
        errors.extend(expired);
        assert_eq!(unsettled_images(&requested, &sizes, &errors), 0, "дедлайн освободил ledger");
        assert!(
            window_load_due(true, false, 0, false, false, false),
            "после дедлайна load не ждёт"
        );
    }

    #[test]
    fn frame_past_deadline_is_not_busy() {
        let t0 = Instant::now();
        assert!(!frame_deadline_passed(t0, t0 + DEAD_SUBRESOURCE_DEADLINE - Duration::from_millis(1)));
        assert!(frame_deadline_passed(t0, t0 + DEAD_SUBRESOURCE_DEADLINE));
    }
}
