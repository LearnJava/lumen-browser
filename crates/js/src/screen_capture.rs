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
use std::sync::{
    Arc, OnceLock, RwLock,
    atomic::{AtomicU64, Ordering},
};

use rquickjs::{Ctx, Function};

use lumen_core::ext::{ScreenCaptureConfig, ScreenCaptureHandle, ScreenCaptureProvider};

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
fn get_provider() -> Option<Arc<dyn ScreenCaptureProvider>> {
    provider_lock().read().ok()?.clone()
}

// ── Per-JS-thread capture handle storage ────────────────────────────────────

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
/// Call `set_screen_capture_provider` before this function in tests.
/// In production the provider is set once at shell startup.
pub fn install_screen_capture_bindings(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let provider = get_provider();
    let g = ctx.globals();

    // __lumen_screen_capture_list_sources() → JSON array of source descriptors
    {
        let p = provider.clone();
        g.set(
            "__lumen_screen_capture_list_sources",
            Function::new(ctx.clone(), move || -> String {
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
            }),
        )?;
    }

    // __lumen_screen_capture_start(source_id) → handle_id (f64) or -1 on error
    {
        let p = provider.clone();
        g.set(
            "__lumen_screen_capture_start",
            Function::new(ctx.clone(), move |source_id: String| -> f64 {
                let Some(ref prov) = p else {
                    return -1.0;
                };
                let config = ScreenCaptureConfig {
                    source_id: if source_id.is_empty() {
                        None
                    } else {
                        Some(source_id)
                    },
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
            }),
        )?;
    }

    // __lumen_screen_capture_info(handle_id) → JSON {width, height, source_id, label}
    g.set(
        "__lumen_screen_capture_info",
        Function::new(ctx.clone(), |handle_id: f64| -> String {
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
        }),
    )?;

    // __lumen_screen_capture_read_frame(handle_id) → JSON {width,height,data:[u8,…]} or ""
    g.set(
        "__lumen_screen_capture_read_frame",
        Function::new(ctx.clone(), |handle_id: f64| -> String {
            CAPTURES.with(|c| {
                let mut map = c.borrow_mut();
                if let Some(h) = map.get_mut(&(handle_id as u64)) {
                    match h.read_frame() {
                        Some(frame) => {
                            let mut out = format!(
                                r#"{{"width":{},"height":{},"data":["#,
                                frame.width, frame.height
                            );
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
        }),
    )?;

    // __lumen_screen_capture_stop(handle_id)
    g.set(
        "__lumen_screen_capture_stop",
        Function::new(ctx.clone(), |handle_id: f64| {
            CAPTURES.with(|c| {
                let mut map = c.borrow_mut();
                if let Some(mut h) = map.remove(&(handle_id as u64)) {
                    h.stop();
                }
            });
        }),
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::{
        NullScreenCaptureProvider, ScreenCaptureError, ScreenSourceDescriptor, VideoFrame,
    };
    use rquickjs::{Context, Runtime};

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

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install_mock(ctx: &Context, fail: bool) {
        set_screen_capture_provider(Arc::new(MockProvider { fail }));
        ctx.with(|ctx| install_screen_capture_bindings(&ctx).unwrap());
    }

    #[test]
    fn install_succeeds() {
        let (_rt, ctx) = make_ctx();
        install_mock(&ctx, false);
    }

    #[test]
    fn list_sources_returns_json_array() {
        let (_rt, ctx) = make_ctx();
        install_mock(&ctx, false);
        ctx.with(|ctx| {
            let json: String = ctx.eval("__lumen_screen_capture_list_sources()").unwrap();
            assert!(json.starts_with('['), "expected JSON array: {json}");
            assert!(json.contains("mock-screen-0"), "expected source_id: {json}");
            assert!(json.contains("monitor"), "expected kind: {json}");
        });
    }

    #[test]
    fn start_returns_positive_id() {
        let (_rt, ctx) = make_ctx();
        install_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_screen_capture_start('')").unwrap();
            assert!(id >= 1.0, "expected positive handle id, got {id}");
        });
    }

    #[test]
    fn start_fails_when_provider_denies() {
        let (_rt, ctx) = make_ctx();
        install_mock(&ctx, true);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_screen_capture_start('')").unwrap();
            assert_eq!(id, -1.0, "expected -1 on capture failure, got {id}");
        });
    }

    #[test]
    fn info_returns_json() {
        let (_rt, ctx) = make_ctx();
        install_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_screen_capture_start('')").unwrap();
            assert!(id >= 1.0);
            let code = format!("__lumen_screen_capture_info({id})");
            let info: String = ctx.eval(code.as_str()).unwrap();
            assert!(info.contains("\"width\":4"), "expected width in info: {info}");
            assert!(info.contains("mock-screen-0"), "expected source_id in info: {info}");
            assert!(info.contains("Mock Screen"), "expected label in info: {info}");
        });
    }

    #[test]
    fn read_frame_returns_json_with_data() {
        let (_rt, ctx) = make_ctx();
        install_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_screen_capture_start('')").unwrap();
            let code = format!("__lumen_screen_capture_read_frame({id})");
            let json: String = ctx.eval(code.as_str()).unwrap();
            assert!(!json.is_empty(), "read_frame must return non-empty string");
            assert!(json.contains("\"width\":4"), "expected width in frame: {json}");
            assert!(json.contains("\"height\":2"), "expected height in frame: {json}");
            assert!(json.contains("255"), "expected pixel data in frame: {json}");
        });
    }

    #[test]
    fn stop_removes_handle() {
        let (_rt, ctx) = make_ctx();
        install_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_screen_capture_start('')").unwrap();
            let stop = format!("__lumen_screen_capture_stop({id})");
            ctx.eval::<(), _>(stop.as_str()).unwrap();
            let info = format!("__lumen_screen_capture_info({id})");
            let result: String = ctx.eval(info.as_str()).unwrap();
            assert_eq!(result, "{}", "info after stop must be empty: {result}");
        });
    }

    #[test]
    fn read_frame_empty_after_stop() {
        let (_rt, ctx) = make_ctx();
        install_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_screen_capture_start('')").unwrap();
            let stop = format!("__lumen_screen_capture_stop({id})");
            ctx.eval::<(), _>(stop.as_str()).unwrap();
            let code = format!("__lumen_screen_capture_read_frame({id})");
            let frame: String = ctx.eval(code.as_str()).unwrap();
            assert!(frame.is_empty(), "read_frame after stop must be empty: {frame}");
        });
    }

    #[test]
    fn null_provider_list_sources_returns_empty_array() {
        let (_rt, ctx) = make_ctx();
        set_screen_capture_provider(Arc::new(NullScreenCaptureProvider));
        ctx.with(|ctx| {
            install_screen_capture_bindings(&ctx).unwrap();
            let json: String = ctx.eval("__lumen_screen_capture_list_sources()").unwrap();
            assert_eq!(json, "[]", "null provider must enumerate empty: {json}");
        });
    }

    #[test]
    fn null_provider_start_returns_minus_one() {
        let (_rt, ctx) = make_ctx();
        set_screen_capture_provider(Arc::new(NullScreenCaptureProvider));
        ctx.with(|ctx| {
            install_screen_capture_bindings(&ctx).unwrap();
            let id: f64 = ctx.eval("__lumen_screen_capture_start('')").unwrap();
            assert_eq!(id, -1.0, "null provider must return -1: {id}");
        });
    }

    #[test]
    fn start_with_explicit_source_id_succeeds() {
        let (_rt, ctx) = make_ctx();
        install_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 =
                ctx.eval("__lumen_screen_capture_start('mock-screen-0')").unwrap();
            assert!(id >= 1.0, "expected positive handle with explicit source_id: {id}");
        });
    }
}
