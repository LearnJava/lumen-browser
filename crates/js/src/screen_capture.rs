//! Native bindings bridging `navigator.mediaDevices.getDisplayMedia` to the
//! platform screen capture backend (`ScreenCaptureProvider`).
//!
//! ## Architecture
//!
//! The shell installs a concrete [`ScreenCaptureProvider`] via
//! [`set_screen_capture_provider`] before any JS runs.  When JS calls
//! `getDisplayMedia()`, it invokes `__lumen_screen_capture_start(source_id)`:
//!
//! 1. Reads the process-global provider.
//! 2. Calls `provider.capture(config)` → `ScreenCaptureHandle`.
//! 3. Stores the handle in a thread-local `HashMap<u64, Box<dyn ScreenCaptureHandle>>`.
//! 4. Returns an opaque `handle_id` (u64 cast to f64 for JS compatibility).
//!
//! The JS shim creates a `MediaStream` with a live video `MediaStreamTrack`.
//! Frame data is available via `__lumen_screen_capture_read_frame` (used by Phase 2
//! MediaRecorder and canvas capture).
//!
//! ## Installed globals
//!
//! | Name | Signature | Notes |
//! |---|---|---|
//! | `__lumen_screen_capture_list_sources` | `() → String` | JSON array of source descriptors |
//! | `__lumen_screen_capture_start` | `(source_id: String) → f64` | handle id or `-1` on error |
//! | `__lumen_screen_capture_info` | `(handle_id: f64) → String` | JSON `{width,height,source_id,label}` |
//! | `__lumen_screen_capture_read_frame` | `(handle_id: f64) → String` | JSON `{width,height,data:[u8,…]}` or `""` |
//! | `__lumen_screen_capture_stop` | `(handle_id: f64) → ()` | stops and removes the handle |

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
#[cfg(feature = "v8-backend")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "v8-backend")]
use lumen_core::ext::ScreenCaptureConfig;
use lumen_core::ext::{ScreenCaptureHandle, ScreenCaptureProvider};

// ── Process-global provider ──────────────────────────────────────────────────

static PROVIDER: OnceLock<RwLock<Option<Arc<dyn ScreenCaptureProvider>>>> = OnceLock::new();

fn provider_lock() -> &'static RwLock<Option<Arc<dyn ScreenCaptureProvider>>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Install the platform screen capture backend.
///
/// Must be called once by the shell before any JS context is created.
/// Subsequent calls overwrite the previous provider.
pub fn set_screen_capture_provider(p: Arc<dyn ScreenCaptureProvider>) {
    *provider_lock().write().unwrap() = Some(p);
}

/// Return the currently installed provider, or `None` when none is registered.
#[cfg(feature = "v8-backend")]
fn get_provider() -> Option<Arc<dyn ScreenCaptureProvider>> {
    provider_lock().read().ok()?.clone()
}

// ── Per-JS-thread capture handle storage ────────────────────────────────────

#[cfg(feature = "v8-backend")]
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// Active screen capture handles keyed by opaque ID.
    ///
    /// Thread-local so rquickjs closures (`'static + Send`) can access handles
    /// without cross-thread locking.  All JS callbacks run on the single-threaded
    /// rquickjs event loop.
    static CAPTURES: RefCell<HashMap<u64, Box<dyn ScreenCaptureHandle>>> =
        RefCell::new(HashMap::new());
}

// ── Native function installation ─────────────────────────────────────────────

