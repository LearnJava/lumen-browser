//! GPU texture pool with recycling support for layer rendering.
//!
//! Phase 2 ADR-008: T0 memory optimization through texture reuse.
//! Instead of allocating a new `wgpu::Texture` for each layer, this pool
//! maintains a free list of textures keyed by (width, height, format).
//! When a layer texture is no longer needed, it returns to the pool.
//!
//! Pool is size-aware: fetching a texture of size W×H returns a texture
//! of that exact size if available in the free list, or allocates a new one.
//!
//! **BUG-272 срез 21 — байтовый бюджет свободного списка.** До среза пул рос
//! неограниченно: `release` всегда клал текстуру в список своего класса
//! размеров, а освобождался список только на `clear` (ресайз окна / явная
//! эвикция). Пока классов немного, это ровно тот выигрыш, ради которого пул и
//! заводился, но bbox-офскрины backdrop-фильтра выравниваются вверх до 64 px и
//! дают десятки разных классов, а прокрутка порождает новые: класс,
//! востребованный один раз, держал свою текстуру (полноэкранная — ~4.6 МБ при
//! 1280×900) до конца жизни окна. Теперь свободный список ограничен байтовым
//! бюджетом, и при превышении вытесняются классы, к которым дольше всего не
//! обращались (LRU по классам — тот же приём, что у femtovg-`resized_variants`
//! в срезе 18 и у `layer_pool` с капом 8 в срезе 1).

use std::collections::HashMap;

/// Байт на пиксель у offscreen-слоя: формат поверхности — 8-битный RGBA/BGRA.
/// Драйвер может выравнивать строки и тайлить, поэтому это оценка снизу; для
/// бюджета важен порядок величины, а не байт-в-байт совпадение с драйвером.
const BYTES_PER_PIXEL: u64 = 4;

/// Бюджет свободного списка по умолчанию в мегабайтах.
///
/// Полноэкранный слой при 1280×900 — ~4.6 МБ, а глубина вложенности офскринов
/// на реальных страницах измеряется единицами, поэтому 64 МБ с запасом
/// покрывают рабочий набор повторно используемых классов и при этом отсекают
/// бесконечное накопление одноразовых bbox-классов.
const DEFAULT_BUDGET_MB: u64 = 64;

/// Объём текстуры W×H в байтах.
fn texture_bytes(width: u32, height: u32) -> u64 {
    u64::from(width) * u64::from(height) * BYTES_PER_PIXEL
}

/// Бюджет свободного списка по умолчанию в байтах.
///
/// Переопределяется переменной окружения `LUMEN_TEXTURE_POOL_MB`; значение `0`
/// снимает ограничение и возвращает поведение до BUG-272 среза 21 — это
/// kill-switch для A/B-замера памяти на одном бинарнике.
fn default_budget_bytes() -> u64 {
    use std::sync::OnceLock;
    static BUDGET: OnceLock<u64> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let mb = std::env::var("LUMEN_TEXTURE_POOL_MB")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_BUDGET_MB);
        mb * 1024 * 1024
    })
}

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

/// Размер хранимой в пуле текстуры — всё, что пулу нужно знать о полезной
/// нагрузке. Вынесен в трейт, чтобы учёт байтов и LRU-вытеснение проверялись
/// юнит-тестами без живого GPU-устройства (`PooledTexture` не сконструировать
/// без `wgpu::Device`).
pub trait PoolTexture {
    /// Ширина текстуры в физических пикселях.
    fn width(&self) -> u32;
    /// Высота текстуры в физических пикселях.
    fn height(&self) -> u32;
}

/// A pooled GPU texture resource.
/// Wraps `wgpu::Texture` and metadata for reuse management.
/// Only available with the `backend-wgpu` feature (ADR-010).
#[cfg(feature = "backend-wgpu")]
#[derive(Debug)]
pub struct PooledTexture {
    /// GPU texture object.
    pub texture: wgpu::Texture,
    /// Texture view for rendering operations.
    pub view: wgpu::TextureView,
    /// Bind group for composite operations.
    pub bind_group: wgpu::BindGroup,
    /// Actual texture width in physical pixels.
    pub width: u32,
    /// Actual texture height in physical pixels.
    pub height: u32,
}

