//! Aggregated lumen-driver integration test suite.
//!
//! All integration tests live here as submodules so they link into ONE
//! test binary (`tests/all.rs`) instead of ~64 separate binaries. Each
//! separate binary statically links the full engine (wgpu/winit/rustls),
//! so collapsing them turns ~64 link steps into 1 — see BT-1.
//!
//! Feature-gated modules (`snapshot_cpu`, `compare_backends`) carry an inner
//! `#![cfg(feature = ...)]` that empties them when the feature is off.
#![allow(dead_code)]

mod antidetect_surface_api;
mod compare_backends;
mod isolation;
mod mock_transport;
mod snapshot_cpu;
mod snapshot_generator;
mod snapshot_vs_edge;
mod test_00_calibration;
mod test_01_sanity;
mod test_02_color_named;
mod test_03_color_formats;
mod test_04_color_alpha;
mod test_05_border_width;
mod test_06;
mod test_07;
mod test_08_padding;
mod test_09;
mod test_10;
mod test_11;
mod test_12;
mod test_13_visibility_opacity;
mod test_14;
mod test_15;
mod test_16;
mod test_17;
mod test_18_images;
mod test_19;
mod test_20;
mod test_21;
mod test_22_transform;
mod test_23;
mod test_24;
mod test_25;
mod test_26_mask_image;
mod test_27;
mod test_28;
mod test_29;
mod test_30;
mod test_31;
mod test_32;
mod test_33;
mod test_34;
mod test_35;
mod test_36_border_radius;
mod test_37;
mod test_38;
mod test_39_gradients;
mod test_40;
mod test_41;
mod test_42;
mod test_43;
mod test_44;
mod test_45;
mod test_46_individual_transforms;
mod test_47;
mod test_48;
mod test_49;
mod test_a11y_tree;
mod test_compositor;
mod test_gpu_session;
mod test_pages_integrity;
mod test_stacking_order;
mod test_structural_getters;