/// Install `__lumen_screen_capture_*` natives into the JS context.
///
/// Call `set_screen_capture_provider` before this function in tests. In
/// production the provider is set once at shell startup. All five natives go
/// through the V8 compat layer and share the thread-local `CAPTURES` map —
/// sound for the same reason as
/// [`crate::media_capture::install_media_capture_bindings_v8`].
#[cfg(feature = "v8-backend")]
pub(crate) fn install_screen_capture_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1};

    let provider = get_provider();

    {
        let p = provider.clone();
        let native = into_v8_fn0(move || -> String {
            let Some(ref prov) = p else {
                return "[]".to_owned();
            };
            let sources = prov.enumerate_sources();
            let mut out = String::from('[');
            for (i, s) in sources.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    r#"{{"source_id":{:?},"label":{:?},"kind":{:?},"width":{},"height":{}}}"#,
                    s.source_id, s.label, s.kind, s.width, s.height
                ));
            }
            out.push(']');
            out
        });
        rt.register_native("__lumen_screen_capture_list_sources", native)?;
    }

    {
        let p = provider.clone();
        let native = into_v8_fn1(move |source_id: String| -> f64 {
            let Some(ref prov) = p else {
                return -1.0;
            };
            let config = ScreenCaptureConfig {
                source_id: if source_id.is_empty() { None } else { Some(source_id) },
                ..ScreenCaptureConfig::default()
            };
            match prov.capture(config) {
                Ok(handle) => {
                    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed);
                    CAPTURES.with(|c| c.borrow_mut().insert(id, handle));
                    id as f64
                }
                Err(_) => -1.0,
            }
        });
        rt.register_native("__lumen_screen_capture_start", native)?;
    }

    let info = into_v8_fn1(|handle_id: f64| -> String {
        CAPTURES.with(|c| {
            let map = c.borrow();
            if let Some(h) = map.get(&(handle_id as u64)) {
                format!(
                    r#"{{"width":{},"height":{},"source_id":{:?},"label":{:?}}}"#,
                    h.width(),
                    h.height(),
                    h.source_id(),
                    h.label(),
                )
            } else {
                "{}".to_owned()
            }
        })
    });
    rt.register_native("__lumen_screen_capture_info", info)?;

    let read_frame = into_v8_fn1(|handle_id: f64| -> String {
        CAPTURES.with(|c| {
            let mut map = c.borrow_mut();
            if let Some(h) = map.get_mut(&(handle_id as u64)) {
                match h.read_frame() {
                    Some(frame) => {
                        let mut out =
                            format!(r#"{{"width":{},"height":{},"data":["#, frame.width, frame.height);
                        for (i, &b) in frame.data.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            out.push_str(&b.to_string());
                        }
                        out.push_str("]}");
                        out
                    }
                    None => String::new(),
                }
            } else {
                String::new()
            }
        })
    });
    rt.register_native("__lumen_screen_capture_read_frame", read_frame)?;

    let stop = into_v8_fn1(|handle_id: f64| {
        CAPTURES.with(|c| {
            let mut map = c.borrow_mut();
            if let Some(mut h) = map.remove(&(handle_id as u64)) {
                h.stop();
            }
        });
    });
    rt.register_native("__lumen_screen_capture_stop", stop)?;

    Ok(())
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::JsValue;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::ext::{
        NullScreenCaptureProvider, ScreenCaptureError, ScreenSourceDescriptor, VideoFrame,
    };
    use std::sync::Mutex;

    /// Serializes tests against the process-global `PROVIDER`/`NEXT_HANDLE_ID` —
    /// parallel tests would otherwise clobber each other's provider between
    /// `set_screen_capture_provider` and the following `install_*` snapshot.
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
        source_id: String,
        stopped: bool,
    }

    impl ScreenCaptureHandle for MockHandle {
        fn width(&self) -> u32 { 4 }
        fn height(&self) -> u32 { 2 }
        fn source_id(&self) -> &str { &self.source_id }
        fn label(&self) -> &str { "Mock Screen" }

        fn read_frame(&mut self) -> Option<VideoFrame> {
            if self.stopped {
                return None;
            }
            // 4×2 RGBA, all red pixels.
            let mut data = Vec::with_capacity(4 * 2 * 4);
            for _ in 0..8 {
                data.extend_from_slice(&[255u8, 0, 0, 255]);
            }
            Some(VideoFrame { width: 4, height: 2, data })
        }

        fn stop(&mut self) { self.stopped = true; }
    }

    struct MockProvider { fail: bool }

    impl ScreenCaptureProvider for MockProvider {
        fn enumerate_sources(&self) -> Vec<ScreenSourceDescriptor> {
            vec![ScreenSourceDescriptor {
                source_id: "mock-screen-0".into(),
                label: "Mock Screen".into(),
                kind: "monitor",
                width: 1920,
                height: 1080,
            }]
        }

        fn capture(
            &self,
            _config: ScreenCaptureConfig,
        ) -> Result<Box<dyn ScreenCaptureHandle>, ScreenCaptureError> {
            if self.fail {
                return Err(ScreenCaptureError::NotAllowed);
            }
            Ok(Box::new(MockHandle { source_id: "mock-screen-0".into(), stopped: false }))
        }
    }

    fn install_mock(rt: &V8JsRuntime, fail: bool) {
        set_screen_capture_provider(Arc::new(MockProvider { fail }));
        install_screen_capture_bindings_v8(rt).unwrap();
    }

    #[test]
    fn install_succeeds() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_mock(&rt, false);
    }

    #[test]
    fn list_sources_returns_json_array() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_mock(&rt, false);
        let ok = rt
            .eval(
                r#"
                var json = __lumen_screen_capture_list_sources();
                json.charAt(0) === '[' && json.indexOf('mock-screen-0') >= 0 && json.indexOf('monitor') >= 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn start_returns_positive_id() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_mock(&rt, false);
        let id = as_id(rt.eval("__lumen_screen_capture_start('')").unwrap());
        assert!(id >= 1.0, "expected positive handle id, got {id}");
    }

    #[test]
    fn start_fails_when_provider_denies() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_mock(&rt, true);
        let id = rt.eval("__lumen_screen_capture_start('')").unwrap();
        assert_eq!(id, JsValue::Number(-1.0), "expected -1 on capture failure");
    }

    #[test]
    fn info_returns_json() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_mock(&rt, false);
        let ok = rt
            .eval(
                r#"
                var id = __lumen_screen_capture_start('');
                var info = __lumen_screen_capture_info(id);
                info.indexOf('"width":4') >= 0 && info.indexOf('mock-screen-0') >= 0 && info.indexOf('Mock Screen') >= 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn read_frame_returns_json_with_data() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_mock(&rt, false);
        let ok = rt
            .eval(
                r#"
                var id = __lumen_screen_capture_start('');
                var json = __lumen_screen_capture_read_frame(id);
                json.length > 0
                  && json.indexOf('"width":4') >= 0
                  && json.indexOf('"height":2') >= 0
                  && json.indexOf('255') >= 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn stop_removes_handle() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_mock(&rt, false);
        let info = rt
            .eval(
                r#"
                var id = __lumen_screen_capture_start('');
                __lumen_screen_capture_stop(id);
                __lumen_screen_capture_info(id)
                "#,
            )
            .unwrap();
        assert_eq!(info, JsValue::String("{}".to_string()), "info after stop must be empty");
    }

    #[test]
    fn read_frame_empty_after_stop() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_mock(&rt, false);
        let frame = rt
            .eval(
                r#"
                var id = __lumen_screen_capture_start('');
                __lumen_screen_capture_stop(id);
                __lumen_screen_capture_read_frame(id)
                "#,
            )
            .unwrap();
        assert_eq!(frame, JsValue::String(String::new()), "read_frame after stop must be empty");
    }

    #[test]
    fn null_provider_list_sources_returns_empty_array() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        set_screen_capture_provider(Arc::new(NullScreenCaptureProvider));
        install_screen_capture_bindings_v8(&rt).unwrap();
        let json = rt.eval("__lumen_screen_capture_list_sources()").unwrap();
        assert_eq!(json, JsValue::String("[]".to_string()), "null provider must enumerate empty");
    }

    #[test]
    fn null_provider_start_returns_minus_one() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        set_screen_capture_provider(Arc::new(NullScreenCaptureProvider));
        install_screen_capture_bindings_v8(&rt).unwrap();
        let id = rt.eval("__lumen_screen_capture_start('')").unwrap();
        assert_eq!(id, JsValue::Number(-1.0), "null provider must return -1");
    }

    #[test]
    fn start_with_explicit_source_id_succeeds() {
        let _g = guard();
        let rt = V8JsRuntime::new().unwrap();
        install_mock(&rt, false);
        let id = as_id(rt.eval("__lumen_screen_capture_start('mock-screen-0')").unwrap());
        assert!(id >= 1.0, "expected positive handle with explicit source_id, got {id}");
    }
}