#[cfg(feature = "backend-wgpu")]
impl PoolTexture for PooledTexture {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }
}

/// Класс размеров: свободные текстуры одного размера плюс отметка последнего
/// обращения (значение монотонного тика пула) — источник LRU-порядка.
#[derive(Debug)]
struct SizeClass<T> {
    /// Свободные текстуры этого размера.
    free: Vec<T>,
    /// Тик последнего `acquire`/`release` по этому классу.
    last_tick: u64,
}

/// Texture pool managing free textures for recycling.
///
/// Textures are pooled by size. When a texture of size W×H is requested,
/// the pool returns a free texture of that size if available,
/// otherwise returns `None` for the caller to allocate a new one.
/// When a texture is no longer needed, it can be returned to the pool via `release()`.
///
/// Свободный список ограничен байтовым бюджетом (BUG-272 срез 21); вытеснение
/// выполняет [`TexturePool::trim`], которую вызывают в безопасной точке кадра —
/// после `queue.submit`, когда wgpu уже удерживает отправленные ресурсы сам.
#[derive(Debug)]
pub struct TexturePool<T> {
    /// Free textures grouped by size: (width, height) -> размерный класс.
    free_pool: HashMap<TextureKey, SizeClass<T>>,
    /// Total number of textures currently in the pool (free + in-use).
    /// Used for diagnostics.
    pool_size: usize,
    /// Суммарный объём свободного списка в байтах.
    free_bytes: u64,
    /// Бюджет свободного списка в байтах; `0` — без ограничения.
    budget_bytes: u64,
    /// Монотонный счётчик обращений — источник LRU-порядка классов.
    tick: u64,
    /// Сколько текстур вытеснено бюджетом за время жизни пула (диагностика).
    /// Намеренно НЕ сбрасывается в [`TexturePool::clear`].
    evicted: usize,
}

impl<T: PoolTexture> TexturePool<T> {
    /// Create a new empty texture pool with the default byte budget
    /// (см. `DEFAULT_BUDGET_MB` и `LUMEN_TEXTURE_POOL_MB`).
    pub fn new() -> Self {
        Self::with_budget_bytes(default_budget_bytes())
    }

    /// Пул с явным бюджетом свободного списка в байтах; `0` — без ограничения.
    pub fn with_budget_bytes(budget_bytes: u64) -> Self {
        Self {
            free_pool: HashMap::new(),
            pool_size: 0,
            free_bytes: 0,
            budget_bytes,
            tick: 0,
            evicted: 0,
        }
    }

    /// Try to allocate a texture of the given size from the pool.
    /// Returns `Some(texture)` if a free texture of this size exists,
    /// or `None` if the pool is empty for this size (caller should allocate new).
    pub fn acquire(&mut self, width: u32, height: u32) -> Option<T> {
        self.tick += 1;
        let tick = self.tick;
        let class = self.free_pool.get_mut(&TextureKey::new(width, height))?;
        class.last_tick = tick;
        let texture = class.free.pop()?;
        self.free_bytes = self.free_bytes.saturating_sub(texture_bytes(width, height));
        Some(texture)
    }

    /// Return a texture to the pool for reuse.
    /// The texture can later be acquired via `acquire()` if its size matches a request.
    ///
    /// Бюджет здесь НЕ применяется: `release` вызывают по ходу записи команд
    /// кадра, а вытеснение (drop текстуры) отложено до [`TexturePool::trim`]
    /// после сабмита.
    pub fn release(&mut self, texture: T) {
        self.tick += 1;
        let tick = self.tick;
        let key = TextureKey::new(texture.width(), texture.height());
        let bytes = texture_bytes(key.width, key.height);
        let class = self
            .free_pool
            .entry(key)
            .or_insert_with(|| SizeClass { free: Vec::new(), last_tick: tick });
        class.last_tick = tick;
        class.free.push(texture);
        self.free_bytes += bytes;
    }

