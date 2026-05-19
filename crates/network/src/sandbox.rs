//! Re-export `SandboxFlags` и `parse_sandbox_value` из `lumen-core`.
//!
//! Тип живёт в core, чтобы и DOM, и network могли использовать его
//! без нарушения графа зависимостей.
pub use lumen_core::sandbox::{parse_sandbox_value, SandboxFlags};
