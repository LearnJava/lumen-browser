//! Нативный стриминг WebAssembly (срез WASM-V8-NATIVE-S2).
//!
//! Без embedder-колбэка V8 не заводит `WebAssembly.compileStreaming`/
//! `instantiateStreaming`: глобал у него родной, а методов нет. Изолят-колбэк
//! (`Isolate::set_wasm_streaming_callback` — ставится в [`super::thread`] до
//! создания контекста, иначе V8 не включает стриминговые методы) получает
//! `source` и `WasmStreaming<false>`, кладёт стрим в thread-local реестр под
//! новым id и передаёт ход JS-помощнику `__lumen_wasm_stream_start(source, id)`.
//!
//! Помощник — единственное место, знающее про `Response`: он проверяет тип
//! ответа (`opaque`/`opaqueredirect` → `TypeError`), MIME (`application/wasm`
//! ровно, побайтово без учёта регистра ASCII; заголовок с параметрами —
//! `TypeError`), статус (`resp.ok`) и неиспользованное тело — шаги «read the
//! source» спеки WebAssembly JS API; затем вычитывает тело одним
//! `arrayBuffer()` (чанковое чтение не делаем) и возвращает байты в
//! `__lumen_wasm_stream_finish(id, buffer, url)`. Любая ошибка, включая
//! отклонение исходного промиса, уходит в `__lumen_wasm_stream_abort(id, err)`,
//! и натив отклоняет обещание стриминга тем же значением — поэтому `rejected-arg`
//! и `abort` WPT-корпуса видят ровно исходную причину. Проверки выполняются в
//! микрозадаче — уже после возврата `compileStreaming()`, что и требует
//! `body.any.js` («bodyUsed до проверки»).
//!
//! fetch/Response-шимы не трогаются; ESM-интеграция wasm — вне среза (W5).
//! Все три имени — на глобале окна и каждого воркера: нативы регистрируются в
//! `install_wasm_streaming_bindings_v8`, которую зовут и `install_dom`
//! (v8_runtime.rs), и общая установка воркеров (worker.rs).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

