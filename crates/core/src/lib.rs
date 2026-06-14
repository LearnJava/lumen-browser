//! Lumen core: foundational types and traits.
//!
//! Этот крейт — самый нижний в графе зависимостей. Все остальные крейты
//! Lumen зависят от него; он не зависит ни от одного из них. Сюда кладём
//! только то, что нужно более чем одному модулю.

pub mod capability;
pub mod color;
pub mod crash;
pub mod error;
pub mod event;
pub mod ext;
pub mod form;
pub mod geom;
pub mod hash;
pub mod idn;
pub mod json;
pub mod memory_pressure;
pub mod module;
pub mod punycode;
pub mod sandbox;
pub mod sri;
pub mod url;
pub mod web_storage;

pub use capability::{Capability, CapabilityToken};
pub use color::{ColorSpace, detect_color_space_from_icc};
pub use crash::{format_crash_dump, write_crash_dump, CrashRecorder};
pub use error::{Error, Result};
pub use event::{Event, FetchPriority, RequestStage, SubresourceKind, TabId};
pub use ext::{
    match_face, BrowserSession, ClockMode, EventSink, FaceRecord, FontProvider, FontStyle,
    HyphenationProvider, NullBrowserSession, NullHyphenationProvider,
    JsError, JsResult, JsRuntime, JsValue, NoopEventSink, NullJsRuntime, SuspendedHeap,
    MemoryPressureLevel, MemoryPressureSource, NullMemoryPressureSource,
    CacheRegistry, EvictableCache,
    AiBackend, NullAiBackend,
};
pub use form::{
    decode_form_value, encode_form_multipart, encode_form_urlencoded, FormEntry, FormValue,
};
pub use json::{parse as parse_json, JsonError, JsonResult, JsonValue};
pub use geom::{Point, Rect, Size};
pub use module::Module;
pub use sandbox::{parse_sandbox_value, SandboxFlags};
pub use sri::{DigestProvider, IntegrityList, SriAlgorithm, SriError, SriHash, SriResult};
pub use url::Url;
pub use web_storage::WebStorage;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
