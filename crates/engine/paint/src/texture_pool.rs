//! GPU texture pool with recycling support for layer rendering.
//!
//! Phase 2 ADR-008: T0 memory optimization through texture reuse.
//! Instead of allocating a new `wgpu::Texture` for each layer, this pool
//! maintains a free list of textures keyed by (width, height, format).
//! When a layer texture is no longer needed, it returns to the pool.
//!
//! Pool is size-aware: fetching a texture of size W×H returns a texture
//! of that exact size if available in the free list, or allocates a new one.

use std::collections::HashMap;

/// Key for a pool entry: texture dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureKey {
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
}

impl TextureKey {
    /// Create a new texture pool key.
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// A pooled GPU texture resource.
/// Wraps `wgpu::Texture` and metadata for reuse management.
#[derive(Debug)]
pub struct PooledTexture {
    /// GPU texture object.
    pub texture: wgpu::Texture,
    /// Texture view for rendering operations.
    pub view: wgpu::TextureView,
    /// Bind group for composite operations.
    pub bind_group: wgpu::BindGroup,
    /// Actual texture dimensions.
    pub width: u32,
    pub height: u32,
}

/// Texture pool managing free textures for recycling.
///
/// Textures are pooled by size. When a texture of size W×H is requested,
/// the pool returns a free texture of that size if available,
/// otherwise returns `None` for the caller to allocate a new one.
/// When a texture is no longer needed, it can be returned to the pool via `release()`.
#[derive(Debug)]
pub struct TexturePool {
    /// Free textures grouped by size: (width, height) -> Vec<PooledTexture>
    free_pool: HashMap<TextureKey, Vec<PooledTexture>>,
    /// Total number of textures currently in the pool (free + in-use).
    /// Used for diagnostics.
    pool_size: usize,
}

impl TexturePool {
    /// Create a new empty texture pool.
    pub fn new() -> Self {
        Self {
            free_pool: HashMap::new(),
            pool_size: 0,
        }
    }

    /// Try to allocate a texture of the given size from the pool.
    /// Returns `Some(PooledTexture)` if a free texture of this size exists,
    /// or `None` if the pool is empty for this size (caller should allocate new).
    pub fn acquire(&mut self, width: u32, height: u32) -> Option<PooledTexture> {
        let key = TextureKey::new(width, height);
        self.free_pool
            .get_mut(&key)
            .and_then(|free_textures| free_textures.pop())
    }

    /// Return a texture to the pool for reuse.
    /// The texture can later be acquired via `acquire()` if its size matches a request.
    pub fn release(&mut self, texture: PooledTexture) {
        let key = TextureKey::new(texture.width, texture.height);
        self.free_pool.entry(key).or_default().push(texture);
    }

    /// Clear all pooled textures, freeing GPU memory.
    pub fn clear(&mut self) {
        self.free_pool.clear();
        self.pool_size = 0;
    }

    /// Get the number of free textures in the pool (across all sizes).
    pub fn len(&self) -> usize {
        self.free_pool.values().map(|v| v.len()).sum()
    }

    /// Check if the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.free_pool.values().all(|v| v.is_empty())
    }

    /// Get the number of free textures of a specific size.
    pub fn len_for_size(&self, width: u32, height: u32) -> usize {
        let key = TextureKey::new(width, height);
        self.free_pool.get(&key).map(|v| v.len()).unwrap_or(0)
    }

    /// Get total tracked pool size (for diagnostics).
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }

    /// Update internal pool size counter (call after creating or destroying a texture).
    pub fn update_size(&mut self, delta: i32) {
        if delta > 0 {
            self.pool_size += delta as usize;
        } else if delta < 0 {
            self.pool_size = self.pool_size.saturating_sub((-delta) as usize);
        }
    }
}

impl Default for TexturePool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn key(w: u32, h: u32) -> TextureKey {
        TextureKey::new(w, h)
    }

    #[test]
    fn pool_creation() {
        let pool = TexturePool::new();
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn pool_acquire_empty() {
        let mut pool = TexturePool::new();
        assert!(pool.acquire(512, 512).is_none());
    }

    #[test]
    fn pool_len_for_size() {
        let pool = TexturePool::new();
        assert_eq!(pool.len_for_size(256, 256), 0);
    }

    #[test]
    fn clear_pool() {
        let mut pool = TexturePool::new();
        pool.update_size(5);
        assert_eq!(pool.pool_size(), 5);
        pool.clear();
        assert_eq!(pool.pool_size(), 0);
        assert!(pool.is_empty());
    }

    #[test]
    fn update_size_tracking() {
        let mut pool = TexturePool::new();
        pool.update_size(3);
        assert_eq!(pool.pool_size(), 3);
        pool.update_size(-1);
        assert_eq!(pool.pool_size(), 2);
        pool.update_size(-5); // Underflow protection
        assert_eq!(pool.pool_size(), 0);
    }
}
