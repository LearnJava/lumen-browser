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
use std::sync::{Arc, OnceLock, RwLock, atomic::{AtomicU64, Ordering}};

use rquickjs::{Ctx, Function};

use lumen_core::ext::{AudioCaptureConfig, AudioCaptureHandle, AudioCaptureProvider};

// ── Process-global provider ──────────────────────────────────────────────────

static PROVIDER: OnceLock<RwLock<Option<Arc<dyn AudioCaptureProvider>>>> = OnceLock::new();

fn provider_lock() -> &'static RwLock<Option<Arc<dyn AudioCaptureProvider>>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Install the platform audio capture backend.
///
/// Must be called once by the shell before any JS context is created.
/// Subsequent calls from the same process overwrite the previous provider.
pub fn set_audio_capture_provider(p: Arc<dyn AudioCaptureProvider>) {
    *provider_lock().write().unwrap() = Some(p);
}

/// Return the currently installed provider, or `None` when none is registered.
fn get_provider() -> Option<Arc<dyn AudioCaptureProvider>> {
    provider_lock().read().ok()?.clone()
}

// ── Per-JS-thread capture handle storage ────────────────────────────────────

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
/// call is captured by value into the JS closures.  In production the provider is
/// set once at shell startup and never changes, so the snapshot is always correct.
/// In tests, call `set_audio_capture_provider` **before** calling this function.
pub fn install_media_capture_bindings(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    // Snapshot the current provider so closures don't read the global on every call.
    // This avoids races in tests where multiple test threads share the OnceLock.
    let provider = get_provider();
    let g = ctx.globals();

    // __lumen_enumerate_audio_devices() → JSON string
    {
        let p = provider.clone();
        g.set(
            "__lumen_enumerate_audio_devices",
            Function::new(ctx.clone(), move || -> String {
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
            }),
        )?;
    }

    // __lumen_start_audio_capture(device_id, sample_rate, channel_count) → handle_id (f64) or -1
    {
        let p = provider.clone();
        g.set(
            "__lumen_start_audio_capture",
            Function::new(
                ctx.clone(),
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
            ),
        )?;
    }

    // __lumen_audio_capture_info(handle_id) → JSON string {sample_rate, channel_count, device_id, label}
    g.set(
        "__lumen_audio_capture_info",
        Function::new(ctx.clone(), |handle_id: f64| -> String {
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
        }),
    )?;

    // __lumen_read_audio_pcm(handle_id, max_samples) → JSON array string "[f32, …]"
    g.set(
        "__lumen_read_audio_pcm",
        Function::new(ctx.clone(), |handle_id: f64, max_samples: f64| -> String {
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
                        // Use fixed precision to avoid "NaN"/"Inf" that would break JSON.parse
                        let clamped = s.clamp(-1.0, 1.0);
                        out.push_str(&format!("{clamped:.7}"));
                    }
                    out.push(']');
                    out
                } else {
                    "[]".to_owned()
                }
            })
        }),
    )?;

    // __lumen_stop_audio_capture(handle_id)
    g.set(
        "__lumen_stop_audio_capture",
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
    use lumen_core::ext::{AudioCaptureError, AudioDeviceDescriptor};
    use rquickjs::{Context, Runtime};

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

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install_with_mock(ctx: &Context, fail: bool) {
        set_audio_capture_provider(Arc::new(MockProvider { fail }));
        ctx.with(|ctx| install_media_capture_bindings(&ctx).unwrap());
    }

    #[test]
    fn install_succeeds() {
        let (_rt, ctx) = make_ctx();
        install_with_mock(&ctx, false);
    }

    #[test]
    fn enumerate_returns_json_array() {
        let (_rt, ctx) = make_ctx();
        install_with_mock(&ctx, false);
        ctx.with(|ctx| {
            let json: String = ctx.eval("__lumen_enumerate_audio_devices()").unwrap();
            assert!(json.starts_with('['), "expected JSON array, got: {json}");
            assert!(json.contains("mock-0"));
        });
    }

    #[test]
    fn start_capture_returns_positive_id() {
        let (_rt, ctx) = make_ctx();
        install_with_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_start_audio_capture('', 0, 0)").unwrap();
            assert!(id >= 1.0, "expected positive handle id, got {id}");
        });
    }

    #[test]
    fn start_capture_fails_when_no_provider_or_denied() {
        let (_rt, ctx) = make_ctx();
        install_with_mock(&ctx, true);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_start_audio_capture('', 0, 0)").unwrap();
            assert_eq!(id, -1.0, "expected -1 on capture failure");
        });
    }

    #[test]
    fn capture_info_returns_json() {
        let (_rt, ctx) = make_ctx();
        install_with_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_start_audio_capture('', 0, 0)").unwrap();
            assert!(id >= 1.0);
            let code = format!("__lumen_audio_capture_info({id})");
            let info: String = ctx.eval(code.as_str()).unwrap();
            assert!(info.contains("44100"), "expected sample_rate in info: {info}");
            assert!(info.contains("mock-0"), "expected device_id in info: {info}");
        });
    }

    #[test]
    fn read_pcm_returns_json_array() {
        let (_rt, ctx) = make_ctx();
        install_with_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_start_audio_capture('', 0, 0)").unwrap();
            let code = format!("__lumen_read_audio_pcm({id}, 4096)");
            let json: String = ctx.eval(code.as_str()).unwrap();
            assert!(json.starts_with('['), "expected JSON array: {json}");
            // MockHandle has 3 pending samples
            let arr: Vec<f32> = serde_json::from_str(&json).unwrap_or_default();
            assert_eq!(arr.len(), 3, "expected 3 samples, got {}", arr.len());
        });
    }

    #[test]
    fn stop_removes_handle() {
        let (_rt, ctx) = make_ctx();
        install_with_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_start_audio_capture('', 0, 0)").unwrap();
            let stop_code = format!("__lumen_stop_audio_capture({id})");
            ctx.eval::<(), _>(stop_code.as_str()).unwrap();
            // After stop, info should return empty object
            let info_code = format!("__lumen_audio_capture_info({id})");
            let info: String = ctx.eval(info_code.as_str()).unwrap();
            assert_eq!(info, "{}", "expected empty info after stop: {info}");
        });
    }

    #[test]
    fn read_pcm_max_samples_respected() {
        let (_rt, ctx) = make_ctx();
        install_with_mock(&ctx, false);
        ctx.with(|ctx| {
            let id: f64 = ctx.eval("__lumen_start_audio_capture('', 0, 0)").unwrap();
            // Mock has 3 samples; request only 2
            let code = format!("__lumen_read_audio_pcm({id}, 2)");
            let json: String = ctx.eval(code.as_str()).unwrap();
            let arr: Vec<f32> = serde_json::from_str(&json).unwrap_or_default();
            assert_eq!(arr.len(), 2, "max_samples=2 should cap at 2");
        });
    }

    // NullAudioCaptureProvider → enumerate returns [] is tested in lumen-core::audio_capture_tests.
    // Omitted here to avoid a global-state race with parallel test threads.
}
