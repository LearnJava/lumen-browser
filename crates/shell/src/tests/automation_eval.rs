//! BUG-1145: `AutomationCommand::Eval` различает «JS-контекста нет» и «движковый
//! поток не успел за срок». Раньше оба случая схлопывались в одно
//! «JS context not available», и таймаут занятого потока на тяжёлых страницах
//! выглядел как пропавший контекст.
//!
//! Под тестом [`PendingEval::poll`] — всё решение, которое принимает
//! `about_to_wait`; сам движковый поток здесь заменён каналом, в который тест
//! кладёт итог (или не кладёт — «поток занят»).

use super::*;
use crate::app::about_to_wait::EvalOutcome;
use crate::engine_thread::EngineWork;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn pending(timeout: Duration) -> (PendingEval, mpsc::SyncSender<EvalOutcome>) {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let (reply_tx, _reply_rx) = mpsc::channel();
    let pending = PendingEval { result_rx, started: Instant::now(), timeout, reply_tx };
    (pending, result_tx)
}

fn error_text(reply: Option<AutomationReply>) -> String {
    match reply {
        Some(AutomationReply::Error(msg)) => msg,
        other => panic!("ожидался AutomationReply::Error, получено {other:?}"),
    }
}

#[test]
fn eval_result_is_delivered() {
    let (p, tx) = pending(Duration::from_secs(5));
    tx.send(Some(Ok("2".to_owned()))).unwrap();
    assert!(matches!(p.poll(Instant::now(), false, || None), Some(AutomationReply::Eval(json)) if json == "2"));
}

#[test]
fn missing_context_still_says_so() {
    let (p, tx) = pending(Duration::from_secs(5));
    tx.send(None).unwrap();
    assert_eq!(error_text(p.poll(Instant::now(), false, || None)), "JS context not available");
}

#[test]
fn missing_context_during_navigation_says_loading() {
    let (p, tx) = pending(Duration::from_secs(5));
    tx.send(None).unwrap();
    assert_eq!(
        error_text(p.poll(Instant::now(), true, || None)),
        "JS context not available: page is still loading"
    );
}

#[test]
fn busy_thread_waits_until_deadline() {
    let (p, _tx) = pending(Duration::from_secs(5));
    assert!(p.poll(p.started, false, || None).is_none(), "до срока eval ждёт, а не отказывает");
}

#[test]
fn busy_thread_past_deadline_names_timeout_and_work() {
    let site = std::panic::Location::caller();
    let (p, _tx) = pending(Duration::from_millis(1500));
    let msg = error_text(p.poll(p.deadline(), false, || Some((EngineWork::Task(site), Duration::from_secs(12)))));
    assert!(msg.starts_with("engine thread busy: eval not run within 1.5 s"), "{msg}");
    assert!(msg.contains("running task from"), "причина занятости в ответе: {msg}");
    assert!(msg.contains("for 12.0 s"), "{msg}");
    assert!(!msg.contains("JS context not available"), "{msg}");
}

#[test]
fn stopped_engine_thread_is_reported() {
    let (p, tx) = pending(Duration::from_secs(5));
    drop(tx);
    assert!(error_text(p.poll(Instant::now(), false, || None)).contains("engine thread stopped"));
}

#[test]
fn timeout_defaults_and_is_capped() {
    assert_eq!(PendingEval::timeout_from_ms(None), crate::engine_thread::QUERY_TIMEOUT);
    assert_eq!(PendingEval::timeout_from_ms(Some(30_000)), Duration::from_secs(30));
    assert_eq!(PendingEval::timeout_from_ms(Some(u64::MAX)), Duration::from_secs(600));
}
