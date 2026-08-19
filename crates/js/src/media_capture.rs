//! Native bindings bridging `navigator.mediaDevices.getUserMedia({audio})` to the
//! platform audio capture backend (`AudioCaptureProvider`).
//!
//! ## Architecture
//!
//! The shell installs a concrete [`AudioCaptureProvider`] via
//! [`set_audio_capture_provider`] before any JS runs.  When the JS shim calls
//! `getUserMedia({audio})`, it invokes the native function
//! `__lumen_start_audio_capture(device_id, sample_rate, channel_count)` which:
//!
//! 1. Reads the process-global provider.
//! 2. Calls `provider.capture(config)` → `AudioCaptureHandle`.
//! 3. Stores the handle in a thread-local `HashMap<u64, Box<dyn AudioCaptureHandle>>`.
//! 4. Returns an opaque `handle_id` (u64 cast to f64 for JS compatibility).
//!
//! The JS shim then creates a `MediaStreamTrack` whose `readPcm` / `stop` methods
//! call back into `__lumen_read_audio_pcm` / `__lumen_stop_audio_capture`.
//!
//! All JS callbacks run on the rquickjs single-threaded event loop, so the
//! thread-local map needs no synchronisation.  The ring buffer inside the handle
//! is written by the cpal capture thread and read by the JS thread via
//! `Arc<Mutex<VecDeque<f32>>>`.
//!
//! ## Installed globals
//!
//! | Name | Signature | Notes |
//! |---|---|---|
//! | `__lumen_enumerate_audio_devices` | `() → String` | JSON array of `AudioDeviceDescriptor` |
//! | `__lumen_start_audio_capture` | `(device_id: String, sample_rate: f64, channel_count: f64) → f64` | handle id or `-1` on error |
//! | `__lumen_audio_capture_info` | `(handle_id: f64) → String` | JSON `{sample_rate, channel_count, device_id, label}` |
//! | `__lumen_read_audio_pcm` | `(handle_id: f64, max_samples: f64) → String` | JSON `[f32, …]` |
//! | `__lumen_stop_audio_capture` | `(handle_id: f64) → ()` | stops and removes the handle |

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
#[cfg(feature = "v8-backend")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "v8-backend")]
use lumen_core::ext::AudioCaptureConfig;
use lumen_core::ext::{AudioCaptureHandle, AudioCaptureProvider};

// ── Process-global provider ──────────────────────────────────────────────────

static PROVIDER: OnceLock<RwLock<Option<Arc<dyn AudioCaptureProvider>>>> = OnceLock::new();

fn provider_lock() -> &'static RwLock<Option<Arc<dyn AudioCaptureProvider>>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Install the platform audio capture backend.
///
/// Must be called once by the shell before any JS context is created.
/// Subsequent calls from the same process overwrite the previous provider.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn set_audio_capture_provider(p: Arc<dyn AudioCaptureProvider>) {
    *provider_lock().write().unwrap() = Some(p);
}

/// Return the currently installed provider, or `None` when none is registered.
#[cfg(feature = "v8-backend")]
fn get_provider() -> Option<Arc<dyn AudioCaptureProvider>> {
    provider_lock().read().ok()?.clone()
}

// ── Per-JS-thread capture handle storage ────────────────────────────────────

#[cfg(feature = "v8-backend")]
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// Active capture handles keyed by opaque ID.
    ///
    /// Stored in a thread-local so the rquickjs closures (which must be `'static + Send`)
    /// can access them without holding a cross-thread lock.  All JS callbacks that touch
    /// this map run on the same single-threaded rquickjs event loop.
    static CAPTURES: RefCell<HashMap<u64, Box<dyn AudioCaptureHandle>>> =
        RefCell::new(HashMap::new());
}

// ── Native function installation ─────────────────────────────────────────────

