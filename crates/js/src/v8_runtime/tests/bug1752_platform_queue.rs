//! BUG-1752: очередь задач платформы V8 разбирает сам JS-поток.
//!
//! Регрессия ровно на то, чего не хватало: задача платформы появляется, когда
//! поток стоит в ожидании команды от шелла (истёк таймаут `Atomics.waitAsync`,
//! завершилась фоновая компиляция wasm, готов колбэк `FinalizationRegistry`).
//! Тест поэтому спит **не отправляя ни одной команды** и только потом читает
//! результат: раньше само чтение будило поток и маскировало дефект.

use super::*;

/// `Atomics.waitAsync(cell, 0, 0, 20)` паркует ожидающего; его таймаут V8
/// разрешает задачей foreground-очереди платформы, а не микрозадачей.
///
/// До BUG-1752 промис не разрешался, пока шелл не пришлёт хоть одну команду
/// (или не случится GC-проход) — здесь команды нет двести миллисекунд.
#[test]
fn wait_async_timeout_settles_while_the_thread_waits_for_a_command() {
    let rt = rt();
    rt.eval(
        "var _wa_state = 'pending';\
         var _wa_cell = new Int32Array(new SharedArrayBuffer(8));\
         Atomics.waitAsync(_wa_cell, 0, 0, 20).value.then(function (v) { _wa_state = v; });\
         undefined;",
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        rt.eval("_wa_state").unwrap(),
        JsValue::String("timed-out".into()),
        "таймаут Atomics.waitAsync должен разрешиться без V8Command::Run"
    );
}

/// Тот же механизм для уже готовой задачи: `Atomics.notify` из самой страницы
/// будит ожидающего и ставит задачу платформы, которую поток обязан разобрать
/// сам — `.then` должен выполниться в микрозадачном чекпойнте после неё.
#[test]
fn wait_async_notify_settles_while_the_thread_waits_for_a_command() {
    let rt = rt();
    rt.eval(
        "var _wn_state = 'pending';\
         var _wn_cell = new Int32Array(new SharedArrayBuffer(8));\
         var _wn_wait = Atomics.waitAsync(_wn_cell, 0, 0);\
         _wn_wait.value.then(function (v) { _wn_state = v; });\
         Atomics.notify(_wn_cell, 0);\
         undefined;",
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        rt.eval("_wn_state").unwrap(),
        JsValue::String("ok".into()),
        "уведомлённый Atomics.waitAsync должен разрешиться без V8Command::Run"
    );
}
