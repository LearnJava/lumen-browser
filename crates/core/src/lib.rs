//! Lumen core: foundational types and traits.
//!
//! Этот крейт — самый нижний в графе зависимостей. Все остальные крейты
//! Lumen зависят от него; он не зависит ни от одного из них. Сюда кладём
//! только то, что нужно более чем одному модулю.

pub mod capability;
pub mod error;
pub mod event;
pub mod ext;
pub mod geom;
pub mod idn;
pub mod module;
pub mod punycode;
pub mod sri;
pub mod url;

pub use capability::{Capability, CapabilityToken};
pub use error::{Error, Result};
pub use event::{Event, TabId};
pub use ext::{EventSink, JsError, JsResult, JsRuntime, JsValue, NoopEventSink, NullJsRuntime};
pub use geom::{Point, Rect, Size};
pub use module::Module;
pub use sri::{DigestProvider, IntegrityList, SriAlgorithm, SriError, SriHash, SriResult};
pub use url::Url;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