    /// Привести свободный список к бюджету, вытесняя текстуры из классов, к
    /// которым дольше всего не обращались. Возвращает число вытесненных текстур.
    ///
    /// Вызывается один раз за кадр после `queue.submit`: к этому моменту wgpu
    /// удерживает ресурсы, использованные отправленными командами, сам, поэтому
    /// drop свободной текстуры безопасен.
    pub fn trim(&mut self) -> usize {
        if self.budget_bytes == 0 {
            return 0;
        }
        let mut evicted = 0;
        while self.free_bytes > self.budget_bytes {
            let Some(key) = self
                .free_pool
                .iter()
                .filter(|(_, class)| !class.free.is_empty())
                .min_by_key(|(_, class)| class.last_tick)
                .map(|(key, _)| *key)
            else {
                break;
            };
            let Some(class) = self.free_pool.get_mut(&key) else {
                break;
            };
            if class.free.pop().is_none() {
                break;
            }
            if class.free.is_empty() {
                self.free_pool.remove(&key);
            }
            self.free_bytes = self
                .free_bytes
                .saturating_sub(texture_bytes(key.width, key.height));
            self.pool_size = self.pool_size.saturating_sub(1);
            evicted += 1;
        }
        self.evicted += evicted;
        evicted
    }

    /// Clear all pooled textures, freeing GPU memory.
    pub fn clear(&mut self) {
        self.free_pool.clear();
        self.pool_size = 0;
        self.free_bytes = 0;
    }

    /// Get the number of free textures in the pool (across all sizes).
    pub fn len(&self) -> usize {
        self.free_pool.values().map(|class| class.free.len()).sum()
    }

