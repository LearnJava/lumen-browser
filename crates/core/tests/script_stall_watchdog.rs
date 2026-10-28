//! COMPAT-HANG: сторож будит оболочку, когда задача идёт дольше порога, и будит
//! ещё раз, когда она кончилась. Отдельный бинарь: порог читается из окружения
//! один раз на процесс, поэтому его нельзя менять рядом с юнит-тестами модуля.

use lumen_core::script_stall::{StallHandle, set_wake, unclaimed_stalled};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[test]
fn watchdog_wakes_the_shell_once_per_stall_and_on_finish() {
    // SAFETY: до первого чтения порога в этом процессе, других потоков, трогающих окружение, нет.
    unsafe { std::env::set_var("LUMEN_SCRIPT_STALL_MS", "300") };
    let wakes = Arc::new(AtomicUsize::new(0));
    let w = Arc::clone(&wakes);
    set_wake(move || {
        w.fetch_add(1, Ordering::SeqCst);
    });

    let h = StallHandle::new();
    h.job_started();
    let deadline = Instant::now() + Duration::from_secs(5);
    while wakes.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "сторож не разбудил оболочку");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(h.stalled_ms().is_some_and(|ms| ms >= 300));
    assert!(unclaimed_stalled().is_some_and(|u| u.same_runtime(&h)));
    // Повторно о той же задаче не будим, пока пользователь не нажал «Подождать».
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(wakes.load(Ordering::SeqCst), 1);

    h.job_finished();
    assert_eq!(wakes.load(Ordering::SeqCst), 2, "конец отчётной задачи будит оболочку: плашка уходит");
    assert!(h.stalled_ms().is_none());
}
