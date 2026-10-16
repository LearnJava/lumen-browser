//! Media Source Extensions (COMPAT-MSE): нативные привязки `__lumen_mse_*` и шим
//! `MediaSource` / `SourceBuffer` / `SourceBufferList` (`shim/media_source.js`).
//!
//! Страница держит JS-объекты и события; байты, разбор сегментов и `buffered` живут
//! в `lumen-media-mse` (общий реестр `MediaSource` по числовому номеру), а
//! воспроизведение читает те же кадры из оболочки (`lumen_media_native::open_mse`).
//! Как шим связывается с `<video>` — `startMseLoad` в `shim/video_element.js`.
//!
//! | Нативная функция | Назначение |
//! |---|---|
//! | `__lumen_mse_create() → id` | новый `MediaSource` |
//! | `__lumen_mse_add_buffer(id, mime) → index` | `addSourceBuffer`; `-1` — тип не поддержан |
//! | `__lumen_mse_append(id, index, Uint8Array) → string` | `""` — принято; `"init"` — принято с init-сегментом; `"error:…"` — ошибка разбора |
//! | `__lumen_mse_buffered(id, index) → json` | `[[start, end], …]` в секундах |
//! | `__lumen_mse_remove(id, index, start, end)` | `remove()` |
//! | `__lumen_mse_abort(id, index)` | `abort()` |
//! | `__lumen_mse_set_*` | `timestampOffset`, `mode`, окно добавления, `duration` |
//! | `__lumen_mse_end_of_stream(id, ended)` | `endOfStream()` |
//! | `__lumen_mse_attach(nid, id)` | привязать `MediaSource` к `<video>` (оболочка откроет сессию) |
//! | `__lumen_mse_is_type_supported(mime)` | `MediaSource.isTypeSupported` по реестру декодеров |
//! | `__lumen_mse_release(id)` | страница отпустила объект |

#[cfg(feature = "v8-backend")]
use lumen_media_mse::{AppendMode, parse_mime, source};

#[cfg(feature = "v8-backend")]
use crate::video_gif_store::{get_video_decoder, get_video_gif_store};

/// Секунды → микросекунды (NaN и бесконечности — `None`).
#[cfg(feature = "v8-backend")]
fn secs_to_us(secs: f64) -> Option<i64> {
    secs.is_finite().then(|| (secs * 1_000_000.0).round() as i64)
}

/// `isTypeSupported` по факту: контейнер MP4/WebM и (если названы кодеки) каждый из
/// них открывается установленным декодером.
#[cfg(feature = "v8-backend")]
fn type_supported(mime: &str) -> bool {
    let Ok(parsed) = parse_mime(mime) else { return false };
    let Some(decoder) = get_video_decoder() else { return false };
    let answer = decoder.can_play_type(mime);
    if parsed.codecs.is_empty() { !answer.is_empty() } else { answer == "probably" }
}

/// Тело `__lumen_mse_append`: байты берутся прямо из `Uint8Array`, без пересылки
/// через массив чисел (сегменты весят мегабайты).
#[cfg(feature = "v8-backend")]
fn append_native(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, rv: &mut v8::ReturnValue) {
    let id = args.get(0).number_value(scope).unwrap_or(-1.0);
    let index = args.get(1).number_value(scope).unwrap_or(-1.0);
    let status = match v8::Local::<v8::ArrayBufferView>::try_from(args.get(2)) {
        Ok(view) => {
            let mut bytes = vec![0u8; view.byte_length()];
            view.copy_contents(&mut bytes);
            append_bytes(id, index, &bytes)
        }
        Err(_) => "error:not-bytes".to_string(),
    };
    if let Some(s) = v8::String::new(scope, &status) {
        rv.set(s.into());
    }
}

#[cfg(feature = "v8-backend")]
fn append_bytes(id: f64, index: f64, bytes: &[u8]) -> String {
    let Some(shared) = source(id as u64) else { return "error:no-source".to_string() };
    let mut core = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(buffer) = core.buffer_mut(index as usize) else { return "error:no-buffer".to_string() };
    match buffer.append(bytes) {
        Ok(outcome) if outcome.init_received => "init".to_string(),
        Ok(_) => String::new(),
        Err(e) => format!("error:{e}"),
    }
}

