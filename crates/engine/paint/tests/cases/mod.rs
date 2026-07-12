//! Aggregated lumen-paint integration test suite.
//!
//! Every integration test lives here as a submodule so they link into ONE
//! test binary (`tests/all.rs`) instead of one binary per file. Each separate
//! binary statically links the full engine stack, so collapsing them turns N
//! link steps into 1 — same BT-1 pattern already applied to lumen-driver.
//!
//! Feature-gated modules keep their inner `#![cfg(feature = ...)]`, which
//! empties them when the feature is off (headless/texture-pool need backend-wgpu).
#![allow(dead_code)]

mod headless_tests;
mod snapshot_tests;
mod texture_pool_integration;
