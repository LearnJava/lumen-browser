//! Источник и резолв face-ов рендерера: `with_font_provider`/
//! `set_font_provider`, прелоад fallback-цепочек, мемо-резолв `face_id`
//! по `font-family` и параллельный префетч face-ов кадра. Вынесено из
//! `renderer.rs` без правок тел — файл перерос базовую линию размера
//! (docs/lint-policy.md §5.1); приватные `resolve_face_id` и
//! `prefetch_faces_parallel` стали `pub(super)`, их зовёт `render_impl`.

use super::*;

impl Renderer {
    /// Заменяет источник лукапа face-ов. Полезно для тестов (mock-provider) и
    /// headless-режимов (отключить системный скан). `None` отключает поиск —
    /// рендер всегда использует default face.
    #[must_use]
    pub fn with_font_provider(mut self, provider: Option<Arc<dyn FontProvider>>) -> Self {
        self.font_provider = provider;
        self
    }

    /// Заменяет `FontProvider` на работающем рендере. Используется shell-ом,
    /// чтобы передать `FontRegistry` с @font-face шрифтами после загрузки
    /// страницы (Renderer уже создан, builder-паттерн недоступен).
    pub fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>) {
        self.content_generation = self.content_generation.wrapping_add(1);
        self.font_provider = provider;
        // Новый провайдер может отвечать иначе на те же (families, weight,
        // style) — например, FontRegistry с загруженным @font-face.
        self.resolve_cache.clear();
    }

    /// Эагерно загружает указанные family-имена через текущий `FontProvider`,
    /// чтобы они были доступны для codepoint cascade ещё до первого `DrawText`
    /// с этой family-ой в CSS. Используется shell-ом для прогрева
    /// fallback-цепочки (Noto Color Emoji / Noto Sans CJK / etc.), без
    /// которой эмодзи и CJK на странице без явного `font-family` падают
    /// в `.notdef`. Имена, не найденные в провайдере или с битым TTF, тихо
    /// пропускаются. Берётся weight=400 + style=normal — для fallback-целей
    /// этого достаточно. Идемпотентно: повторный вызов на уже загруженной
    /// family не делает работы благодаря `face_id_by_path` cache-у.
    pub fn preload_fallback_chain(&mut self, families: &[&str]) {
        for name in families {
            let _ = self.resolve_face_id(
                &[(*name).to_string()],
                FontWeight::NORMAL,
                FontStyle::Normal,
                FontStretch::NORMAL,
            );
        }
    }

    /// Returns the normalized GPU fingerprint (vendor/renderer strings).
    ///
    /// Returns ("WebKit", "Generic GPU") regardless of actual adapter to prevent
    /// WebGL fingerprinting attacks (ADR-007 Layer 4).
    pub fn gpu_fingerprint(&self) -> &GpuFingerprint {
        &self.gpu_fingerprint
    }

    /// Shortcut: эагерно загружает `CURATED_FALLBACK_FAMILIES` (Noto Color
    /// Emoji / Noto Sans CJK / Apple Color Emoji / Segoe UI Emoji /
    /// PingFang / Hiragino / Microsoft YaHei / Yu Gothic / Malgun Gothic /
    /// Noto Sans Arabic / Hebrew / Devanagari / Thai). На каждой ОС
    /// найдётся лишь часть имён — остальные тихо пропустятся. Это
    /// разблокирует codepoint-cascade для эмодзи / CJK / RTL / Indic /
    /// Thai на страницах **без явного CSS `font-family`** для этих
    /// скриптов. Вызывается shell-ом один раз после `Renderer::new_async`.
    /// Идемпотентен (preload_fallback_chain → resolve_face_id cache).
    pub fn preload_curated_fallbacks(&mut self) {
        self.content_generation = self.content_generation.wrapping_add(1);
        self.preload_fallback_chain(crate::fallback::CURATED_FALLBACK_FAMILIES);
    }

    /// Резолвит `face_id` для `DrawText` с указанным `font-family` списком.
    /// Если `font_provider` есть — перебирает имена в порядке приоритета
    /// (CSS Fonts L4 §3.1), для первого найденного через [`Self::pick_family_face`]
    /// — лениво загружает TTF и возвращает `face_id`. Generic CSS-family-ы
    /// (`serif`/`sans-serif`/`monospace`/`cursive`/`fantasy`/`system-ui`) резолвятся
    /// через `FontProvider::pick_generic_face` (платформенная таблица кандидатов,
    /// BUG-128) — не пропускаются. Если ни одно имя не найдено — возвращает 0
    /// (default face).
    pub(super) fn resolve_face_id(
        &mut self,
        families: &[String],
        weight: FontWeight,
        style: FontStyle,
        stretch: FontStretch,
    ) -> usize {
        // DS-4: chrome never queries the CSS FontProvider — every chrome
        // `DrawText` passes an empty `font_family` (page content always has a
        // non-empty one, from the UA/author stylesheet's font-family cascade),
        // so an empty list defaults to the bundled chrome UI face (Golos
        // Text). Reserved bundled family names resolve directly here,
        // independent of whether a `FontProvider` is installed at all.
        if families.is_empty() {
            return self.chrome_face_id.unwrap_or(0);
        }
        for fam in families {
            match fam.as_str() {
                "Golos Text" => return self.chrome_face_id.unwrap_or(0),
                "Golos Text Medium" => {
                    return self.chrome_face_medium_id.or(self.chrome_face_id).unwrap_or(0);
                }
                "JetBrains Mono" => return self.mono_face_id.unwrap_or(0),
                _ => {}
            }
        }
        let Some(provider) = self.font_provider.clone() else {
            return 0;
        };
        // Мемоизация: горячий путь (каждый DrawText каждого кадра) — один
        // hash-lookup без аллокаций вместо to_lowercase + pick_face.
        let cache_key = Self::resolve_cache_key(families, weight, style, stretch);
        if let Some(&id) = self.resolve_cache.get(&cache_key) {
            return id;
        }
        let resolved =
            self.resolve_face_id_uncached(families, weight, style, stretch, &provider);
        self.resolve_cache.insert(cache_key, resolved);
        resolved
    }

    /// Ключ мемо-кэша [`Self::resolve_face_id`]: хэш `(families, weight,
    /// style, stretch)` без аллокаций. Вынесен, чтобы префетч и резолв
    /// считали ключ одинаково.
    fn resolve_cache_key(
        families: &[String],
        weight: FontWeight,
        style: FontStyle,
        stretch: FontStretch,
    ) -> u64 {
        use std::hash::Hasher;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for fam in families {
            h.write(fam.as_bytes());
            h.write_u8(0xFF); // разделитель — ["ab","c"] ≠ ["a","bc"]
        }
        h.write_u16(weight.0);
        h.write_u8(match style {
            FontStyle::Normal => 0,
            FontStyle::Italic => 1,
            FontStyle::Oblique => 2,
        });
        // Часть ключа: `pick_face` выбирает по stretch-у отдельный
        // condensed/expanded face, значит два stretch-а одного
        // (family, weight, style) — это два разных face_id.
        h.write_u16(stretch.0);
        h.finish()
    }

    /// Резолвит одну CSS-family в системный face.
    ///
    /// Generic-имя (`serif`/`sans-serif`/`monospace`/`cursive`/`fantasy`/
    /// `system-ui`) идёт через [`FontProvider::pick_generic_face`] — таблицу
    /// платформенных кандидатов (BUG-128); конкретное имя — напрямую через
    /// `pick_face`. Раньше generic-имена молча пропускались, и любой
    /// `font-family: serif` рисовался bundled-Inter-ом (sans).
    fn pick_family_face(
        provider: &Arc<dyn FontProvider>,
        family: &str,
        weight: u16,
        style: CssFontStyle,
        stretch: u16,
    ) -> Option<FaceRecord> {
        if lumen_core::ext::is_generic_family(family) {
            provider.pick_generic_face(family, weight, style, stretch)
        } else {
            provider.pick_face(family, weight, style, stretch)
        }
    }

    /// Конверсия paint-стиля в стиль `FontProvider`-а.
    fn css_style_of(style: FontStyle) -> CssFontStyle {
        match style {
            FontStyle::Normal => CssFontStyle::Normal,
            FontStyle::Italic => CssFontStyle::Italic,
            FontStyle::Oblique => CssFontStyle::Oblique,
        }
    }

    /// Параллельная предзагрузка face-ов для всех `DrawText` кадра
    /// (p1-exp-wgpu-only, ярус 1 «вынос загрузки face-ов с render-пути»).
    ///
    /// Раньше первый кадр страницы грузил каждый новый face
    /// ПОСЛЕДОВАТЕЛЬНО внутри пре-резолва: `fs::read` + WOFF-декод +
    /// `build_face_metrics` (~180 мс на 1000000-final.html). Здесь та же
    /// работа выполняется до резолва пачкой в scoped-потоках: диск и декод
    /// независимых face-ов идут параллельно, вставка в `self.faces` — на
    /// UI-потоке в детерминированном порядке (порядок первого появления в
    /// display list-е, как у последовательного кода).
    ///
    /// Семантика [`Self::resolve_face_id_uncached`] сохранена: грузится
    /// только первый `pick_face`-кандидат каждого списка family; если его
    /// загрузка/парсинг провалились — face просто не вставляется, и
    /// последующий последовательный резолв повторит попытку и упадёт на
    /// следующую family штатным путём (редкий случай битого шрифта).
    /// Тёплый кадр (все ключи в `resolve_cache`) не делает ничего.
    pub(super) fn prefetch_faces_parallel(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
    ) {
        let Some(provider) = self.font_provider.clone() else {
            return;
        };
        // Кандидаты: путь + байты из провайдера (@font-face virtual path)
        // либо None → fs::read в воркере + заявленный unicode-range записи
        // (FONTLOAD-9) и override-дескрипторы (FONTLOAD-17) записи — иначе
        // прогретый параллельно primary face потерял бы их, хотя
        // `load_face_by_record` (последовательный путь) проставил бы верно,
        // но эта функция его опережает.
        type PrefetchJob = (
            PathBuf,
            Option<Arc<[u8]>>,
            Vec<(u32, u32)>,
            Option<f32>,
            Option<f32>,
            Option<f32>,
            Vec<([u8; 4], f32)>,
        );
        let mut jobs: Vec<PrefetchJob> = Vec::new();
        let mut seen_keys: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let mut scheduled: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for cmd in content.iter().chain(overlay.iter()) {
            let DisplayCommand::DrawText {
                font_family, font_weight, font_style, font_stretch, ..
            } = cmd
            else {
                continue;
            };
            let key =
                Self::resolve_cache_key(font_family, *font_weight, *font_style, *font_stretch);
            if self.resolve_cache.contains_key(&key) || !seen_keys.insert(key) {
                continue;
            }
            for fam in font_family {
                // DS-4: reserved bundled chrome names resolve without the
                // provider in `resolve_face_id` — skip them here too, else
                // every frame re-attempts a `pick_face` lookup for hot chrome
                // text (omnibox, DevTools) that never gets cached (the actual
                // resolve short-circuits before reaching `resolve_cache`).
                if matches!(fam.as_str(), "Golos Text" | "Golos Text Medium" | "JetBrains Mono") {
                    continue;
                }
                let Some(rec) = Self::pick_family_face(
                    &provider,
                    fam,
                    font_weight.0,
                    Self::css_style_of(*font_style),
                    font_stretch.as_percent(),
                ) else {
                    continue;
                };
                if !self.face_id_by_path.contains_key(&rec.path)
                    && !scheduled.contains(&rec.path)
                {
                    let mem = provider.read_face_bytes(&rec.path);
                    scheduled.insert(rec.path.clone());
                    jobs.push((
                        rec.path,
                        mem,
                        rec.unicode_ranges.clone(),
                        rec.ascent_override,
                        rec.descent_override,
                        rec.size_adjust,
                        rec.variation_settings.clone(),
                    ));
                }
                break; // как в резолве: первый pick_face-хит завершает перебор
            }
        }
        if jobs.is_empty() {
            return;
        }

        // Воркеры разбирают job-ы через атомарный курсор; результат кладётся
        // по индексу job-а — порядок вставки детерминирован.
        let n_workers = std::thread::available_parallelism()
            .map_or(4, std::num::NonZeroUsize::get)
            .min(jobs.len())
            .min(8);
        let cursor = std::sync::atomic::AtomicUsize::new(0);
        // Слот результата job-а: байты шрифта + построенные метрики.
        type FaceSlot = std::sync::Mutex<Option<(Arc<[u8]>, FaceMetrics)>>;
        let results: Vec<FaceSlot> =
            jobs.iter().map(|_| std::sync::Mutex::new(None)).collect();
        std::thread::scope(|s| {
            for _ in 0..n_workers {
                s.spawn(|| {
                    loop {
                        let i = cursor.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((path, mem, ..)) = jobs.get(i) else {
                            break;
                        };
                        // `mem` (Arc из @font-face-реестра) клонируется как
                        // счётчик ссылок; диск читается в новый Arc (BUG-272).
                        let raw: Arc<[u8]> = match mem {
                            Some(bytes) => Arc::clone(bytes),
                            None => match std::fs::read(path) {
                                Ok(b) => Arc::from(b),
                                Err(_) => continue,
                            },
                        };
                        // Ошибки декода/парсинга здесь НЕ логируются: face
                        // просто не вставляется, последовательный резолв
                        // повторит попытку и залогирует штатно (без дублей).
                        // `Ok(None)` (уже sfnt — как все @font-face-байты) отдаёт
                        // тот же Arc, что и реестр: рендер разделяет буфер.
                        let bytes: Arc<[u8]> = match maybe_decode_font(&raw) {
                            Ok(Some(decoded)) => Arc::from(decoded),
                            Ok(None) => raw,
                            Err(_) => continue,
                        };
                        let Some(metrics) = build_face_metrics(&bytes) else {
                            continue;
                        };
                        if let Ok(mut slot) = results[i].lock() {
                            *slot = Some((bytes, metrics));
                        }
                    }
                });
            }
        });

        for (
            (path, _, unicode_ranges, ascent_override, descent_override, size_adjust, variation_settings),
            slot,
        ) in jobs.into_iter().zip(results)
        {
            let Ok(mut guard) = slot.lock() else { continue };
            let Some((bytes, metrics)) = guard.take() else {
                continue; // битый шрифт: последовательный резолв повторит и залогирует
            };
            let id = self.faces.len();
            self.faces.push(LoadedFace {
                bytes,
                metrics: Some(metrics),
                unicode_ranges,
                ascent_override,
                descent_override,
                size_adjust,
                variation_settings,
            });
            self.face_id_by_path.insert(path, id);
        }
    }

    /// Loads one `FaceRecord` into `self.faces` (WOFF/WOFF2 → sfnt decode +
    /// `build_face_metrics`) and returns its `face_id`. `None` on read/decode/
    /// parse failure. Idempotent: an already-loaded path is looked up instead
    /// of reparsed. Factored out of [`Self::resolve_face_id_uncached`] so both
    /// the primary face and its same-bucket siblings (BUG-434) share the
    /// loading logic.
    fn load_face_by_record(
        &mut self,
        rec: &FaceRecord,
        provider: &Arc<dyn FontProvider>,
    ) -> Option<usize> {
        if let Some(&id) = self.face_id_by_path.get(&rec.path) {
            return Some(id);
        }
        // @font-face in-memory байты (virtual path) или диск для системных шрифтов.
        // Реестр отдаёт Arc (клон = счётчик ссылок), диск — новый Arc (BUG-272).
        let raw: Arc<[u8]> = if let Some(mem_bytes) = provider.read_face_bytes(&rec.path) {
            mem_bytes
        } else {
            let disk_bytes = std::fs::read(&rec.path).ok()?;
            Arc::from(disk_bytes)
        };
        // Transparent WOFF/WOFF2 → sfnt conversion before parsing.
        // `Ok(None)` (@font-face-байты уже sfnt) переиспользует Arc реестра.
        let bytes: Arc<[u8]> = match maybe_decode_font(&raw) {
            Ok(Some(decoded)) => Arc::from(decoded),
            Ok(None) => raw,
            Err(e) => {
                eprintln!("[font] WOFF decode failed {}: {e}", rec.path.display());
                return None;
            }
        };
        let Some(metrics) = build_face_metrics(&bytes) else {
            eprintln!("[font] parse failed {}", rec.path.display());
            return None;
        };
        let id = self.faces.len();
        self.faces.push(LoadedFace {
            bytes,
            metrics: Some(metrics),
            unicode_ranges: rec.unicode_ranges.clone(),
            ascent_override: rec.ascent_override,
            descent_override: rec.descent_override,
            size_adjust: rec.size_adjust,
            variation_settings: rec.variation_settings.clone(),
        });
        self.face_id_by_path.insert(rec.path.clone(), id);
        Some(id)
    }

    /// Полный (немемоизированный) резолв — вынесен из [`Self::resolve_face_id`],
    /// который добавляет кэш поверх.
    fn resolve_face_id_uncached(
        &mut self,
        families: &[String],
        weight: FontWeight,
        style: FontStyle,
        stretch: FontStretch,
        provider: &Arc<dyn FontProvider>,
    ) -> usize {
        for fam in families {
            let Some(rec) = Self::pick_family_face(
                provider,
                fam,
                weight.0,
                Self::css_style_of(style),
                stretch.as_percent(),
            ) else {
                continue;
            };
            let Some(primary_id) = self.load_face_by_record(&rec, provider) else {
                continue;
            };
            // BUG-434: @font-face subsets of the same (family, weight, style,
            // stretch) partition the codepoint space via non-overlapping
            // `unicode-range` (CSS Fonts L4 §5.1) instead of competing —
            // `pick_family_face` only ever returns one of them. Load every
            // sibling too, so `pick_face_for_codepoint`'s cmap scan over
            // `self.faces` can actually find the one that has the glyph,
            // instead of silently missing subsets that were never parsed.
            for sibling in provider.lookup_faces(fam) {
                if sibling.path == rec.path
                    || sibling.weight != rec.weight
                    || sibling.style != rec.style
                    || sibling.stretch != rec.stretch
                {
                    continue;
                }
                self.load_face_by_record(&sibling, provider);
            }
            return primary_id;
        }
        0
    }
}
