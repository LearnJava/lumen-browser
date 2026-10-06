//! Журнал present/колеса для замера плавности прокрутки (THREAD-5).
//!
//! Включается `LUMEN_PRESENT_LOG=<файл>`: каждая строка — событие с меткой
//! Unix-времени в микросекундах (та же шкала, что `time.time()` в Python и
//! `performance.timeOrigin` в Chromium, поэтому логи двух браузеров сводятся
//! без поправки на часы).
//!
//! ```text
//! P <unix_us> <commit_id> <frame|tick>   — кадр показан (после backend.render)
//! W <unix_us> <dy>                       — событие колеса получено окном
//! ```
//!
//! `P` ставится на рендер-потоке сразу после возврата `render()`, то есть после
//! `swap_buffers`, блокирующегося на vsync. Без переменной окружения — один
//! проверенный `OnceLock`, на горячем пути нет ни аллокаций, ни ввода-вывода.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static SINK: OnceLock<Option<Mutex<BufWriter<File>>>> = OnceLock::new();

fn sink() -> Option<&'static Mutex<BufWriter<File>>> {
    SINK.get_or_init(|| {
        let path = std::env::var("LUMEN_PRESENT_LOG").ok().filter(|p| !p.is_empty())?;
        File::create(path).ok().map(|f| Mutex::new(BufWriter::new(f)))
    })
    .as_ref()
}

fn unix_us() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_micros()
}

fn write_line(args: std::fmt::Arguments<'_>) {
    if let Some(m) = sink()
        && let Ok(mut w) = m.lock()
    {
        // Построчный flush: лог читают, пока окно открыто, а сброс при
        // убийстве процесса потерял бы хвост серии.
        let _ = w.write_fmt(args);
        let _ = w.flush();
    }
}

/// Кадр показан. `self_tick` — кадр инерции, нарисованный рендер-потоком без UI.
pub fn present(commit_id: u64, self_tick: bool) {
    if sink().is_some() {
        let kind = if self_tick { "tick" } else { "frame" };
        write_line(format_args!("P {} {commit_id} {kind}\n", unix_us()));
    }
}

/// Событие колеса дошло до обработчика окна (`dy` — как отдал winit).
pub fn wheel(dy: f32) {
    if sink().is_some() {
        write_line(format_args!("W {} {dy}\n", unix_us()));
    }
}
