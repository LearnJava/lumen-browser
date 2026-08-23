// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

pub mod audio_element;
pub mod background_fetch;
pub mod background_sync;
pub mod badging;
pub mod periodic_sync;
pub mod battery_bindings;
pub mod css_properties_values_api;
pub mod esm;
pub mod import_attributes;
pub mod import_meta;
pub mod paint_worklet;
pub mod gamepad;
pub mod highlight_api;
pub mod iframe_element;
pub mod broadcast_channel;
pub mod canvas2d;
pub mod close_watcher;
pub mod download_bindings;
pub mod network_log_bindings;
pub mod pip_bindings;
pub mod clipboard;
pub mod contacts;
pub mod cookie_banner;
pub mod cookie_store;
pub mod storage_buckets;
pub mod payment_request;
pub mod credentials;
pub mod device_sensors;
pub mod document_pip;
pub mod documentpip_bindings;
pub mod eye_dropper;
pub mod dom;
pub mod filesystem_access;
pub mod geolocation;
pub mod heap_snapshot;
pub mod intl_bindings;
pub mod media_capture;
pub mod media_devices;
pub mod media_session;
pub mod screen_capture;
pub mod navigator_bindings;
pub mod notifications_bindings;
pub mod offscreen_canvas;
pub mod pointer_lock;
pub mod push_api;
pub mod shape_detection;
pub mod shared_worker;
pub mod speech;
pub mod surface_api;
pub mod img_bitmap_store;
pub mod text_track_store;
pub mod video_bindings;
pub mod video_gif_store;
pub mod view_transitions;
pub mod bluetooth;
pub mod subtle_crypto;
pub mod temporal_api;
pub mod webgl_canvas;
pub mod webrtc_stub;
pub mod webhid;
pub mod webusb;
pub mod worker;
pub mod url_pattern;
pub mod navigation_api;
pub mod typed_om_api;
pub mod trusted_types;
pub mod sanitizer;
pub mod screen_orientation;
pub mod scroll_snap_events;
pub mod scroll_timeline;
pub mod sri;
pub mod media_stream_recording;
pub mod serial;
pub mod compute_pressure;
pub mod csp;
pub mod permissions;
pub mod permissions_policy;
pub mod web_codecs;
pub mod ua_client_hints;
pub mod media_capabilities;
pub mod virtual_keyboard;
pub mod wake_lock;
pub mod web_locks;
pub mod scheduler;
pub mod reporting_api;
pub mod web_audio;
pub mod webgpu;
pub mod webxr;
pub mod form_validation;
pub mod element_internals;
pub mod presentation_api;
pub mod webassembly;
pub mod wasm;
pub mod generic_sensor;
pub mod video_pip;
pub mod web_midi;
pub mod storage_manager;
pub mod xhr;
pub mod dom_parser;
pub mod gc_policy;
pub mod svg;
pub mod file_input;
/// BUG-378: sealing pass that hides the engine's internal `_lumen_*` global
/// names from enumeration and freezes the function-valued ones.
#[cfg(feature = "v8-backend")]
mod internal_globals;
pub mod tc39_proposals;
pub mod es2026_proposals;
pub mod async_context;
pub mod decorators;
pub mod speculation_rules;
pub mod soft_navigation;
pub mod content_index;
pub mod digital_credentials;
pub mod window_management;
pub mod local_font_access;
pub mod long_animation_frames;
pub mod launch_handler;
pub mod inert;
pub mod shared_storage;
pub mod idle_detection;
pub mod topics_api;
pub mod attribution_reporting;
pub mod frame_bridge;
pub mod pointer_capture;
pub mod sw_worker;

/// V8 compat layer (slice S2): `IntoV8NativeFn`, `register_v8_native`, and
/// helpers for registering typed Rust closures as V8 globals.
#[cfg(feature = "v8-backend")]
pub(crate) mod v8_compat;

/// V8-based JS runtime (slices S1–S2: runtime skeleton + compat layer).
///
/// Compiled only when the `v8-backend` feature is enabled. `V8JsRuntime` is
/// the crate's `JsRuntime` implementation and exposes `ensure_v8_platform` so
/// all code in this crate shares one V8 init path.
#[cfg(feature = "v8-backend")]
pub mod v8_runtime;

/// V8 ES-module loader (slice S12b-23): `<script type=module>`, import maps,
/// import attributes and dynamic `import()` on the V8 backend.
#[cfg(feature = "v8-backend")]
pub(crate) mod v8_esm;

pub use clipboard::set_clipboard_provider;
pub use credentials::set_credential_provider;
pub use media_capture::set_audio_capture_provider;
pub use screen_capture::set_screen_capture_provider;
pub use audio_element::set_audio_playback_provider;
pub use wake_lock::set_wake_lock_provider;
pub use video_gif_store::{set_video_gif_store, VideoGifStore};
pub use text_track_store::{set_text_track_store, CueData, TextTrackData, TextTrackStore};
pub use css_properties_values_api::{RegisteredProperty, RegisteredPropertiesMap, get_registered_properties};
pub use paint_worklet::{PaintWorkletDef, PaintWorkletRegistry, get_paint_worklet_registry};
pub use dom::{FullscreenRequest, HistoryUrlUpdate, NavigateRequest, PrintRequest};
/// BUG-341 S7: page-side DOM-mutation tracker outcome, feeding
/// `lumen_layout::style::restyle_root_set_for_node_change`. Only compiled
/// under `v8-backend` — see `v8_runtime::DomTouched`'s doc comment.
#[cfg(feature = "v8-backend")]
pub use v8_runtime::DomTouched;
pub use view_transitions::ViewTransitionEvent;
pub use navigator_bindings::{NavigatorProfile, set_navigator_profile};
pub use surface_api::{global_privacy_control_enabled, set_global_privacy_control};
pub use lumen_core::WebStorage;

/// Compute a deterministic u64 seed from a URL for deterministic render mode (8F).
///
/// Uses the URL fragment (`#...`) if present; otherwise the full URL string.
/// FNV-1a 64-bit hash guarantees the same seed across platforms and Rust versions.
/// Result is guaranteed non-zero (xorshift32 must not start at 0).
pub fn deterministic_seed_from_url(url: &str) -> u64 {
    let src = if let Some(pos) = url.rfind('#') { &url[pos + 1..] } else { url };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in src.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if h == 0 { 1 } else { h }
}

/// Build a JSON array of `{ id, json }` objects from the drained worker message list.
///
/// Each element is `{"id":<worker_id>,"json":<raw_json_value>}` so that
/// `_lumen_deliver_worker_messages` can parse the payload without double-JSON-encoding.
///
/// Only called from `v8_runtime`'s pump methods and `shared_worker`'s V8 test
/// harness — unused (and unbuilt) without the `v8-backend` feature.
#[cfg(feature = "v8-backend")]
fn build_worker_messages_json(messages: &[(u32, String)]) -> String {
    let items: Vec<String> = messages
        .iter()
        .map(|(id, json)| format!("{{\"id\":{id},\"json\":{json}}}"))
        .collect();
    format!("[{}]", items.join(","))
}

