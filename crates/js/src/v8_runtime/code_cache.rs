//! Процессный кэш скомпилированного байт-кода V8 (PERF-9) и макрос
//! `compile_cached!`, который через него компилирует.
//!
//! Вынесено из `v8_runtime.rs` батчем SPLIT-JS5. Макрос отдаётся наружу не
//! `#[macro_use]`, а `pub(super) use` по пути: тогда он виден независимо от
//! порядка объявлений `mod` в родителе, а не только ниже по тексту.

use super::*;

/// PERF-9 process-wide V8 bytecode cache: source-hash → `CachedData` bytes.
///
/// The engine's ~94 per-API module shims (`install_v8!` in `install_dom`,
/// `crate::*::install_*_v8`) are `&'static str` constants run through
/// `eval()` on **every navigation** — none of them takes a page parameter, so
/// the text is byte-identical every time (the same property that made them
/// PERF-9's originally-proposed snapshot batch, see
/// `docs/tasks/perf-startup-census.md` §6). Caching the compiled bytecode
/// captures most of that win without a snapshot's cost: no native call has to
/// become lazy, because the script still *executes* normally on both a hit
/// and a miss — only the parse phase is skipped on a hit.
///
/// `WEB_API_SHIM` itself is deliberately not routed through this cache: it
/// runs as a *string argument* to indirect `eval()` (BUG-378, see the comment
/// at the WEB_API_SHIM call site in `install_dom`), not as a `v8::Script`, so
/// there is no `UnboundScript` here to attach cached bytecode to — fixing
/// that would mean giving up the `configurable`/`enumerable:false` property
/// indirect eval buys, which is a BUG-378 regression, not a PERF-9 change.
pub(super) static CODE_CACHE: LazyLock<Mutex<HashMap<u64, Vec<u8>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Below this size, the parse cost this cache exists to skip is cheap enough
/// that hashing the source and touching the cache mutex is not worth it —
/// most `eval()` calls are small one-off scripts (`_lumen_focus_update(3)`,
/// deterministic-seed/UA/timezone overrides, …), not shims. Measured against
/// the actual `*_SHIM` constants (`docs/tasks/perf-startup-census.md` §6):
/// 512 bytes catches 96 of 98 module shims, missing only 637 of their 710 KB
/// combined — well above genuinely dynamic one-off content, which tops out
/// around a few dozen bytes.
pub(super) const CODE_CACHE_MIN_LEN: usize = 512;

/// Bounds cache growth if a caller ever routes large, non-repeating content
/// through `eval()`. Nothing in the engine does today — every script above
/// `CODE_CACHE_MIN_LEN` is one of the static module shims — but `eval()` is a
/// generic trait method, not shim-specific, so this is a defensive cap rather
/// than a tuned figure.
pub(super) const CODE_CACHE_MAX_ENTRIES: usize = 512;

pub(super) fn code_cache_hash(src: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut hasher);
    hasher.finish()
}

/// Compile `$src_str` (already interned as `$v8_src`) through `CODE_CACHE`.
/// Below `CODE_CACHE_MIN_LEN` this is exactly `v8::Script::compile`. At or
/// above it: consume a cached `UnboundScript` on a hit, or compile normally
/// and store the result's code cache for next time. Self-heals on a rejected
/// entry (recompiles and overwrites it) rather than trusting the cache to
/// always be valid — correctness never depends on a hit.
macro_rules! compile_cached {
    ($tc:expr, $src_str:expr, $v8_src:expr) => {{
        let __src_str: &str = $src_str;
        let __v8_src: v8::Local<v8::String> = $v8_src;
        if __src_str.len() < CODE_CACHE_MIN_LEN {
            v8::Script::compile($tc, __v8_src, None)
        } else {
            let __hash = code_cache_hash(__src_str);
            let __hit = CODE_CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&__hash)
                .cloned();
            let mut __compiled = None;
            if let Some(__bytes) = __hit {
                let mut __source = v8::script_compiler::Source::new_with_cached_data(
                    __v8_src,
                    None,
                    v8::CachedData::new(&__bytes),
                );
                if let Some(__unbound) = v8::script_compiler::compile_unbound_script(
                    $tc,
                    &mut __source,
                    v8::script_compiler::CompileOptions::ConsumeCodeCache,
                    v8::script_compiler::NoCacheReason::NoReason,
                ) {
                    let __rejected = __source
                        .get_cached_data()
                        .map(|d| d.rejected())
                        .unwrap_or(true);
                    if !__rejected {
                        __compiled = Some(__unbound.bind_to_current_context($tc));
                    }
                }
            }
            if __compiled.is_none() {
                let mut __source = v8::script_compiler::Source::new(__v8_src, None);
                if let Some(__unbound) = v8::script_compiler::compile_unbound_script(
                    $tc,
                    &mut __source,
                    v8::script_compiler::CompileOptions::NoCompileOptions,
                    v8::script_compiler::NoCacheReason::NoReason,
                ) {
                    if let Some(__cache) = __unbound.create_code_cache() {
                        let mut __map = CODE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                        if __map.len() < CODE_CACHE_MAX_ENTRIES || __map.contains_key(&__hash) {
                            __map.insert(__hash, __cache.to_vec());
                        }
                    }
                    __compiled = Some(__unbound.bind_to_current_context($tc));
                }
            }
            __compiled
        }
    }};
}

pub(super) use compile_cached;