    /// Check if the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.free_pool.values().all(|class| class.free.is_empty())
    }

    /// Get the number of free textures of a specific size.
    pub fn len_for_size(&self, width: u32, height: u32) -> usize {
        self.free_pool
            .get(&TextureKey::new(width, height))
            .map_or(0, |class| class.free.len())
    }

    /// Число различных классов размеров в свободном списке (диагностика).
    pub fn size_classes(&self) -> usize {
        self.free_pool.len()
    }

    /// Суммарный объём свободного списка в байтах (диагностика).
    pub fn free_bytes(&self) -> u64 {
        self.free_bytes
    }

    /// Бюджет свободного списка в байтах; `0` — без ограничения (диагностика).
    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// Сколько текстур вытеснено бюджетом за время жизни пула (диагностика).
    pub fn evicted(&self) -> usize {
        self.evicted
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

impl<T: PoolTexture> Default for TexturePool<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Полезная нагрузка для тестов: пулу от текстуры нужны только размеры,
    /// поэтому логика бюджета и LRU проверяется без живого GPU-устройства.
    #[derive(Debug, PartialEq, Eq)]
    struct FakeTexture {
        /// Ширина в физических пикселях.
        width: u32,
        /// Высота в физических пикселях.
        height: u32,
        /// Метка экземпляра — чтобы отличать текстуры одного размера.
        tag: u32,
    }

    impl PoolTexture for FakeTexture {
        fn width(&self) -> u32 {
            self.width
        }

        fn height(&self) -> u32 {
            self.height
        }
    }

    fn fake(width: u32, height: u32, tag: u32) -> FakeTexture {
        FakeTexture { width, height, tag }
    }

    /// Пул без бюджета: поведение до среза 21 (`LUMEN_TEXTURE_POOL_MB=0`).
    fn unbounded() -> TexturePool<FakeTexture> {
        TexturePool::with_budget_bytes(0)
    }

    #[test]
    fn pool_creation() {
        let pool = unbounded();
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn pool_acquire_empty() {
        let mut pool = unbounded();
        assert!(pool.acquire(512, 512).is_none());
    }

    #[test]
    fn pool_len_for_size() {
        let pool = unbounded();
        assert_eq!(pool.len_for_size(256, 256), 0);
    }

    #[test]
    fn clear_pool() {
        let mut pool = unbounded();
        pool.update_size(5);
        assert_eq!(pool.pool_size(), 5);
        pool.clear();
        assert_eq!(pool.pool_size(), 0);
        assert!(pool.is_empty());
    }

    #[test]
    fn update_size_tracking() {
        let mut pool = unbounded();
        pool.update_size(3);
        assert_eq!(pool.pool_size(), 3);
        pool.update_size(-1);
        assert_eq!(pool.pool_size(), 2);
        pool.update_size(-5); // Underflow protection
        assert_eq!(pool.pool_size(), 0);
    }

    #[test]
    fn release_then_acquire_returns_same_texture() {
        let mut pool = unbounded();
        pool.release(fake(64, 64, 7));
        assert_eq!(pool.free_bytes(), 64 * 64 * 4);
        let got = pool.acquire(64, 64).expect("released texture is reusable");
        assert_eq!(got.tag, 7);
        assert_eq!(pool.free_bytes(), 0);
    }

    /// Без бюджета свободный список растёт неограниченно — поведение до среза 21.
    #[test]
    fn unbounded_pool_never_evicts() {
        let mut pool = unbounded();
        for i in 0..64 {
            pool.release(fake(1024, 1024, i));
        }
        assert_eq!(pool.trim(), 0);
        assert_eq!(pool.len(), 64);
        assert_eq!(pool.evicted(), 0);
    }

    /// Бюджет вытесняет ровно столько, чтобы уложиться в лимит.
    #[test]
    fn trim_evicts_down_to_budget() {
        // Текстура 512×512 = 1 МиБ; бюджет — 4 МиБ.
        let mut pool = TexturePool::with_budget_bytes(4 * 1024 * 1024);
        pool.update_size(10);
        for i in 0..10 {
            pool.release(fake(512, 512, i));
        }
        assert_eq!(pool.len(), 10);
        assert_eq!(pool.trim(), 6);
        assert_eq!(pool.len(), 4);
        assert_eq!(pool.free_bytes(), 4 * 1024 * 1024);
        assert_eq!(pool.evicted(), 6);
        // Вытеснение уменьшает и учёт живых текстур.
        assert_eq!(pool.pool_size(), 4);
    }

    /// Вытесняется класс, к которому дольше всего не обращались, а не просто
    /// первый попавшийся: обращение через `acquire` освежает класс.
    #[test]
    fn trim_evicts_least_recently_used_class() {
        // Каждая текстура здесь 1 МиБ; бюджет — 2 МиБ.
        let mut pool = TexturePool::with_budget_bytes(2 * 1024 * 1024);
        pool.release(fake(512, 512, 1)); // класс A
        pool.release(fake(256, 1024, 2)); // класс B
        pool.release(fake(1024, 256, 3)); // класс C
        // Освежаем класс A обращением: теперь самый старый — B.
        let a = pool.acquire(512, 512).expect("class A has a free texture");
        pool.release(a);
        assert_eq!(pool.trim(), 1);
        assert_eq!(pool.len_for_size(256, 1024), 0, "LRU-класс B вытеснен");
        assert_eq!(pool.len_for_size(512, 512), 1, "освежённый класс A цел");
        assert_eq!(pool.len_for_size(1024, 256), 1, "класс C цел");
    }

    /// Опустошённые классы удаляются из карты: иначе `size_classes()` рос бы
    /// вечно даже после вытеснения всех текстур класса.
    #[test]
    fn trim_drops_empty_size_classes() {
        let mut pool = TexturePool::with_budget_bytes(1024 * 1024);
        pool.release(fake(512, 512, 1));
        pool.release(fake(256, 1024, 2));
        assert_eq!(pool.size_classes(), 2);
        pool.trim();
        assert_eq!(pool.size_classes(), 1);
        assert_eq!(pool.free_bytes(), 1024 * 1024);
    }

    /// `clear` обнуляет учёт байтов — иначе следующий `trim` вытеснял бы
    /// текстуры, которых в свободном списке уже нет.
    #[test]
    fn clear_resets_byte_accounting() {
        let mut pool = TexturePool::with_budget_bytes(1024 * 1024);
        pool.release(fake(1024, 1024, 1));
        assert!(pool.free_bytes() > 0);
        pool.clear();
        assert_eq!(pool.free_bytes(), 0);
        assert_eq!(pool.size_classes(), 0);
        assert_eq!(pool.trim(), 0);
    }
}
