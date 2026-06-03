//! GPU-бэкенды для [`crate::RenderBackend`] (ADR-010).
//!
//! Каждый бэкенд — отдельный модуль, изолированный от соседей:
//! изменения в `vello_backend` не затрагивают `wgpu_backend`.
//!
//! | Модуль | Feature | Статус |
//! |---|---|---|
//! | [`wgpu_backend`] | всегда | Phase 1 (текущий) |

pub mod wgpu_backend;

pub use wgpu_backend::WgpuBackend;
