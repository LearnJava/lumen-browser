//! PH3-19 / BUG-1154 / BUG-1634: `@font-face url()` loading of the current
//! document — which pending blocks are actually needed (CSS Fonts 4 §4.5) and
//! the background fetches that register them.
//!
//! Moved verbatim out of `page_load.rs` (file-size gate, docs/lint-policy.md
//! §5.1). Method bodies are unchanged; only module path and visibility differ.

use crate::*;

impl Lumen {
    /// BUG-1634 (CSS Fonts 4 §4.5): из `incoming` и ранее отложенных блоков оставляет
    /// те, чей `unicode-range` пересекается с символами документа; остальные
    /// возвращаются в `deferred_web_fonts` и ждут, пока такой символ появится
    /// (проверка идёт на каждом relayout). Блоки без `unicode-range` нужны всегда.
    /// Уже запрошенные и уже отложенные источники отсеиваются здесь же, поэтому
    /// повторная сборка листа (`spawn_dynamic_web_fonts`) не плодит дубликатов.
    pub(crate) fn take_needed_web_fonts(&mut self, incoming: Vec<PendingWebFont>) -> Vec<PendingWebFont> {
        let key_of = |pf: &PendingWebFont| format!("{}|{}|{:?}|{}", pf.family, pf.weight, pf.style, pf.url);
        let mut all = std::mem::take(&mut self.deferred_web_fonts);
        if all.is_empty() && incoming.is_empty() {
            return Vec::new();
        }
        let mut seen: std::collections::HashSet<String> = all.iter().map(key_of).collect();
        for pf in incoming {
            let key = key_of(&pf);
            if !self.requested_web_fonts.contains(&key) && seen.insert(key) {
                all.push(pf);
            }
        }
        // Нет документа — нечего сверять, грузим всё, как до BUG-1634.
        let Some(doc) = self.layout_source.as_ref().map(|src| std::sync::Arc::clone(&src.document)) else {
            return all;
        };
        let codepoints = match doc.try_lock() {
            Ok(d) => crate::web_font_usage::document_codepoints(&d),
            Err(std::sync::TryLockError::Poisoned(e)) => crate::web_font_usage::document_codepoints(&e.into_inner()),
            // Документ занят другим потоком: перепроверим на следующем relayout.
            Err(std::sync::TryLockError::WouldBlock) => {
                self.deferred_web_fonts = all;
                return Vec::new();
            }
        };
        let (needed, deferred) = crate::web_font_usage::partition_by_usage(all, &codepoints);
        self.deferred_web_fonts = deferred;
        needed
    }

