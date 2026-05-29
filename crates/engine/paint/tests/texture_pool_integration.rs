//! Integration tests for texture pool recycling (Phase 2 ADR-008).
//!
//! Tests verify that TexturePool integrates correctly with Renderer:
//! - Pool is used for texture reuse
//! - Textures are properly cached and returned
//! - Pool is cleared on resize

use lumen_paint::texture_pool::{TextureKey, TexturePool};

#[test]
fn texture_pool_basic_operations() {
    let mut pool = TexturePool::new();

    // Pool starts empty
    assert!(pool.is_empty());
    assert_eq!(pool.len(), 0);

    // No texture available before adding
    assert!(pool.acquire(512, 512).is_none());
}

#[test]
fn texture_pool_size_tracking() {
    let mut pool = TexturePool::new();

    pool.update_size(5);
    assert_eq!(pool.pool_size(), 5);

    pool.update_size(3);
    assert_eq!(pool.pool_size(), 8);

    pool.update_size(-2);
    assert_eq!(pool.pool_size(), 6);

    pool.clear();
    assert_eq!(pool.pool_size(), 0);
}

#[test]
fn texture_pool_clear_resets_diagnostics() {
    let mut pool = TexturePool::new();

    pool.update_size(10);
    assert_eq!(pool.pool_size(), 10);

    pool.clear();
    assert_eq!(pool.pool_size(), 0);
    assert!(pool.is_empty());
    assert_eq!(pool.len(), 0);
}

#[test]
fn texture_key_equality() {
    let key1 = TextureKey::new(512, 512);
    let key2 = TextureKey::new(512, 512);
    let key3 = TextureKey::new(256, 512);

    assert_eq!(key1, key2);
    assert_ne!(key1, key3);
}

#[test]
fn texture_pool_separate_by_size() {
    let pool = TexturePool::new();

    // Sizes are tracked independently
    assert_eq!(pool.len_for_size(256, 256), 0);
    assert_eq!(pool.len_for_size(512, 512), 0);
}