/// Привязки MSE: N нативных функций и шим `MediaSource`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_mse_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4};
    use lumen_core::ext::JsRuntime as _;

    rt.register_native("__lumen_mse_create", into_v8_fn0(|| -> f64 { lumen_media_mse::create_source() as f64 }))?;
    rt.register_native(
        "__lumen_mse_release",
        into_v8_fn1(|id: f64| {
            lumen_media_mse::drop_source(id as u64);
        }),
    )?;
    rt.register_native(
        "__lumen_mse_add_buffer",
        into_v8_fn2(|id: f64, mime: String| -> f64 {
            if !type_supported(&mime) {
                return -1.0;
            }
            let Some(shared) = source(id as u64) else { return -1.0 };
            let mut core = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            core.add_buffer(&mime).map_or(-1.0, |i| i as f64)
        }),
    )?;
    rt.register_native_scoped("__lumen_mse_append", Box::new(append_native))?;
    rt.register_native(
        "__lumen_mse_buffered",
        into_v8_fn2(|id: f64, index: f64| -> String {
            let spans = source(id as u64)
                .and_then(|s| {
                    let core = s.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    core.buffer(index as usize).map(lumen_media_mse::SourceBufferModel::buffered_us)
                })
                .unwrap_or_default();
            let json: Vec<String> =
                spans.iter().map(|&(s, e)| format!("[{},{}]", s as f64 / 1e6, e as f64 / 1e6)).collect();
            format!("[{}]", json.join(","))
        }),
    )?;
    rt.register_native(
        "__lumen_mse_remove",
        into_v8_fn4(|id: f64, index: f64, start: f64, end: f64| {
            let (Some(shared), Some(s)) = (source(id as u64), secs_to_us(start)) else { return };
            let e = secs_to_us(end).unwrap_or(i64::MAX);
            let mut core = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(b) = core.buffer_mut(index as usize) {
                b.remove(s, e);
            }
        }),
    )?;
    rt.register_native(
        "__lumen_mse_abort",
        into_v8_fn2(|id: f64, index: f64| {
            let Some(shared) = source(id as u64) else { return };
            let mut core = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(b) = core.buffer_mut(index as usize) {
                b.abort();
            }
        }),
    )?;
    rt.register_native(
        "__lumen_mse_set_timestamp_offset",
        into_v8_fn3(|id: f64, index: f64, secs: f64| {
            let Some(shared) = source(id as u64) else { return };
            let mut core = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let (Some(b), Some(us)) = (core.buffer_mut(index as usize), secs_to_us(secs)) {
                b.set_timestamp_offset_us(us);
            }
        }),
    )?;
    rt.register_native(
        "__lumen_mse_set_sequence_mode",
        into_v8_fn3(|id: f64, index: f64, sequence: bool| {
            let Some(shared) = source(id as u64) else { return };
            let mut core = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(b) = core.buffer_mut(index as usize) {
                b.set_mode(if sequence { AppendMode::Sequence } else { AppendMode::Segments });
            }
        }),
    )?;
    rt.register_native(
        "__lumen_mse_set_append_window",
        into_v8_fn4(|id: f64, index: f64, start: f64, end: f64| {
            let Some(shared) = source(id as u64) else { return };
            let mut core = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(b) = core.buffer_mut(index as usize) {
                b.set_append_window_us(secs_to_us(start).unwrap_or(0), secs_to_us(end).unwrap_or(i64::MAX));
            }
        }),
    )?;
    rt.register_native(
        "__lumen_mse_set_duration",
        into_v8_fn2(|id: f64, secs: f64| {
            if let Some(shared) = source(id as u64) {
                shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner).set_duration_us(secs_to_us(secs));
            }
        }),
    )?;
    rt.register_native(
        "__lumen_mse_end_of_stream",
        into_v8_fn2(|id: f64, ended: bool| {
            if let Some(shared) = source(id as u64) {
                shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner).set_ended(ended);
            }
        }),
    )?;
    rt.register_native(
        "__lumen_mse_is_type_supported",
        into_v8_fn1(|mime: String| -> bool { type_supported(&mime) }),
    )?;
    {
        let store = get_video_gif_store();
        rt.register_native(
            "__lumen_mse_attach",
            into_v8_fn2(move |nid: f64, id: f64| {
                if let Some(s) = &store {
                    s.pending_mse_attach
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((nid as u32, id as u64));
                }
            }),
        )?;
    }
    rt.eval(MEDIA_SOURCE_SHIM)?;
    Ok(())
}

/// Шим `MediaSource` / `SourceBuffer` / `SourceBufferList`, как есть из `shim/media_source.js`.
#[cfg(feature = "v8-backend")]
const MEDIA_SOURCE_SHIM: &str = include_str!("shim/media_source.js");
