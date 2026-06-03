//! GPU-бэкенды для [`crate::RenderBackend`] (ADR-010).
//!
//! Каждый бэкенд — отдельный модуль, изолированный от соседей:
//! изменения в `vello_backend` не затрагивают `wgpu_backend`.
//!
//! | Модуль | Feature | Статус |
//! |---|---|---|
//! | [`wgpu_backend`] | `backend-wgpu` | Phase 1 (текущий) |
//! | [`femtovg_backend`] | `backend-femtovg` | Phase 2 (RB-5 скелет, RB-6 полный) |
//! | [`vello_backend`] | `backend-vello` | Phase 3 (RB-7 заглушка, RB-10 полный) |
//! | `compare_backend` | `compare` | тестовый (RB-8) |

#[cfg(feature = "backend-wgpu")]
pub mod wgpu_backend;

#[cfg(feature = "backend-femtovg")]
pub mod femtovg_backend;

#[cfg(feature = "backend-vello")]
pub mod vello_backend;

#[cfg(feature = "backend-wgpu")]
pub use wgpu_backend::WgpuBackend;

#[cfg(feature = "backend-femtovg")]
pub use femtovg_backend::FemtovgBackend;

#[cfg(feature = "backend-vello")]
pub use vello_backend::VelloBackend;
