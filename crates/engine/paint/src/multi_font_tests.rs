//! P1/SPLIT-PT1: тесты мультишрифтового резолва (`mod multi_font_tests` из `lib.rs`).
//! Перенесено байт-в-байт без дедента (приём ST-1/DL-1).

    use super::*;
    use lumen_layout::TextMeasurer;

    static INTER: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

    /// Путь к bundled JetBrains Mono — «системный» моноширинный шрифт для
    /// generic-тестов (метрики заведомо отличаются от пропорционального Inter).
    const MONO_PATH: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../../assets/fonts/JetBrainsMono-Regular.ttf");

    /// Имя «установленного» конкретного системного семейства — не generic и не
    /// кандидат ни одного generic-а, поэтому найти его можно только через
    /// именной резолв (BUG-128, п.1).
    const NAMED_FAMILY: &str = "Lumen Test Mono";

    /// Префикс имён для проверки предела кэша: провайдер «знает» их сколько
    /// угодно.
    const CAP_FAMILY_PREFIX: &str = "cap-family-";

    /// Провайдер, у которого «установлены» первый платформенный кандидат на
    /// `monospace`, конкретное семейство [`NAMED_FAMILY`] и сколь угодно много
    /// `cap-family-<N>` — все указывают на bundled JetBrains Mono.
    ///
    /// Считает обращения к индексу: тесты ленивого кэша требуют ровно одного
    /// резолва на имя, сколько бы символов им ни мерили.
    struct MonoOnlyProvider {
        /// Число вызовов [`FontProvider::lookup_family`] с момента создания.
        lookups: std::sync::atomic::AtomicUsize,
    }

    impl MonoOnlyProvider {
        fn new() -> Self {
            Self { lookups: std::sync::atomic::AtomicUsize::new(0) }
        }

        /// Сколько раз провайдер спрашивали об именах семейств.
        fn lookups(&self) -> usize {
            self.lookups.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl lumen_core::ext::FontProvider for MonoOnlyProvider {
        fn lookup_family(&self, family: &str) -> Vec<std::path::PathBuf> {
            self.lookups.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let first = lumen_core::ext::generic_family_candidates("monospace")[0];
            let installed = family.eq_ignore_ascii_case(first)
                || family.eq_ignore_ascii_case(NAMED_FAMILY)
                || family.to_ascii_lowercase().starts_with(CAP_FAMILY_PREFIX);
            if installed {
                vec![std::path::PathBuf::from(MONO_PATH)]
            } else {
                Vec::new()
            }
        }
        fn list_families(&self) -> Vec<String> {
            vec![
                lumen_core::ext::generic_family_candidates("monospace")[0].to_string(),
                NAMED_FAMILY.to_string(),
            ]
        }
    }

    #[test]
    fn named_system_family_measured_with_system_face_not_inter() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        // Ни @font-face, ни generic — только конкретное системное имя.
        let families = vec![NAMED_FAMILY.to_string()];
        let inter_i = m.char_width_with_families('i', 16.0, &families);
        let inter_w = m.char_width_with_families('W', 16.0, &families);
        assert!(inter_i < inter_w, "Inter пропорционален: {inter_i} vs {inter_w}");

        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(
            MonoOnlyProvider::new(),
        ))));
        let mono_i = m.char_width_with_families('i', 16.0, &families);
        let mono_w = m.char_width_with_families('W', 16.0, &families);
        assert!(
            (mono_i - mono_w).abs() < 0.01,
            "конкретное системное семейство должно мериться своим face-ом, {mono_i} vs {mono_w}"
        );
    }

    #[test]
    fn named_family_matched_case_insensitively() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(
            MonoOnlyProvider::new(),
        ))));
        // CSS Fonts L4 §4.3: имена семейств сравниваются case-insensitive.
        let exact = m.char_width_with_families('W', 16.0, &[NAMED_FAMILY.to_string()]);
        let shouty = m.char_width_with_families('W', 16.0, &[NAMED_FAMILY.to_uppercase()]);
        assert!((exact - shouty).abs() < f32::EPSILON, "{exact} vs {shouty}");
    }

    #[test]
    fn named_family_resolved_once_and_reused() {
        let provider = Arc::new(MonoOnlyProvider::new());
        let set = SystemFaceSet::from_provider(provider.clone());
        let after_generics = provider.lookups();

        for _ in 0..32 {
            assert!(set.metrics(&NAMED_FAMILY.to_ascii_lowercase()).is_some());
        }
        assert_eq!(
            provider.lookups(),
            after_generics + 1,
            "резолв конкретного семейства обязан быть ленивым и однократным"
        );

        // Отрицательный ответ кэшируется так же: в системе такого нет.
        for _ in 0..32 {
            assert!(set.metrics("no such family").is_none());
        }
        assert_eq!(
            provider.lookups(),
            after_generics + 2,
            "промах тоже кэшируется, иначе индекс опрашивается на каждый символ"
        );
    }

    /// BUG-625: зарезервированные bundled-имена хрома меряются теми же
    /// bundled-байтами, которыми их рисует рендер, — а не bundled Inter-ом и
    /// не одноимённым системным шрифтом.
    #[test]
    fn chrome_bundled_families_measured_with_bundled_faces() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let mono = vec!["JetBrains Mono".to_string()];
        let golos = vec!["Golos Text".to_string()];
        let inter_i = m.char_width_with_families('i', 16.0, &mono);
        let inter_w = m.char_width_with_families('W', 16.0, &mono);
        assert!(inter_i < inter_w, "до регистрации — пропорциональный Inter");
        let golos_before = m.char_width_with_families('a', 16.0, &golos);

        m.register_chrome_bundled_families();
        assert_eq!(m.family_count(), 3);

        let mono_i = m.char_width_with_families('i', 16.0, &mono);
        let mono_w = m.char_width_with_families('W', 16.0, &mono);
        assert!((mono_i - mono_w).abs() < 0.01, "JetBrains Mono моноширинный: {mono_i} vs {mono_w}");
        let expected = OwnedFontMetrics::from_bytes(crate::chrome_fonts::GOLOS_TEXT_REGULAR)
            .unwrap()
            .try_char_width('a', 16.0)
            .unwrap();
        let golos_after = m.char_width_with_families('a', 16.0, &golos);
        assert!((golos_after - expected).abs() < f32::EPSILON, "{golos_after} vs {expected}");
        assert!((golos_after - golos_before).abs() > 0.01, "Golos ≠ Inter: {golos_before}");
    }

    #[test]
    fn unknown_named_family_falls_back_to_inter() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["No Such Family".to_string()];
        let before = m.char_width_with_families('W', 16.0, &families);
        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(
            MonoOnlyProvider::new(),
        ))));
        let after = m.char_width_with_families('W', 16.0, &families);
        assert!((after - before).abs() < f32::EPSILON, "{before} → {after}");
    }

    #[test]
    fn named_face_cache_is_bounded() {
        let set = SystemFaceSet::from_provider(Arc::new(MonoOnlyProvider::new()));
        for i in 0..MAX_CACHED_NAMED_FACES {
            assert!(
                set.metrics(&format!("{CAP_FAMILY_PREFIX}{i}")).is_some(),
                "первые {MAX_CACHED_NAMED_FACES} семейств кэшируются"
            );
        }
        // Сверх предела метрики не удерживаются: страница со списком из тысячи
        // имён не должна держать тысячу cmap-ов до конца процесса.
        assert!(set.metrics(&format!("{CAP_FAMILY_PREFIX}overflow")).is_none());
        assert_eq!(set.cached_named_face_count(), MAX_CACHED_NAMED_FACES);
    }

    #[test]
    fn font_face_family_wins_over_same_named_system_family() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(
            MonoOnlyProvider::new(),
        ))));
        let families = vec![NAMED_FAMILY.to_string()];
        let system_w = m.char_width_with_families('W', 16.0, &families);
        // CSS Fonts L4 §5: @font-face затеняет одноимённое системное семейство.
        m.register_family(NAMED_FAMILY, INTER.to_vec());
        let web_w = m.char_width_with_families('W', 16.0, &families);
        assert!(
            (web_w - m.char_width('W', 16.0)).abs() < f32::EPSILON && web_w != system_w,
            "@font-face должен победить системное имя: {system_w} → {web_w}"
        );
    }

    #[test]
    fn generic_monospace_measured_with_system_face_not_inter() {
        let font = inter_font();
        let generics = Arc::new(SystemFaceSet::from_provider(Arc::new(MonoOnlyProvider::new())));
        assert_eq!(generics.resolved_generic_count(), 1, "резолвиться должен только monospace");

        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["monospace".to_string()];
        // До подключения набора generic мерится bundled Inter-ом —
        // пропорциональным, значит 'i' уже 'W'.
        let inter_i = m.char_width_with_families('i', 16.0, &families);
        let inter_w = m.char_width_with_families('W', 16.0, &families);
        assert!(inter_i < inter_w, "Inter пропорционален: {inter_i} vs {inter_w}");

        m.set_system_faces(generics);
        let mono_i = m.char_width_with_families('i', 16.0, &families);
        let mono_w = m.char_width_with_families('W', 16.0, &families);
        assert!(
            (mono_i - mono_w).abs() < 0.01,
            "system monospace face: advance должен совпадать, {mono_i} vs {mono_w}"
        );
    }

    #[test]
    fn unresolved_generic_falls_back_to_inter() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["serif".to_string()];
        let before = m.char_width_with_families('W', 16.0, &families);
        // MonoOnlyProvider не знает ни одного serif-кандидата → generic
        // остаётся нерезолвленным, измеритель обязан вести себя как раньше.
        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(MonoOnlyProvider::new()))));
        assert!((m.char_width_with_families('W', 16.0, &families) - before).abs() < f32::EPSILON);
    }

    #[test]
    fn empty_generic_set_keeps_inter_metrics() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["monospace".to_string()];
        let before = m.char_width_with_families('W', 16.0, &families);
        m.set_system_faces(Arc::new(SystemFaceSet::empty()));
        assert!((m.char_width_with_families('W', 16.0, &families) - before).abs() < f32::EPSILON);
    }

    fn inter_font() -> lumen_font::Font<'static> {
        lumen_font::Font::parse(INTER).expect("Inter TTF должен парситься")
    }

    #[test]
    fn new_creates_measurer_with_fallback() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        assert_eq!(m.family_count(), 0);
        // Fallback (Inter) должен давать ненулевую ширину для ASCII
        let w = m.char_width('A', 16.0);
        assert!(w > 0.0, "Inter должен дать ненулевую ширину для 'A'");
    }

    #[test]
    fn char_width_with_families_falls_back_to_inter_when_no_family_registered() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        let w_direct = m.char_width('A', 16.0);
        let w_families = m.char_width_with_families('A', 16.0, &["nonexistent".to_string()]);
        assert_eq!(w_direct, w_families, "без зарегистрированных семей должен использоваться fallback");
    }

    #[test]
    fn char_width_with_empty_families_uses_fallback() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        let w_direct = m.char_width('B', 20.0);
        let w_families = m.char_width_with_families('B', 20.0, &[]);
        assert_eq!(w_direct, w_families);
    }

    #[test]
    fn register_family_increases_count() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("testfont", INTER.to_vec());
        assert_eq!(m.family_count(), 1);
    }

    #[test]
    fn register_family_with_bad_bytes_is_ignored() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("broken", vec![0u8; 16]); // явно не шрифт
        assert_eq!(m.family_count(), 0, "сломанный шрифт должен тихо игнорироваться");
    }

    #[test]
    fn char_width_with_registered_family_uses_that_font() {
        // Регистрируем Inter под новым именем — должна быть та же ширина, что и от fallback
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("inter-copy", INTER.to_vec());
        let w_fallback = m.char_width('H', 16.0);
        let w_family = m.char_width_with_families('H', 16.0, &["inter-copy".to_string()]);
        // Inter registered → Inter fallback: должны совпадать
        assert!((w_fallback - w_family).abs() < 0.01, "ширины должны совпадать: {w_fallback} vs {w_family}");
    }

    #[test]
    fn family_lookup_is_case_insensitive() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("MyFont", INTER.to_vec());
        // Запрашиваем под разными регистрами
        let w1 = m.char_width_with_families('X', 16.0, &["myfont".to_string()]);
        let w2 = m.char_width_with_families('X', 16.0, &["MYFONT".to_string()]);
        let w3 = m.char_width_with_families('X', 16.0, &["MyFont".to_string()]);
        assert!(w1 > 0.0 && w1 == w2 && w2 == w3, "lookup должен быть case-insensitive");
    }

    // ── ascent/descent_px_with_families (FONTLOAD-10, BUG-467) ──────────────

    #[test]
    fn ascent_descent_with_families_falls_back_to_inter_when_unregistered() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["nonexistent".to_string()];
        assert_eq!(m.ascent_px(16.0), m.ascent_px_with_families(16.0, &families));
        assert_eq!(m.descent_px(16.0), m.descent_px_with_families(16.0, &families));
    }

    #[test]
    fn ascent_descent_with_empty_families_uses_fallback() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        assert_eq!(m.ascent_px(16.0), m.ascent_px_with_families(16.0, &[]));
        assert_eq!(m.descent_px(16.0), m.descent_px_with_families(16.0, &[]));
    }

    #[test]
    fn ascent_descent_with_families_uses_registered_font_not_bundled_fallback() {
        // JetBrains Mono has different hhea/OS2 metrics from bundled Inter —
        // registering it must move ascent/descent away from the fallback's
        // hardcoded values (FONTLOAD-9's bug report: before this slice,
        // `MultiFontMeasurer::ascent_px`/`descent_px` always delegated to
        // `self.fallback` regardless of which family was actually selected).
        let mono_bytes = std::fs::read(MONO_PATH).expect("bundled JetBrains Mono должен читаться");
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("registered-mono", mono_bytes);
        let families = vec!["registered-mono".to_string()];
        let inter_ascent = m.ascent_px(16.0);
        let mono_ascent = m.ascent_px_with_families(16.0, &families);
        assert_ne!(
            inter_ascent, mono_ascent,
            "ascent зарегистрированного @font-face должен отличаться от bundled fallback"
        );
        let inter_descent = m.descent_px(16.0);
        let mono_descent = m.descent_px_with_families(16.0, &families);
        assert_ne!(
            inter_descent, mono_descent,
            "descent зарегистрированного @font-face должен отличаться от bundled fallback"
        );
    }

    #[test]
    fn ascent_descent_with_families_picks_first_resolving_family() {
        // CSS font stack fallback: первая семья без данных пропускается,
        // используется первая, у которой они есть — тот же приоритет, что
        // `resolve_font_stretch` уже применяет к `wdth`-оси.
        let mono_bytes = std::fs::read(MONO_PATH).expect("bundled JetBrains Mono должен читаться");
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("registered-mono", mono_bytes);
        let direct = vec!["registered-mono".to_string()];
        let with_missing_first = vec!["no-such-family".to_string(), "registered-mono".to_string()];
        assert_eq!(
            m.ascent_px_with_families(16.0, &direct),
            m.ascent_px_with_families(16.0, &with_missing_first)
        );
        assert_eq!(
            m.descent_px_with_families(16.0, &direct),
            m.descent_px_with_families(16.0, &with_missing_first)
        );
    }

    #[test]
    fn ascent_descent_with_families_uses_system_face_not_inter() {
        // BUG-128 симметрия: системное имя (не @font-face) тоже обязано
        // мериться своим face-ом, а не bundled Inter-ом.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec![NAMED_FAMILY.to_string()];
        let inter_ascent = m.ascent_px_with_families(16.0, &families);
        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(
            MonoOnlyProvider::new(),
        ))));
        let mono_ascent = m.ascent_px_with_families(16.0, &families);
        assert_ne!(
            inter_ascent, mono_ascent,
            "системное имя должно мериться своим face-ом: {inter_ascent} vs {mono_ascent}"
        );
    }

    // ── metric-override дескрипторы (FONTLOAD-11, BUG-467) ──────────────────

    #[test]
    fn ascent_override_replaces_real_metric_with_font_size_fraction() {
        // CSS Fonts L4 §14: ascent-override — доля font-size, не face-а.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family_with_overrides(
            "overridden",
            INTER.to_vec(),
            Vec::new(),
            Some(1.0), // 100%
            None,
            None,
            None,
        );
        let families = vec!["overridden".to_string()];
        assert_eq!(m.ascent_px_with_families(20.0, &families), 20.0);
    }

    #[test]
    fn descent_override_replaces_real_metric_with_font_size_fraction() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family_with_overrides(
            "overridden",
            INTER.to_vec(),
            Vec::new(),
            None,
            Some(0.5), // 50%
            None,
            None,
        );
        let families = vec!["overridden".to_string()];
        assert_eq!(m.descent_px_with_families(20.0, &families), 10.0);
    }

    #[test]
    fn metric_override_none_matches_register_family_with_ranges() {
        // `None` (CSS `normal`/дескриптор отсутствует) не должен ничего
        // менять относительно пути без overrides (FONTLOAD-10 baseline).
        let font = inter_font();
        let mut plain = MultiFontMeasurer::new(&font).unwrap();
        plain.register_family_with_ranges("plain", INTER.to_vec(), Vec::new());
        let mut overridden = MultiFontMeasurer::new(&font).unwrap();
        overridden.register_family_with_overrides(
            "plain", INTER.to_vec(), Vec::new(), None, None, None, None,
        );
        let families = vec!["plain".to_string()];
        assert_eq!(
            plain.ascent_px_with_families(16.0, &families),
            overridden.ascent_px_with_families(16.0, &families)
        );
        assert_eq!(
            plain.descent_px_with_families(16.0, &families),
            overridden.descent_px_with_families(16.0, &families)
        );
    }

    #[test]
    fn ascent_override_alone_leaves_descent_at_real_metric() {
        // Асимметричный override: только ascent задан — descent остаётся
        // реальной метрикой face-а, а не тоже подменяется.
        let mono_bytes = std::fs::read(MONO_PATH).expect("bundled JetBrains Mono должен читаться");
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family_with_overrides(
            "mono-ascent-override",
            mono_bytes.clone(),
            Vec::new(),
            Some(1.0),
            None,
            None,
            None,
        );
        let mut baseline = MultiFontMeasurer::new(&font).unwrap();
        baseline.register_family("mono-baseline", mono_bytes);
        let overridden_families = vec!["mono-ascent-override".to_string()];
        let baseline_families = vec!["mono-baseline".to_string()];
        assert_eq!(m.ascent_px_with_families(16.0, &overridden_families), 16.0);
        assert_eq!(
            m.descent_px_with_families(16.0, &overridden_families),
            baseline.descent_px_with_families(16.0, &baseline_families),
            "descent без собственного override должен остаться реальной метрикой face-а"
        );
    }

    // ── `size-adjust` дескриптор (CSS Fonts L4 §14.4, FONTLOAD-12) ─────────

    #[test]
    fn size_adjust_scales_ascent_and_descent_like_a_bigger_font_size() {
        // CSS Fonts L4 §14.4 + size-adjust-01.html (WPT): `size-adjust: 150%`
        // at font-size N must render like the SAME face at font-size N*1.5 —
        // not like a separate multiplier bolted onto the real metric.
        let mono_bytes = std::fs::read(MONO_PATH).expect("bundled JetBrains Mono должен читаться");
        let font = inter_font();
        let mut adjusted = MultiFontMeasurer::new(&font).unwrap();
        adjusted.register_family_with_overrides(
            "mono-size-adjust",
            mono_bytes.clone(),
            Vec::new(),
            None,
            None,
            Some(1.5), // 150%
            None,
        );
        let mut baseline = MultiFontMeasurer::new(&font).unwrap();
        baseline.register_family("mono-baseline", mono_bytes);
        let adjusted_families = vec!["mono-size-adjust".to_string()];
        let baseline_families = vec!["mono-baseline".to_string()];
        assert_eq!(
            adjusted.ascent_px_with_families(20.0, &adjusted_families),
            baseline.ascent_px_with_families(30.0, &baseline_families),
        );
        assert_eq!(
            adjusted.descent_px_with_families(20.0, &adjusted_families),
            baseline.descent_px_with_families(30.0, &baseline_families),
        );
    }

    #[test]
    fn size_adjust_scales_char_width_like_a_bigger_font_size() {
        let mono_bytes = std::fs::read(MONO_PATH).expect("bundled JetBrains Mono должен читаться");
        let font = inter_font();
        let mut adjusted = MultiFontMeasurer::new(&font).unwrap();
        adjusted.register_family_with_overrides(
            "mono-size-adjust",
            mono_bytes.clone(),
            Vec::new(),
            None,
            None,
            Some(2.0), // 200%
            None,
        );
        let mut baseline = MultiFontMeasurer::new(&font).unwrap();
        baseline.register_family("mono-baseline", mono_bytes);
        let adjusted_families = vec!["mono-size-adjust".to_string()];
        let baseline_families = vec!["mono-baseline".to_string()];
        assert_eq!(
            adjusted.char_width_with_families('X', 10.0, &adjusted_families),
            baseline.char_width_with_families('X', 20.0, &baseline_families),
        );
    }

    #[test]
    fn size_adjust_none_matches_100_percent() {
        // `None` (дескриптор отсутствует/невалиден) должен вести себя как
        // явные `100%`, а не как отдельный, третий режим.
        let font = inter_font();
        let mut none_variant = MultiFontMeasurer::new(&font).unwrap();
        none_variant.register_family_with_overrides(
            "plain", INTER.to_vec(), Vec::new(), None, None, None, None,
        );
        let mut hundred_variant = MultiFontMeasurer::new(&font).unwrap();
        hundred_variant.register_family_with_overrides(
            "plain", INTER.to_vec(), Vec::new(), None, None, Some(1.0), None,
        );
        let families = vec!["plain".to_string()];
        assert_eq!(
            none_variant.ascent_px_with_families(16.0, &families),
            hundred_variant.ascent_px_with_families(16.0, &families),
        );
        assert_eq!(
            none_variant.char_width_with_families('X', 16.0, &families),
            hundred_variant.char_width_with_families('X', 16.0, &families),
        );
    }

    #[test]
    fn size_adjust_composes_with_ascent_override() {
        // font-size-adjust-metrics-override.html (WPT): when both are present,
        // the override percentage applies to the size-adjust-scaled font-size,
        // not the raw one — so ascent-override:100% under size-adjust:150% at
        // font-size 20px must resolve to 30px, not 20px.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family_with_overrides(
            "both",
            INTER.to_vec(),
            Vec::new(),
            Some(1.0), // ascent-override: 100%
            None,
            Some(1.5), // size-adjust: 150%
            None,
        );
        let families = vec!["both".to_string()];
        assert_eq!(m.ascent_px_with_families(20.0, &families), 30.0);
    }

    // ── `line-gap-override` дескриптор (CSS Fonts L4 §14.3, FONTLOAD-13) ───

    #[test]
    fn line_gap_override_replaces_real_metric_with_font_size_fraction() {
        // Та же семантика, что ascent-override: доля font-size, не face-а.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family_with_overrides(
            "overridden",
            INTER.to_vec(),
            Vec::new(),
            None,
            None,
            None,
            Some(1.0), // line-gap-override: 100%
        );
        let families = vec!["overridden".to_string()];
        assert_eq!(m.line_gap_px_with_families(20.0, &families), 20.0);
    }

    #[test]
    fn line_gap_override_none_matches_register_family_with_ranges() {
        // `None` (CSS `normal`/дескриптор отсутствует) не должен ничего
        // менять относительно пути без overrides (FONTLOAD-10 baseline).
        let font = inter_font();
        let mut plain = MultiFontMeasurer::new(&font).unwrap();
        plain.register_family_with_ranges("plain", INTER.to_vec(), Vec::new());
        let mut overridden = MultiFontMeasurer::new(&font).unwrap();
        overridden.register_family_with_overrides(
            "plain", INTER.to_vec(), Vec::new(), None, None, None, None,
        );
        let families = vec!["plain".to_string()];
        assert_eq!(
            plain.line_gap_px_with_families(16.0, &families),
            overridden.line_gap_px_with_families(16.0, &families)
        );
    }

    #[test]
    fn line_gap_override_alone_leaves_ascent_and_descent_at_real_metric() {
        // Асимметричный override: только line-gap задан — ascent/descent
        // остаются реальными метриками face-а, а не тоже подменяются.
        let mono_bytes = std::fs::read(MONO_PATH).expect("bundled JetBrains Mono должен читаться");
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family_with_overrides(
            "mono-line-gap-override",
            mono_bytes.clone(),
            Vec::new(),
            None,
            None,
            None,
            Some(0.5),
        );
        let mut baseline = MultiFontMeasurer::new(&font).unwrap();
        baseline.register_family("mono-baseline", mono_bytes);
        let overridden_families = vec!["mono-line-gap-override".to_string()];
        let baseline_families = vec!["mono-baseline".to_string()];
        assert_eq!(
            m.ascent_px_with_families(16.0, &overridden_families),
            baseline.ascent_px_with_families(16.0, &baseline_families),
        );
        assert_eq!(
            m.descent_px_with_families(16.0, &overridden_families),
            baseline.descent_px_with_families(16.0, &baseline_families),
        );
    }

    #[test]
    fn line_gap_without_override_matches_real_face_metric() {
        // Без дескриптора line_gap_px читает реальную hhea/OS2-метрику face-а,
        // не 0.0 по умолчанию (тот default — только для реализаций
        // `TextMeasurer` без доступа к метрикам, FONTLOAD-13).
        let mono_bytes = std::fs::read(MONO_PATH).expect("bundled JetBrains Mono должен читаться");
        let mono_font = lumen_font::Font::parse(&mono_bytes).expect("валидный sfnt");
        let direct = FontMeasurer::new(&mono_font).expect("метрики JetBrains Mono");
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("mono-plain", mono_bytes.clone());
        let families = vec!["mono-plain".to_string()];
        assert_eq!(m.line_gap_px_with_families(16.0, &families), direct.line_gap_px(16.0));
    }

    #[test]
    fn line_gap_override_composes_with_size_adjust() {
        // Тот же принцип композиции, что `size_adjust_composes_with_ascent_override`:
        // override — доля УЖЕ скорректированного size-adjust'ом font-size.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family_with_overrides(
            "both",
            INTER.to_vec(),
            Vec::new(),
            None,
            None,
            Some(1.5), // size-adjust: 150%
            Some(1.0), // line-gap-override: 100%
        );
        let families = vec!["both".to_string()];
        assert_eq!(m.line_gap_px_with_families(20.0, &families), 30.0);
    }

    // ── resolve_font_stretch (CSS Fonts L4 §5.2) ────────────────────────────

    #[test]
    fn resolve_font_stretch_no_families_returns_none() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        assert_eq!(m.resolve_font_stretch(&[], 100.0), None);
        assert_eq!(m.resolve_font_stretch(&["any".to_string()], 100.0), None);
    }

    #[test]
    fn resolve_font_stretch_non_variable_font_returns_none() {
        // Inter — не variable font, нет fvar/wdth → None
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("inter", INTER.to_vec());
        assert_eq!(m.resolve_font_stretch(&["inter".to_string()], 100.0), None);
    }

    #[test]
    fn resolve_font_stretch_clamps_below_axis_min() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        // wdth ось: [75%, 150%] — ultra-condensed (50%) < min → clamp to 75%
        m.insert_test_wdth_family("varifont", 75.0, 150.0);
        assert_eq!(
            m.resolve_font_stretch(&["varifont".to_string()], 50.0),
            Some(75.0),
            "значение ниже min должно зажиматься к min"
        );
    }

    #[test]
    fn resolve_font_stretch_clamps_above_axis_max() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        // wdth ось: [75%, 150%] — ultra-expanded (200%) > max → clamp to 150%
        m.insert_test_wdth_family("varifont", 75.0, 150.0);
        assert_eq!(
            m.resolve_font_stretch(&["varifont".to_string()], 200.0),
            Some(150.0),
            "значение выше max должно зажиматься к max"
        );
    }

    // ── char_width_varied (CSS Fonts L4 §6.3) ───────────────────────────────

    #[test]
    fn char_width_varied_empty_axes_matches_char_width_with_families() {
        // Empty axes → same result as char_width_with_families (default impl).
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("inter", INTER.to_vec());
        let families = vec!["inter".to_string()];
        let w_normal = m.char_width_with_families('A', 16.0, &families);
        let w_varied = m.char_width_varied('A', 16.0, &[], &families);
        assert!((w_normal - w_varied).abs() < 0.01,
            "пустые axes должны давать тот же результат: {w_normal} vs {w_varied}");
    }

    #[test]
    fn char_width_varied_static_font_ignores_axes() {
        // Inter is a static font (no fvar). Variation axes should be ignored.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("inter", INTER.to_vec());
        let families = vec!["inter".to_string()];
        let axes = vec![lumen_layout::FontVariationSetting { tag: *b"wght", value: 700.0 }];
        let w_normal = m.char_width_with_families('B', 16.0, &families);
        let w_varied = m.char_width_varied('B', 16.0, &axes, &families);
        // Inter has no HVAR — delta is zero, so widths must be equal.
        assert!((w_normal - w_varied).abs() < 0.01,
            "статический шрифт без HVAR: axes не влияют на ширину");
    }

    #[test]
    fn char_width_varied_unknown_family_falls_back_to_inter() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["nonexistent-vf".to_string()];
        let axes = vec![lumen_layout::FontVariationSetting { tag: *b"wght", value: 900.0 }];
        let w = m.char_width_varied('C', 16.0, &axes, &families);
        let w_fallback = m.char_width('C', 16.0);
        assert!((w - w_fallback).abs() < 0.01,
            "неизвестная семья → fallback Inter: {w} vs {w_fallback}");
    }

    // ── unicode-range фильтрация (CSS Fonts L4 §5.1) ────────────────────────

    #[test]
    fn unicode_range_covers_char_uses_registered_font() {
        // Регистрируем Inter только для ASCII (U+0020-007E).
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let ranges = lumen_font::parse_unicode_ranges("U+0020-007E");
        m.register_family_with_ranges("myfont", INTER.to_vec(), ranges);
        // ASCII 'A' (U+0041) покрыт диапазоном → должны получить ширину из Inter.
        let w_family = m.char_width_with_families('A', 16.0, &["myfont".to_string()]);
        let w_fallback = m.char_width('A', 16.0);
        assert!((w_family - w_fallback).abs() < 0.01,
            "символ внутри unicode-range: должна использоваться зарегистрированная семья");
    }

    #[test]
    fn unicode_range_outside_falls_back_to_inter() {
        // Регистрируем Inter только для ASCII (U+0020-007E).
        // Кириллица (U+0410 = А) — вне диапазона → должен быть fallback.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let ranges = lumen_font::parse_unicode_ranges("U+0020-007E");
        m.register_family_with_ranges("myfont", INTER.to_vec(), ranges);
        let families = vec!["myfont".to_string()];
        // Inter содержит кириллицу, поэтому если unicode-range игнорируется,
        // ширины были бы равны. Нас интересует, что слот пропускается —
        // fallback Inter (без unicode-range) даёт тот же результат,
        // поэтому тест просто проверяет, что ширина ненулевая.
        let w = m.char_width_with_families('А', 16.0, &families);
        assert!(w > 0.0, "кириллица вне unicode-range: fallback должен дать ненулевую ширину");
    }

    #[test]
    fn multiple_slots_per_family_unicode_range_selection() {
        // Два слота для одной семьи: первый — ASCII, второй — кириллица.
        // Символ из ASCII → должен выбраться первый слот.
        // Символ из кириллицы → первый слот пропускается, берётся второй.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let latin_ranges = lumen_font::parse_unicode_ranges("U+0020-007E");
        let cyrillic_ranges = lumen_font::parse_unicode_ranges("U+0400-04FF");
        m.register_family_with_ranges("subset", INTER.to_vec(), latin_ranges);
        m.register_family_with_ranges("subset", INTER.to_vec(), cyrillic_ranges);
        // Ровно одна уникальная семья
        assert_eq!(m.family_count(), 1);
        // ASCII 'A' и кирилл. 'А' — оба покрыты через разные слоты
        let w_latin = m.char_width_with_families('A', 16.0, &["subset".to_string()]);
        let w_cyrillic = m.char_width_with_families('А', 16.0, &["subset".to_string()]);
        assert!(w_latin > 0.0, "латиница должна быть покрыта первым слотом");
        assert!(w_cyrillic > 0.0, "кириллица должна быть покрыта вторым слотом");
    }

    #[test]
    fn register_family_with_ranges_empty_ranges_is_unrestricted() {
        // Пустые ranges = нет ограничений — все символы проходят через этот слот.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family_with_ranges("all", INTER.to_vec(), Vec::new());
        let w = m.char_width_with_families('А', 16.0, &["all".to_string()]);
        assert!(w > 0.0, "пустой unicode-range → нет ограничений, кириллица должна работать");
    }
