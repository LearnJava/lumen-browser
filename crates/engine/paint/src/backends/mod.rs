//! GPU-бэкенды для [`crate::RenderBackend`] (ADR-010).
//!
//! Каждый бэкенд — отдельный модуль, изолированный от соседей:
//! изменения в `vello_backend` не затрагивают `wgpu_backend`.
//!
//! | Модуль | Feature | Статус |
//! |---|---|---|
//! | [`wgpu_backend`] | `backend-wgpu` | Phase 1 (текущий) |
//! | `femtovg_backend` | `backend-femtovg` | Phase 2 (RB-5) |
//! | `vello_backend` | `backend-vello` | Phase 3 (RB-10) |
//! | `compare_backend` | `compare` | тестовый (RB-8) |

pub mod wgpu_backend;

pub use wgpu_backend::WgpuBackend;