    /// PH3-19 / BUG-1154: запускает фоновую загрузку `@font-face url()`-источников
    /// (CSP `font-src`-гейт, `upgrade-insecure-requests`, результат —
    /// `LoadEvent::FontLoaded`). Зовётся из `apply_loaded_page` для первичной
    /// сборки и из `refresh_dynamic_css` для правил, появившихся после загрузки
    /// (поздно вставленный `<style>`); уже запрошенные источники пропускаются.
    pub(crate) fn spawn_web_font_fetches(&mut self, pending: Vec<PendingWebFont>) {
        let pending = self.take_needed_web_fonts(pending);
        if !pending.is_empty() {
            let base_opt = self.document_resource_base();
            // GAP-CSPENF срез 19: `font-src`/`default-src` против `@font-face
            // url()` — последний из трёх производителей среза 4 (`img-src`/
            // `style-src`/`connect-src`… уже гейтили свои сети), которого
            // не было вовсе (`grep -n FontSrc crates/` до этого среза не
            // находил ничего — директива не была даже распарсена). Проверка
            // стоит здесь, на потоке, породившем `page`, а не внутри
            // спавненного `std::thread::spawn` ниже: у детач-потока нет
            // `&self` (нет `js_ctx`/`engine_thread` для диспатча события),
            // та же причина, по которой срезы 10-13/16/17 переносили гейт в
            // `lumen-network` — здесь переносить некуда, читатель URL уже
            // синхронный и на главном потоке, поэтому решение «фетчить или
            // нет» принимается тут же, до `std::thread::spawn`.
            let csp_gate = self.layout_source.as_ref().and_then(|src| {
                let doc = src.document.lock().ok()?;
                let root = doc.root();
                crate::csp_enforce::document_csp_policy(&doc, root)
            });
            // GAP-REFERRER срез 5: same one-shot read as `csp_gate` above,
            // captured before the detached thread below the same way it is.
            let referrer_policy = self.layout_source.as_ref().map_or_else(
                lumen_network::ReferrerPolicy::default_policy,
                |src| {
                    src.document.lock().map_or_else(
                        |_| lumen_network::ReferrerPolicy::default_policy(),
                        |d| crate::resource_base::document_referrer_policy(&d),
                    )
                },
            );
            let self_origin = base_opt.as_ref().and_then(|b| b.origin());
            for pf in pending {
                // BUG-1154: один и тот же источник (например, `<style>` с
                // `@font-face` пересобирается при каждой правке документа) не
                // должен порождать второй запрос.
                let key = format!("{}|{}|{:?}|{}", pf.family, pf.weight, pf.style, pf.url);
                if !self.requested_web_fonts.insert(key) {
                    continue;
                }
                if let Some(base) = base_opt.clone() {
                    // GAP-CSPENF срез 48: `upgrade-insecure-requests`
                    // переписывает `http://` в `https://` до гейта
                    // `font-src` (тот же порядок Fetch §4.1, что срезы
                    // 43-47 уже дали картинкам/скриптам/CSS) — `gate_url`
                    // и есть то, что реально уходит в `fetch_font_bytes`.
                    let resolved = base.resolve_str(&pf.url);
                    let gate_url = csp_gate.as_ref()
                        .and_then(|(policy, _)| crate::csp_enforce::upgrade_insecure_url(policy, &resolved))
                        .unwrap_or_else(|| resolved.clone());
                    let violated: Vec<String> = csp_gate.as_ref().map_or_else(Vec::new, |(policy, _)| {
                        crate::csp_enforce::violating_fetch_policy(
                            policy, &lumen_network::csp::CspDirective::FontSrc, &gate_url, self_origin.as_ref(),
                        ).into_iter().map(str::to_owned).collect()
                    });
                    if !violated.is_empty() {
                        // Срез 58: одно событие на каждую нарушенную политику.
                        let blocked_url = gate_url.clone();
                        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                            for policy_text in &violated {
                                j.fire_csp_violation("font-src", &blocked_url, policy_text);
                            }
                        });
                        continue;
                    }
                    let sink = Arc::clone(&self.event_sink);
                    let cookie_jar = self.active_cookie_jar();
                    let proxy = self.load_proxy.clone();
                    let fetches = self.web_font_fetches.clone();
                    std::thread::spawn(move || {
                        // BUG-1650: вариативный шрифт приходит несколькими блоками
                        // `@font-face` на один URL — сеть и декод один раз на URL.
                        let family = pf.family.clone();
                        let bytes = fetches.get_or_fetch(&gate_url, || {
                            let raw = match fetch_font_bytes(&gate_url, &base, &sink, Some(cookie_jar), referrer_policy) {
                                Ok(b) => b,
                                Err(e) => {
                                    eprintln!("@font-face «{family}»: не загружен {}: {e}", pf.url);
                                    return None;
                                }
                            };
                            let bytes = match lumen_font::maybe_decode_font(&raw) {
                                Ok(Some(d)) => d,
                                Ok(None) => raw,
                                Err(e) => {
                                    eprintln!("@font-face «{family}»: WOFF-декод провалился: {e}");
                                    return None;
                                }
                            };
                            if lumen_font::Font::parse(&bytes).is_err() {
                                eprintln!("@font-face «{family}»: невалидный sfnt {}", pf.url);
                                return None;
                            }
                            Some(bytes)
                        });
                        let Some(bytes) = bytes else { return };
                        let bytes = bytes.as_ref().clone();
                        eprintln!("@font-face async загружен: «{}» weight={}", pf.family, pf.weight);
                        let unicode_range = pf.unicode_range_str
                            .as_deref()
                            .map(lumen_font::parse_unicode_ranges)
                            .unwrap_or_default();
                        // CSS Fonts L4 §14 (FONTLOAD-11/12/13, BUG-467): ascent/descent/line-gap-override, size-adjust.
                        let ascent_override = pf.ascent_override_str.as_deref()
                            .and_then(lumen_font::parse_metric_override_percent);
                        let descent_override = pf.descent_override_str.as_deref()
                            .and_then(lumen_font::parse_metric_override_percent);
                        let size_adjust = pf.size_adjust_str.as_deref()
                            .and_then(lumen_font::parse_metric_override_percent);
                        let line_gap_override = pf.line_gap_override_str.as_deref()
                            .and_then(lumen_font::parse_metric_override_percent);
                        // FONTLOAD-20 (BUG-467): font-variation-settings дескриптор,
                        // тот же приём, что четыре override-строки выше.
                        let variation_settings = pf.variation_settings_str.as_deref()
                            .map(lumen_font::parse_variation_settings)
                            .unwrap_or_default();
                        let _ = proxy.send_event(LoadEvent::FontLoaded {
                            family: pf.family,
                            weight: pf.weight,
                            style: pf.style,
                            unicode_range,
                            ascent_override,
                            descent_override,
                            size_adjust,
                            line_gap_override,
                            variation_settings,
                            bytes,
                        });
                    });
                }
            }
        }
    }
}