/// Install `__lumen_*` audio capture natives into the JS context.
///
/// The provider registered via [`set_audio_capture_provider`] at the time of this
/// call is snapshotted into the native closures so they don't re-read the global
/// on every call; in tests, call `set_audio_capture_provider` **before** this
/// function. All five natives go through the V8 compat layer and share the
/// thread-local `CAPTURES` map — sound because every JS call, including
/// trampoline-invoked natives, dispatches from the one dedicated JS thread
/// (see slice S1's threading model).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_media_capture_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3};

    let provider = get_provider();

    {
        let p = provider.clone();
        let native = into_v8_fn0(move || -> String {
            let Some(ref prov) = p else {
                return "[]".to_owned();
            };
            let devs = prov.enumerate_devices();
            let mut out = String::from('[');
            for (i, d) in devs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    r#"{{"device_id":{:?},"group_id":{:?},"kind":{:?},"label":{:?},"is_default":{}}}"#,
                    d.device_id, d.group_id, d.kind, d.label, d.is_default
                ));
            }
            out.push(']');
            out
        });
        rt.register_native("__lumen_enumerate_audio_devices", native)?;
    }

    {
        let p = provider.clone();
        let native = into_v8_fn3(
            move |device_id: String, sample_rate: f64, channel_count: f64| -> f64 {
                let Some(ref prov) = p else {
                    return -1.0;
                };
                let config = AudioCaptureConfig {
                    device_id: if device_id.is_empty() { None } else { Some(device_id) },
                    sample_rate: if sample_rate > 0.0 { Some(sample_rate as u32) } else { None },
                    channel_count: if channel_count > 0.0 {
                        Some(channel_count as u32)
                    } else {
                        None
                    },
                    ..AudioCaptureConfig::default()
                };
                match prov.capture(config) {
                    Ok(handle) => {
                        let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed);
                        CAPTURES.with(|c| c.borrow_mut().insert(id, handle));
                        id as f64
                    }
                    Err(_) => -1.0,
                }
            },
        );
        rt.register_native("__lumen_start_audio_capture", native)?;
    }

    let info = into_v8_fn1(|handle_id: f64| -> String {
        CAPTURES.with(|c| {
            let map = c.borrow();
            if let Some(h) = map.get(&(handle_id as u64)) {
                format!(
                    r#"{{"sample_rate":{},"channel_count":{},"device_id":{:?},"label":{:?}}}"#,
                    h.sample_rate(),
                    h.channel_count(),
                    h.device_id(),
                    h.device_label(),
                )
            } else {
                "{}".to_owned()
            }
        })
    });
    rt.register_native("__lumen_audio_capture_info", info)?;

    let read_pcm = into_v8_fn2(|handle_id: f64, max_samples: f64| -> String {
        CAPTURES.with(|c| {
            let mut map = c.borrow_mut();
            if let Some(h) = map.get_mut(&(handle_id as u64)) {
                let samples = h.read_pcm_f32();
                let limit = (max_samples as usize).min(samples.len());
                if limit == 0 {
                    return "[]".to_owned();
                }
                let mut out = String::from('[');
                for (i, &s) in samples[..limit].iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    let clamped = s.clamp(-1.0, 1.0);
                    out.push_str(&format!("{clamped:.7}"));
                }
                out.push(']');
                out
            } else {
                "[]".to_owned()
            }
        })
    });
    rt.register_native("__lumen_read_audio_pcm", read_pcm)?;

    let stop = into_v8_fn1(|handle_id: f64| {
        CAPTURES.with(|c| {
            let mut map = c.borrow_mut();
            if let Some(mut h) = map.remove(&(handle_id as u64)) {
                h.stop();
            }
        });
    });
    rt.register_native("__lumen_stop_audio_capture", stop)?;

    Ok(())
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // `panic!` — штатный способ провалить тест; исключение из clippy.toml не
    // достаёт до хелперов модуля (docs/lint-policy.md §10).
    #![allow(clippy::panic, clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::JsValue;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::ext::{AudioCaptureError, AudioDeviceDescriptor};
    use std::sync::Mutex;

    /// Serializes tests against the process-global `PROVIDER`/`NEXT_HANDLE_ID` —
    /// parallel tests would otherwise clobber each other's provider between
    /// `set_audio_capture_provider` and the following `install_*` snapshot.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn as_id(v: JsValue) -> f64 {
        match v {
            JsValue::Number(n) => n,
            other => panic!("expected a number, got {other:?}"),
        }
    }

    // ── Mock provider ────────────────────────────────────────────────────────

    struct MockHandle {
        sr: u32,
        ch: u32,
        stopped: bool,
        pending: Vec<f32>,
    }

    impl AudioCaptureHandle for MockHandle {
        fn sample_rate(&self) -> u32 { self.sr }
        fn channel_count(&self) -> u32 { self.ch }
        fn device_id(&self) -> &str { "mock-0" }
        fn device_label(&self) -> &str { "Mock Microphone" }
        fn read_pcm_f32(&mut self) -> Vec<f32> {
            if self.stopped { return Vec::new(); }
            std::mem::take(&mut self.pending)
        }
        fn stop(&mut self) { self.stopped = true; }
    }

    struct MockProvider {
        fail: bool,
    }

    impl AudioCaptureProvider for MockProvider {
        fn enumerate_devices(&self) -> Vec<AudioDeviceDescriptor> {
            vec![AudioDeviceDescriptor {
                device_id: "mock-0".into(),
                group_id: "grp-0".into(),
                label: "Mock Microphone".into(),
                kind: "audioinput",
                is_default: true,
            }]
        }

        fn capture(
            &self,
            _config: AudioCaptureConfig,
        ) -> Result<Box<dyn AudioCaptureHandle>, AudioCaptureError> {
            if self.fail {
                return Err(AudioCaptureError::NotAllowed);
            }
            Ok(Box::new(MockHandle {
                sr: 44100,
                ch: 1,
                stopped: false,
                pending: vec![0.1, -0.2, 0.3],
            }))
        }
    }

    fn install_with_mock(rt: &V8JsRuntime, fail: bool) {
        set_audio_capture_provider(Arc::new(MockProvider { fail }));
        install_media_capture_bindings_v8(rt).unwrap();
    }

    #[test]
    fn install_succeeds() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_with_mock(&rt, false);
    }

    #[test]
    fn enumerate_returns_json_array() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_with_mock(&rt, false);
        let ok = rt
            .eval(
                r#"
                var json = __lumen_enumerate_audio_devices();
                json.charAt(0) === '[' && json.indexOf('mock-0') >= 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn start_capture_returns_positive_id() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_with_mock(&rt, false);
        let id = as_id(rt.eval("__lumen_start_audio_capture('', 0, 0)").unwrap());
        assert!(id >= 1.0, "expected positive handle id, got {id}");
    }

    #[test]
    fn start_capture_fails_when_no_provider_or_denied() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_with_mock(&rt, true);
        let id = rt.eval("__lumen_start_audio_capture('', 0, 0)").unwrap();
        assert_eq!(id, JsValue::Number(-1.0), "expected -1 on capture failure");
    }

    #[test]
    fn capture_info_returns_json() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_with_mock(&rt, false);
        let ok = rt
            .eval(
                r#"
                var id = __lumen_start_audio_capture('', 0, 0);
                var info = __lumen_audio_capture_info(id);
                info.indexOf('44100') >= 0 && info.indexOf('mock-0') >= 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn read_pcm_returns_json_array() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_with_mock(&rt, false);
        // MockHandle has 3 pending samples.
        let len = rt
            .eval(
                r#"
                var id = __lumen_start_audio_capture('', 0, 0);
                var json = __lumen_read_audio_pcm(id, 4096);
                JSON.parse(json).length
                "#,
            )
            .unwrap();
        assert_eq!(len, JsValue::Number(3.0));
    }

    #[test]
    fn stop_removes_handle() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_with_mock(&rt, false);
        let info = rt
            .eval(
                r#"
                var id = __lumen_start_audio_capture('', 0, 0);
                __lumen_stop_audio_capture(id);
                __lumen_audio_capture_info(id)
                "#,
            )
            .unwrap();
        assert_eq!(info, JsValue::String("{}".to_string()), "expected empty info after stop");
    }

    #[test]
    fn read_pcm_max_samples_respected() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_with_mock(&rt, false);
        // Mock has 3 samples; request only 2.
        let len = rt
            .eval(
                r#"
                var id = __lumen_start_audio_capture('', 0, 0);
                var json = __lumen_read_audio_pcm(id, 2);
                JSON.parse(json).length
                "#,
            )
            .unwrap();
        assert_eq!(len, JsValue::Number(2.0), "max_samples=2 should cap at 2");
    }

    // NullAudioCaptureProvider → enumerate returns [] is tested in lumen-core::audio_capture_tests.
    // Omitted here to avoid a global-state race with parallel test threads.
}
