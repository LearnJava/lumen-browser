//! COMPAT-HANG: бесконечный скрипт виден сторожу и останавливается
//! `TerminateExecution`, после чего рантайм исполняет следующие задачи.

use super::*;
use lumen_core::JsRuntime;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !cond() {
        assert!(Instant::now() < deadline, "не дождались: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn infinite_loop_is_terminated_and_runtime_survives() {
    let rt = Arc::new(rt());
    let stall = rt.stall_handle();
    let spinner = {
        let rt = Arc::clone(&rt);
        std::thread::spawn(move || rt.eval("while (true) {}"))
    };
    wait_until("рантайм занят", || stall.is_busy());
    assert!(stall.terminate(), "занятый рантайм должен принять остановку");
    let outcome = spinner.join().expect("поток eval не должен паниковать");
    assert!(outcome.is_err(), "прерванный скрипт не возвращает значения: {outcome:?}");
    wait_until("рантайм свободен", || !stall.is_busy());
    // Рантайм жив: следующая задача исполняется.
    assert_eq!(rt.eval("1 + 2").unwrap(), JsValue::Number(3.0));
}

#[test]
fn terminate_between_jobs_does_not_kill_the_next_one() {
    let rt = rt();
    let stall = rt.stall_handle();
    assert!(!stall.terminate(), "свободный рантайм останавливать нечего");
    assert_eq!(rt.eval("40 + 2").unwrap(), JsValue::Number(42.0));
}