// Живые стриминговые компиляции этого изолята, по id, выданному колбэком.
// Изолят целиком живёт на одном потоке (thread.rs), thread-local достаточен.
thread_local! {
    static STREAMS: RefCell<HashMap<u32, v8::WasmStreaming<false>>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

/// JS-помощник стриминга: `__lumen_wasm_stream_start(source, id)`.
///
/// MIME сравнивается вручную по ASCII-регистру: `application/wasm;` и
/// `application/wasm;charset=UTF-8` обязаны отвергаться (WPT `contenttype`),
/// а `APPLICATION/WASM` — приниматься; общий `String::toLowerCase` здесь
/// шире нужного (Unicode-складывание), поэтому побайтовый цикл.
const WASM_STREAMING_HELPER: &str = r#"
(function () {
  'use strict';
  var MIME = 'application/wasm';
  function mime_is_wasm(value) {
    if (typeof value !== 'string' || value.length !== MIME.length) return false;
    for (var i = 0; i < MIME.length; i++) {
      var c = value.charCodeAt(i);
      if (c >= 65 && c <= 90) c += 32; // ASCII-only lowercase
      if (c !== MIME.charCodeAt(i)) return false;
    }
    return true;
  }
  globalThis.__lumen_wasm_stream_start = function (source, id) {
    Promise.resolve(source).then(function (resp) {
      if (!(resp instanceof Response)) {
        throw new TypeError('WebAssembly streaming: the argument must resolve to a Response');
      }
      var type = resp.type;
      if (type === 'opaque' || type === 'opaqueredirect') {
        throw new TypeError('WebAssembly streaming: response type is ' + type);
      }
      if (!mime_is_wasm(resp.headers.get('Content-Type'))) {
        throw new TypeError('WebAssembly streaming: Unexpected MIME type');
      }
      if (!resp.ok) {
        throw new TypeError('WebAssembly streaming: HTTP status is not ok');
      }
      if (resp.bodyUsed) {
        throw new TypeError('WebAssembly streaming: body has already been used');
      }
      return resp.arrayBuffer().then(function (buffer) {
        __lumen_wasm_stream_finish(id, buffer, resp.url);
      });
    }).catch(function (err) {
      __lumen_wasm_stream_abort(id, err);
    });
  };
})();
"#;

/// Ставит изолят-колбэк стриминга. Вызывается из [`super::thread`] сразу после
/// `Isolate::new`, до создания контекста — иначе V8 не заведёт
/// `compileStreaming`/`instantiateStreaming` в этом изоляте вовсе.
pub(crate) fn install_streaming_callback_on_isolate(isolate: &mut v8::Isolate) {
    // Замыкание, а не fn-item: выводимая HRTB-граница требует одного времени
    // жизни у scope и `Local<Value>` — fn-item с элайзами его не даёт
    // (образец — tests `wasm_streaming_callback` самого rusty_v8).
    isolate.set_wasm_streaming_callback(
        |scope: &mut v8::PinScope,
         source: v8::Local<v8::Value>,
         ws: v8::WasmStreaming<false>| {
            wasm_streaming_callback(scope, source, ws);
        },
    );
}

/// Колбэк `WebAssembly.compileStreaming`/`instantiateStreaming`: спрятать
/// `WasmStreaming` в реестр под новым id и запустить JS-помощник. Само
/// разбиение source на байты делает помощник (нужен доступ к `Response`).
fn wasm_streaming_callback(
    scope: &mut v8::PinScope,
    source: v8::Local<v8::Value>,
    ws: v8::WasmStreaming<false>,
) {
    let ctx = scope.get_current_context();
    let global = ctx.global(scope);
    let key = v8::String::new(scope, "__lumen_wasm_stream_start");
    let start = key
        .and_then(|k| global.get(scope, k.into()))
        .and_then(|v| v8::Local::<v8::Function>::try_from(v).ok());
    let Some(start) = start else {
        // Помощник не установлен: обещание обязано отклониться, а не повиснуть.
        let message = v8::String::new(
            scope,
            "WebAssembly streaming: __lumen_wasm_stream_start is not installed",
        );
        ws.abort(message.map(|m| v8::Exception::type_error(scope, m)));
        return;
    };
    let id = NEXT_ID.with(|c| c.replace(c.get().wrapping_add(1)));
    STREAMS.with(|m| m.borrow_mut().insert(id, ws));
    let id_value = v8::Number::new(scope, f64::from(id));
    if start
        .call(scope, global.into(), &[source, id_value.into()])
        .is_none()
    {
        // Помощник по построению не бросает синхронно (только Promise-цепочка);
        // defensive-ветка всё равно отклоняет стрим, чтобы обещание не повисло.
        let message = v8::String::new(
            scope,
            "WebAssembly streaming: __lumen_wasm_stream_start failed",
        );
        if let Some(ws) = take_stream(id) {
            ws.abort(message.map(|m| v8::Exception::type_error(scope, m)));
        }
    }
}

/// `__lumen_wasm_stream_finish(id, buffer, url)` — скормить V8 весь буфер.
/// `set_url` — до байт (требование V8), чтение байт — сырой срез хранилища
/// буфера: `on_bytes_received` копирует их синхронно, ренатрея нет.
fn wasm_stream_finish_native(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    _rv: &mut v8::ReturnValue,
) {
    let Some(id) = stream_id(scope, args) else { return };
    let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(args.get(1)) else {
        return;
    };
    let url = args.get(2).to_rust_string_lossy(scope);
    let Some(mut ws) = take_stream(id) else { return };
    ws.set_url(&url);
    let length = buffer.byte_length();
    let bytes: &[u8] = match buffer.data() {
        // SAFETY: `ptr` — живое хранилище `buffer` на `length` байт; буфер
        // удержан аргументом натива и не может отсоединиться под нами, пока
        // синхронный `on_bytes_received` копирует байты (JS не исполняется).
        Some(ptr) => unsafe { std::slice::from_raw_parts(ptr.as_ptr() as *const u8, length) },
        None => &[],
    };
    ws.on_bytes_received(bytes);
    ws.finish();
}

/// `__lumen_wasm_stream_abort(id, err)` — отклонить обещание стриминга тем же
/// значением, каким отвергся исходный промис или упали проверки помощника.
fn wasm_stream_abort_native(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    _rv: &mut v8::ReturnValue,
) {
    let Some(id) = stream_id(scope, args) else { return };
    let err = args.get(1);
    if let Some(ws) = take_stream(id) {
        ws.abort(Some(err));
    }
}

/// id из первого аргумента натива (`Number` от помощника).
fn stream_id(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments) -> Option<u32> {
    let n = args.get(0).number_value(scope)?;
    let id = u32::try_from(n as i64).ok()?;
    Some(id)
}

fn take_stream(id: u32) -> Option<v8::WasmStreaming<false>> {
    STREAMS.with(|m| m.borrow_mut().remove(&id))
}

/// Регистрация нативов и JS-помощника на рантайме (окно — из `install_dom`,
/// воркеры — из `install_worker_scope_globals_v8`). Помощник трогает
/// `Response` только внутри тела функции, поэтому его можно ставить до
/// worker_net-шима — как раньше ставился шим WebAssembly целиком.
pub(crate) fn install_wasm_streaming_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.register_native_scoped(
        "__lumen_wasm_stream_finish",
        Box::new(wasm_stream_finish_native),
    )?;
    rt.register_native_scoped(
        "__lumen_wasm_stream_abort",
        Box::new(wasm_stream_abort_native),
    )?;
    rt.eval(WASM_STREAMING_HELPER)?;
    Ok(())
}
