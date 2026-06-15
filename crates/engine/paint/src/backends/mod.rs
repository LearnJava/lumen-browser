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
//! | [`cpu_backend`] | `backend-cpu` | headless CI / тестовый (RB-8) |
//! | [`compare_backend`] | `compare` | pixel-diff тестовый (RB-8) |

#[cfg(feature = "backend-wgpu")]
pub mod wgpu_backend;

#[cfg(feature = "backend-femtovg")]
pub mod femtovg_backend;

#[cfg(feature = "backend-vello")]
pub mod vello_backend;

#[cfg(feature = "backend-cpu")]
pub mod cpu_backend;

#[cfg(feature = "compare")]
pub mod compare_backend;

#[cfg(feature = "backend-wgpu")]
pub use wgpu_backend::WgpuBackend;

#[cfg(feature = "backend-femtovg")]
pub use femtovg_backend::FemtovgBackend;

#[cfg(feature = "backend-vello")]
pub use vello_backend::VelloBackend;

#[cfg(feature = "backend-cpu")]
pub use cpu_backend::CpuBackend;

#[cfg(feature = "compare")]
pub use compare_backend::CompareBackend;
