    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Рантайм с установленным бриджем и одним биндингом.
    ///
    /// `html` парсится как полный под-документ; `accessible=false` моделирует
    /// cross-origin/opaque-sandbox фрейм.
    fn with_frame(html: &str, accessible: bool, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        // Прод-контекст всегда имеет window (WEB_API_SHIM исполняется раньше
        // бриджа); тестовый изолят объявляет его ДО установки, чтобы блок
        // переопределения window.parent/top/… в шиме отработал так же.
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        let doc = lumen_html_parser::parse(html);
        registry.lock().unwrap().frames.push(FrameDocBinding {
            host_nid: 7,
            doc: Arc::new(Mutex::new(doc)),
            url: "about:srcdoc".to_owned(),
            name: None,
            accessible,
            opaque_id: None,
            peer: None,
        });
        f(&rt);
    }

    /// Рантайм с бриджем и пустым реестром — моделирует страницу без
    /// загруженных фреймов.
    fn with_empty_registry(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, registry).unwrap();
        f(&rt);
    }

    /// Контекст фрейма: реестр со слотом родителя (и опционально верха),
    /// моделирующий JS-изолят загруженного `<iframe>`.
    ///
    /// `parent_html` — документ родителя (читается ребёнком при доступном
    /// мосте); `top_html` — отдельный документ верха для глубины 2. При
    /// `top_html == None` слот top не заполняется: у фрейма первого уровня
    /// `top === parent` разрешается через [`PARENT_BID`].
    ///
    /// После заполнения слотов вызывается `_lumen_frame_install_hierarchy` —
    /// тот же шаг, что делает `register_parent_document` в проде (шим
    /// ставит геттеры parent/top/frameElement/name лениво, см. шим).
    fn with_child_context(
        parent_html: &str,
        top_html: Option<&str>,
        host_nid: u32,
        accessible: bool,
        f: impl FnOnce(&V8JsRuntime),
    ) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        {
            let mut reg = registry.lock().unwrap();
            reg.parent = Some(FrameDocBinding {
                host_nid,
                doc: Arc::new(Mutex::new(lumen_html_parser::parse(parent_html))),
                url: "https://parent.example/".to_owned(),
                name: Some("hostframe".to_owned()),
                accessible,
                opaque_id: None,
                peer: None,
            });
            if let Some(top) = top_html {
                reg.top = Some(FrameDocBinding {
                    host_nid: 0,
                    doc: Arc::new(Mutex::new(lumen_html_parser::parse(top))),
                    url: "https://top.example/".to_owned(),
                    name: None,
                    accessible,
                    opaque_id: None,
                    peer: None,
                });
            }
        }
        rt.eval(
            "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
        )
        .unwrap();
        f(&rt);
    }

    fn eval_bool(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    /// OBJECT-1 срез 3: `contentWindow.name` фасада — имя, запомненное при
    /// регистрации фрейма, а не живой атрибут `name` хоста (HTML LS §7.2.3,
    /// WPT `the-object-element/object-attributes.html`).
    #[test]
    fn content_window_name_is_registered_name_not_live_attribute() {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        registry.lock().unwrap().frames.push(FrameDocBinding {
            host_nid: 7,
            doc: Arc::new(Mutex::new(lumen_html_parser::parse("<html><body></body></html>"))),
            url: "about:blank".to_owned(),
            name: Some("o".to_owned()),
            accessible: true,
            opaque_id: None,
            peer: None,
        });
        // Атрибут хоста уже переименован — фасад его не читает.
        rt.eval("__lumen_C._lumen_get_attr = function() { return 'o1'; };").unwrap();
        assert!(eval_bool(&rt, "_lumen_frame_content_window(7).name === 'o'"));
    }

    #[test]
    fn unbound_iframe_gives_null_for_both_getters() {
        with_empty_registry(|rt| {
            assert!(eval_bool(
                rt,
                "_lumen_frame_content_window(7) === null && _lumen_frame_content_document(7) === null"
            ));
        });
    }

    #[test]
    fn content_document_facade_reads_child_tree() {
        with_frame(
            "<html><body><div id='a' class='x'>hello</div></body></html>",
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d !== null && d.body.tagName === 'BODY' && d.body.nodeType === 1"
                ));
                assert!(eval_bool(rt, "_lumen_frame_content_document(7).title === ''"));
                assert!(eval_bool(
                    rt,
                    "var e = _lumen_frame_content_document(7).getElementById('a'); \
                     e.localName === 'div' && e.id === 'a' && e.className === 'x' \
                     && e.textContent === 'hello' && e.getAttribute('class') === 'x'"
                ));
                assert!(eval_bool(
                    rt,
                    "var all = _lumen_frame_content_document(7).querySelectorAll('div'); \
                     all.length === 1 && all[0].id === 'a'"
                ));
                assert!(eval_bool(
                    rt,
                    "_lumen_frame_content_document(7).querySelector('body').tagName === 'BODY' \
                     && _lumen_frame_content_document(7).querySelector('nothing') === null"
                ));
            },
        );
    }

    #[test]
    fn facades_are_interned_and_cross_linked() {
        with_frame("<html><body></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "var w1 = _lumen_frame_content_window(7), w2 = _lumen_frame_content_window(7); \
                 w1 === w2"
            ));
            assert!(eval_bool(
                rt,
                "var d1 = _lumen_frame_content_document(7), d2 = _lumen_frame_content_document(7); \
                 d1 === d2 && w1.document === d1 && d1.defaultView === w1"
            ));
            assert!(eval_bool(
                rt,
                "w1.window === w1 && w1.self === w1 && w1.frames === w1 \
                 && w1.parent === window && w1.top === window && w1.closed === false"
            ));
            assert!(eval_bool(
                rt,
                "var b1 = d1.body, b2 = d1.body; b1 === b2 && b1.parentElement === d1.documentElement"
            ));
        });
    }

    #[test]
    fn inaccessible_frame_hides_document_but_keeps_window() {
        with_frame("<html><body><p>secret</p></body></html>", false, |rt| {
            assert!(eval_bool(rt, "_lumen_frame_content_document(7) === null"));
            assert!(eval_bool(
                rt,
                "var w = _lumen_frame_content_window(7); \
                 w !== null && w.document === null && w.location.href === ''"
            ));
        });
    }

    #[test]
    fn unknown_host_returns_null_without_touching_registry() {
        with_frame("<html><body></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "_lumen_frame_content_window(99) === null && _lumen_frame_content_document(99) === null"
            ));
        });
    }

    #[test]
    fn element_children_and_parent_walk_child_tree_only() {
        with_frame(
            "<html><body><ul><li>one</li><li>two</li></ul></body></html>",
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var ul = _lumen_frame_content_document(7).querySelector('ul'); \
                     ul.children.length === 2 && ul.children[0].textContent === 'one' \
                     && ul.firstElementChild.textContent === 'one' \
                     && ul.lastElementChild.textContent === 'two' \
                     && ul.children[0].parentElement === ul \
                     && ul.parentElement.tagName === 'BODY'"
                ));
            },
        );
    }

    #[test]
    fn geometry_is_zero_until_frame_layout_lands() {
        with_frame("<html><body><p>x</p></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "var b = _lumen_frame_content_document(7).body; \
                 b.offsetWidth === 0 && b.getBoundingClientRect().width === 0"
            ));
        });
    }

    // ── Срез 3: иерархия окон ─────────────────────────────────────────────────

    #[test]
    fn top_level_context_keeps_self_parent_and_top() {
        // Иерархия включена (как после первой регистрации в любом контексте),
        // но слотов предков нет: геттеры обязаны воспроизводить прежние
        // константы WEB_API_SHIM — parent/top = self, frameElement = null,
        // length = число фреймов (0).
        with_empty_registry(|rt| {
            rt.eval(
                "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
            )
            .unwrap();
            assert!(eval_bool(rt, "typeof _lumen_frame_install_hierarchy === 'function'"));
            assert!(eval_bool(rt, "window.parent === window"), "parent fallback");
            assert!(eval_bool(rt, "window.top === window"), "top fallback");
            assert!(eval_bool(rt, "window.frameElement === null"), "frameElement fallback");
            // length ставит только регистрация фрейма (_lumen_frame_install_index):
            // здесь фреймов нет — геттера ещё нет, в проде статический 0 из
            // WEB_API_SHIM, в минимальном изоляте undefined.
            assert!(eval_bool(rt, "window.length === undefined"), "length not yet installed");
        });
    }

    #[test]
    fn child_window_parent_reads_parent_document() {
        with_child_context(
            "<html><body><div id='p'>parent</div></body></html>",
            None,
            42,
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "window.parent !== window \
                     && window.top === window.parent \
                     && window.parent.document !== null \
                     && window.parent.document.getElementById('p').textContent === 'parent' \
                     && window.parent.location.href === 'https://parent.example/'"
                ));
            },
        );
    }

    #[test]
    fn child_parent_facade_is_interned_and_self_referential() {
        with_child_context("<html><body></body></html>", None, 42, true, |rt| {
            assert!(eval_bool(
                rt,
                "var p1 = window.parent, p2 = window.parent; \
                 p1 === p2 && p1.parent === p1 && p1.top === p1 \
                 && p1.self === p1 && p1.window === p1"
            ));
        });
    }

    #[test]
    fn grandchild_top_resolves_root_not_direct_parent() {
        // Слот parent указывает на документ промежуточного фрейма, top — на
        // корень: window.top должен вести в корень, отличаясь от parent.
        with_child_context(
            "<html><body><b>mid</b></body></html>",
            Some("<html><body><i>root</i></body></html>"),
            9,
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "window.parent !== window && window.top !== window.parent \
                     && window.top.document.querySelector('i').textContent === 'root' \
                     && window.parent.document.querySelector('b').textContent === 'mid' \
                     && window.top.parent === window.top"
                ));
            },
        );
    }

    /// Предки внука сходятся к top: `parent.parent === top`, `parent.top ===
    /// top`. Раньше фасад родителя был сам себе parent, и цикл поиска
    /// TCF-локатора из `ads.pubmatic.com/AdServer/js/user_sync.html` вешал
    /// поток движка навсегда.
    #[test]
    fn grandchild_ancestor_chain_reaches_top() {
        with_child_context(
            "<html><body><b>mid</b></body></html>",
            Some("<html><body><i>root</i></body></html>"),
            9,
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "window.parent.parent === window.top && window.parent.top === window.top"
                ));
                assert!(eval_bool(
                    rt,
                    "var ba = window, steps = 0; \
                     while (steps < 10) { if (ba === window.top) break; ba = ba.parent; steps++; } \
                     ba === window.top && steps === 2"
                ));
            },
        );
    }

    /// BUG-921: `window.name` — свойство СВОЕГО контекста, снятое снимком при
    /// его создании, а не чтение документа родителя — cross-origin (`accessible
    /// = false`) не прячет его, в отличие от `.document`/`frameElement`.
    #[test]
    fn cross_origin_child_gets_window_but_no_documents() {
        with_child_context(
            "<html><body><div>secret</div></body></html>",
            None,
            42,
            false,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var p = window.parent; \
                     p !== null && typeof p === 'object' \
                     && p.document === null \
                     && window.top.document === null \
                     && window.frameElement === null \
                     && window.name === 'hostframe'"
                ));
            },
        );
    }

    #[test]
    fn child_frame_element_is_host_facade_from_parent_tree() {
        // host_nid = 4 — индекс <iframe> в дереве родителя
        // [root, html, head, body, iframe]; нативы читают nid без проверок.
        with_child_context(
            "<html><body><iframe id='host' name='hostframe'></iframe></body></html>",
            None,
            4,
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var fe = window.frameElement; \
                     fe !== null && fe.localName === 'iframe' && fe.id === 'host' \
                     && fe.getAttribute('name') === 'hostframe'"
                ));
                assert!(eval_bool(rt, "window.name === 'hostframe'"));
            },
        );
    }

    /// BUG-921: правка атрибута `name` хоста ПОСЛЕ создания контекста не
    /// переименовывает уже созданный `window.name` (HTML LS §7.2.3 — атрибут
    /// используется только в момент создания вложенного navigable).
    #[test]
    fn window_name_survives_host_attribute_mutation_after_creation() {
        with_child_context(
            "<html><body><iframe id='host' name='hostframe'></iframe></body></html>",
            None,
            4,
            true,
            |rt| {
                assert!(eval_bool(rt, "window.name === 'hostframe'"));
                assert!(eval_bool(
                    rt,
                    "var p = _lumen_parent_binding(); \
                     _lumen_f_set_attr(p, _lumen_f_host(p), 'name', 'meh')"
                ));
                assert!(
                    eval_bool(rt, "window.name === 'hostframe'"),
                    "правка атрибута задним числом не должна переименовывать контекст"
                );
                assert!(eval_bool(
                    rt,
                    "var p = _lumen_parent_binding(); \
                     _lumen_f_remove_attr(p, _lumen_f_host(p), 'name')"
                ));
                assert!(
                    eval_bool(rt, "window.name === 'hostframe'"),
                    "удаление атрибута задним числом не должно переименовывать контекст"
                );
            },
        );
    }

    /// GAP-NAVCTX срез 8: `contentWindow.location.href = url` (здесь —
    /// `window.parent.location` из ребёнка, тот же фасад, что и
    /// `fr.contentWindow` из родителя) больше не молча теряется на
    /// одноразовом литерале — пишет `src` хоста в дереве родителя тем же
    /// нативом (`_lumen_f_set_attr`), каким уже пишет `iframe.src=` (срез
    /// 6/7), так что `poll_dynamic_frames` подхватывает её на следующем
    /// тике без отдельного пути навигации.
    ///
    /// Реестр строится вручную (не через `with_child_context`), чтобы
    /// получить `Arc` документа родителя и слить каждую мутацию через
    /// [`take_frame_dom_dirty`] сразу же — иначе висящий флаг в глобальном
    /// реестр (ключ — адрес `Arc`, срез 25) переживает эту функцию и
    /// после освобождения памяти может ложно сработать на чужом документе,
    /// которому аллокатор отдаст тот же адрес (гонка с параллельными
    /// тестами, ровно то, от чего `bridge_mutation_marks_frame_dirty_and_drains_once`
    /// дренирует свой флаг перед возвратом).
    #[test]
    fn parent_facade_location_setter_writes_host_src_attribute() {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        let parent_doc = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<html><body><iframe id='host' name='hostframe'></iframe></body></html>",
        )));
        registry.lock().unwrap().parent = Some(FrameDocBinding {
            host_nid: 4,
            doc: Arc::clone(&parent_doc),
            url: "https://parent.example/".to_owned(),
            name: Some("hostframe".to_owned()),
            accessible: true,
            opaque_id: None,
            peer: None,
        });
        rt.eval(
            "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
        )
        .unwrap();
        let key = Arc::as_ptr(&parent_doc) as usize;

        assert!(eval_bool(
            &rt,
            "window.parent.location.href = 'https://parent.example/via-href.html'; \
             window.frameElement.getAttribute('src') === 'https://parent.example/via-href.html'"
        ));
        assert!(take_frame_dom_dirty(key));

        assert!(eval_bool(
            &rt,
            "window.parent.location.assign('https://parent.example/via-assign.html'); \
             window.frameElement.getAttribute('src') === 'https://parent.example/via-assign.html'"
        ));
        assert!(take_frame_dom_dirty(key));

        assert!(eval_bool(
            &rt,
            "window.parent.location.replace('https://parent.example/via-replace.html'); \
             window.frameElement.getAttribute('src') === 'https://parent.example/via-replace.html'"
        ));
        assert!(take_frame_dom_dirty(key));

        assert!(eval_bool(
            &rt,
            "window.parent.location = 'https://parent.example/via-assign-whole.html'; \
             window.frameElement.getAttribute('src') === 'https://parent.example/via-assign-whole.html'"
        ));
        assert!(take_frame_dom_dirty(key));
    }

    /// GAP-NAVCTX срез 9: срез 8 писал URL дословно в `src` хоста, и относительный
    /// URL резолвился против базы документа-ВЛАДЕЛЬЦА хоста (`parent.example`) —
    /// расходится со спекой (навигация резолвится против базы НАВИГИРУЮЩЕГО,
    /// т.е. читающего фасад, документа). Стаб `_lumen_document_base_url`
    /// изображает базу читателя, заведомо отличную от базы хоста, чтобы отличить
    /// "резолвим против читателя" (правильно) от "резолвим против хоста" (срез 8).
    #[test]
    fn parent_facade_location_setter_resolves_relative_url_against_reader_base() {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        let parent_doc = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<html><body><iframe id='host' name='hostframe'></iframe></body></html>",
        )));
        registry.lock().unwrap().parent = Some(FrameDocBinding {
            host_nid: 4,
            doc: Arc::clone(&parent_doc),
            url: "https://parent.example/".to_owned(),
            name: Some("hostframe".to_owned()),
            accessible: true,
            opaque_id: None,
            peer: None,
        });
        rt.eval(
            "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
        )
        .unwrap();
        let key = Arc::as_ptr(&parent_doc) as usize;

        rt.eval(
            "__lumen_C._lumen_document_base_url = function() { return 'https://reader.example/sub/'; }; \
             globalThis._url_resolve = function(rel, base) { \
                 if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(rel)) return rel; \
                 return base + rel; \
             };",
        )
        .unwrap();

        assert!(eval_bool(
            &rt,
            "window.parent.location.href = 'relative.html'; \
             window.frameElement.getAttribute('src') === 'https://reader.example/sub/relative.html'"
        ));
        assert!(take_frame_dom_dirty(key));
    }

    #[test]
    fn frame_length_and_accessors_track_registrations() {
        // Регистрация двух фреймов + постановка доступников тем же шагом,
        // что делает register_frame_document в проде (push + install_index).
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        for (i, (nid, name)) in [(0u32, (11u32, None)), (1u32, (12u32, Some("second".to_owned())))] {
            registry.lock().unwrap().frames.push(FrameDocBinding {
                host_nid: nid,
                doc: Arc::new(Mutex::new(lumen_html_parser::parse(
                    "<html><body><p>x</p></body></html>",
                ))),
                url: "about:blank".to_owned(),
                name,
                accessible: true,
                opaque_id: None,
                peer: None,
            });
            rt.eval(&format!("_lumen_frame_install_index({i})")).unwrap();
        }
        assert!(eval_bool(&rt, "window.length === 2"));
        assert!(eval_bool(
            &rt,
            "window[0] !== null && window[0] !== window \
             && window[0].document.body.tagName === 'BODY'"
        ));
        assert!(eval_bool(
            &rt,
            "window.second !== undefined && window.second === window[1] \
             && window[0] === window[0]"
        ));
        assert!(eval_bool(&rt, "window[5] === undefined"));
    }

    // ── Срез 4: кросс-фреймовый postMessage ───────────────────────────────────

    /// Пара изолятов «родитель ↔ ребёнок» с общим документом — та же топология,
    /// что строит shell (срезы 1–3): Arc документа ребёнка один и тот же в
    /// реестре родителя и в self_key ребёнка, и наоборот для родителя.
    ///
    /// Вместо WEB_API_SHIM (в минимальном изоляте его нет) каждый контекст
    /// получает мини-хуки приёма, складывающие доставки в `__msgs` (срез 4),
    /// клики в `__clicks` (срез 6) и фокус/dom-события в `__acts` (срез 7).
    fn with_parent_child_pair(
        parent_accessible_to_child: bool,
        child_accessible_to_parent: bool,
    ) -> (V8JsRuntime, V8JsRuntime) {
        let (rt_parent, rt_child, _parent_doc, _child_doc) =
            with_parent_child_pair_docs(parent_accessible_to_child, child_accessible_to_parent);
        (rt_parent, rt_child)
    }

    /// Вариант [`with_parent_child_pair`], дополнительно отдающий оба общих
    /// `Arc<Mutex<Document>>` — для Rust-стороны ожидаемых nid (срез 6).
    fn with_parent_child_pair_docs(
        parent_accessible_to_child: bool,
        child_accessible_to_parent: bool,
    ) -> (
        V8JsRuntime,
        V8JsRuntime,
        Arc<Mutex<lumen_dom::Document>>,
        Arc<Mutex<lumen_dom::Document>>,
    ) {
        let parent_doc = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<html><body><div id='p'>parent</div></body></html>",
        )));
        let child_doc = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<html><body><b>child</b></body></html>",
        )));

        let rt_parent = V8JsRuntime::new().unwrap();
        let reg_p: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt_parent.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt_parent, Arc::clone(&reg_p)).unwrap();
        {
            let mut p = reg_p.lock().unwrap();
            p.self_key = Some(Arc::as_ptr(&parent_doc) as usize);
            p.self_origin = "https://parent.example".to_owned();
            p.frames.push(FrameDocBinding {
                host_nid: 7,
                doc: Arc::clone(&child_doc),
                url: "about:srcdoc".to_owned(),
                name: None,
                accessible: child_accessible_to_parent,
                opaque_id: None,
                peer: None,
            });
        }
        rt_parent
            .eval("typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()")
            .unwrap();

        let rt_child = V8JsRuntime::new().unwrap();
        let reg_c: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt_child.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt_child, Arc::clone(&reg_c)).unwrap();
        {
            let mut c = reg_c.lock().unwrap();
            c.self_key = Some(Arc::as_ptr(&child_doc) as usize);
            c.self_origin = "about:srcdoc".to_owned();
            c.parent = Some(FrameDocBinding {
                host_nid: 7,
                doc: Arc::clone(&parent_doc),
                url: "https://parent.example/".to_owned(),
                name: Some("hostframe".to_owned()),
                accessible: parent_accessible_to_child,
                opaque_id: None,
                peer: None,
            });
        }
        rt_child
            .eval("typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()")
            .unwrap();

        for rt in [&rt_parent, &rt_child] {
            rt.eval(
                "globalThis.__msgs = []; \
                 globalThis.__clicks = []; \
                 globalThis.__acts = []; \
                 __lumen_C._lumen_deliver_frame_message = function(d, o, s) { \
                     __msgs.push({ d: d, o: o, s: s }); \
                 }; \
                 __lumen_C._lumen_deliver_frame_click = function(nid) { \
                     __clicks.push(nid); \
                 }; \
                 __lumen_C._lumen_deliver_frame_focus = function(nid, ps) { \
                     __acts.push(['focus', nid, !!ps]); \
                 }; \
                 __lumen_C._lumen_deliver_frame_blur = function(nid) { \
                     __acts.push(['blur', nid]); \
                 }; \
                 __lumen_C._lumen_deliver_frame_dom_event = function(nid, env) { \
                     __acts.push(['dom', nid, env.type, !!env.bubbles, \
                                  (env.detail === undefined) ? null : env.detail]); \
                 };",
            )
            .unwrap();
        }
        (rt_parent, rt_child, parent_doc, child_doc)
    }

    fn msg_count(rt: &V8JsRuntime) -> usize {
        match rt.eval("__msgs.length") {
            Ok(JsValue::Number(n)) => n as usize,
            _ => 0,
        }
    }

    #[test]
    fn post_message_from_child_reaches_parent_with_inherited_origin() {
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        // Ребёнок постит через настоящий фасад window.parent.
        rt_child
            .eval("window.parent.postMessage({a: 1, b: ['x', 2]}, '*')")
            .unwrap();
        assert_eq!(msg_count(&rt_parent), 0, "до пумпы ничего не доставлено");
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent), 1);
        // origin srcdoc-ребёнка наследуется от родителя-получателя;
        // source — интернированный фасад окна ребёнка.
        assert!(eval_bool(
            &rt_parent,
            "__msgs[0].d.a === 1 && __msgs[0].d.b.length === 2 && __msgs[0].d.b[1] === 2 \
             && __msgs[0].o === 'https://parent.example' \
             && __msgs[0].s !== null && __msgs[0].s === _lumen_frame_content_window(7)"
        ));
        // Повторная пумпа ничего не добавляет — ящик разобран.
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent), 1);
    }

    #[test]
    fn post_message_from_parent_reaches_child() {
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_parent
            .eval("_lumen_frame_content_window(7).postMessage('hello', '*')")
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_child), 1);
        assert!(eval_bool(
            &rt_child,
            "__msgs[0].d === 'hello' \
             && __msgs[0].o === 'https://parent.example' \
             && __msgs[0].s === window.parent"
        ));
    }

    #[test]
    fn top_level_undefined_becomes_null_like_json() {
        // Отклонение от structured clone, задокументированное в баг-файле:
        // JSON не переносит верхнеуровневый undefined — уходит null.
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_parent
            .eval("_lumen_frame_content_window(7).postMessage(undefined, '*')")
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(eval_bool(&rt_child, "__msgs[0].d === null"));
    }

    #[test]
    fn explicit_origin_mismatch_is_silently_dropped() {
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_child
            .eval("window.parent.postMessage('x', 'https://other.example')")
            .unwrap();
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent), 0);
    }

    #[test]
    fn slash_target_origin_follows_accessibility_flag() {
        // same-origin пара: '/' доставляет…
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_child
            .eval("window.parent.postMessage('same', '/')")
            .unwrap();
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent), 1);
        drop((rt_parent, rt_child));

        // …cross-origin пара (accessible=false у слота родителя): '/' режется.
        let (rt_parent2, rt_child2) = with_parent_child_pair(false, true);
        rt_child2
            .eval("window.parent.postMessage('cross', '/')")
            .unwrap();
        rt_parent2.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent2), 0);
    }

    #[test]
    fn unbound_bid_posts_nothing() {
        let (_rt_parent, rt_child) = with_parent_child_pair(true, true);
        assert!(eval_bool(
            &rt_child,
            "_lumen_f_post_message(99, '1', '*') === false"
        ));
    }

    #[test]
    fn functions_throw_data_clone_error() {
        let (_rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_child
            .eval("globalThis.DOMException = function(m, n) { this.message = m; this.name = n; };")
            .unwrap();
        assert!(eval_bool(
            &rt_child,
            "(function() { \
                 try { window.parent.postMessage(function(){}, '*'); return false; } \
                 catch (e) { return e.name === 'DataCloneError'; } \
             })()"
        ));
    }

    #[test]
    fn foreign_messages_survive_until_their_receiver_pumps() {
        // Сообщение адресовано родителю: разбор ящика ребёнком его не трогает.
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_child
            .eval("window.parent.postMessage('keep', '*')")
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_child), 0);
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(eval_bool(&rt_parent, "__msgs[0].d === 'keep'"));
    }

    // ── Срез 5: мутации под-документа из родителя ─────────────────────────────

    /// Рантайм + реестр + общий `Arc` документа биндинга `frames[0]` —
    /// для проверок мутаций и со стороны JS, и со стороны Rust.
    fn with_shared_frame(
        html: &str,
        accessible: bool,
        f: impl FnOnce(&V8JsRuntime, &Arc<Mutex<lumen_dom::Document>>),
    ) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        let doc = Arc::new(Mutex::new(lumen_html_parser::parse(html)));
        registry.lock().unwrap().frames.push(FrameDocBinding {
            host_nid: 7,
            doc: Arc::clone(&doc),
            url: "about:srcdoc".to_owned(),
            name: None,
            accessible,
            opaque_id: None,
            peer: None,
        });
        f(&rt, &doc);
    }

    /// Второй изолят над ТЕМ ЖЕ `Arc<Mutex<Document>>` — модель «контекст
    /// ребёнка видит мутации родителя»: у shell документ фрейма один и тот же
    /// инстанс в реестре родителя и в контексте ребёнка (срез 1).
    fn sibling_runtime_over(doc: &Arc<Mutex<lumen_dom::Document>>) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        registry.lock().unwrap().frames.push(FrameDocBinding {
            host_nid: 7,
            doc: Arc::clone(doc),
            url: "about:srcdoc".to_owned(),
            name: None,
            accessible: true,
            opaque_id: None,
            peer: None,
        });
        rt
    }

    #[test]
    fn mutations_land_in_the_real_child_tree() {
        with_shared_frame(
            "<html><body><p>keep</p></body></html>",
            true,
            |rt, doc| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     var p = d.createElement('p'); \
                     p.id = 'injected'; \
                     p.setAttribute('data-from', 'parent'); \
                     p.textContent = 'from-parent'; \
                     d.body.appendChild(p) === p && \
                     d.getElementById('injected').textContent === 'from-parent' && \
                     d.getElementById('injected').getAttribute('data-from') === 'parent'"
                ));
                // Мутация попала в настоящее дерево, а не в копию фасада.
                let injected = {
                    let d = doc.lock().unwrap();
                    lumen_dom::Document::find_by_id(&d, "injected")
                };
                let injected = injected.expect("узел должен существовать в общем дереве");
                let d = doc.lock().unwrap();
                assert_eq!(
                    d.get(injected).get_attr("data-from"),
                    Some("parent"),
                    "атрибут выставлен нативом в общий Document"
                );
                let body = d.body().expect("body на месте");
                assert_eq!(d.get(body).children.len(), 2, "p добавлен последним ребёнком body");
            },
        );
    }

    #[test]
    fn mutations_are_visible_to_another_isolate_sharing_the_arc() {
        let shared = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<html><body><b>old</b></body></html>",
        )));
        let writer = sibling_runtime_over(&shared);
        let reader = sibling_runtime_over(&shared);
        assert!(eval_bool(
            &writer,
            "var d = _lumen_frame_content_document(7); \
             var s = d.createElement('span'); s.textContent = 'fresh'; \
             d.body.insertBefore(s, d.body.firstElementChild) !== undefined"
        ));
        assert!(eval_bool(
            &reader,
            "var b = _lumen_frame_content_document(7).querySelector('body'); \
             b.children.length === 2 && b.children[0].textContent === 'fresh' \
             && b.children[0].tagName === 'SPAN'"
        ));
    }

    #[test]
    fn text_content_setter_replaces_children_of_element() {
        with_shared_frame(
            "<html><body><div id='a'><b>one</b>two<i>three</i></div></body></html>",
            true,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var a = _lumen_frame_content_document(7).getElementById('a'); \
                     a.textContent = 'flat'; \
                     a.children.length === 0 && a.textContent === 'flat'"
                ));
            },
        );
    }

    #[test]
    fn remove_and_insert_before_reorder_children() {
        with_shared_frame(
            "<html><body><ul id='u'><li id='x'>x</li><li id='z'>z</li></ul></body></html>",
            true,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     var y = d.createElement('li'); y.id = 'y'; y.textContent = 'y'; \
                     d.getElementById('u').insertBefore(y, d.getElementById('z')) !== undefined && \
                     d.getElementById('u').children[1].id === 'y'"
                ));
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d.getElementById('y').remove(); \
                     var ids = d.getElementById('u').children.map(function(c) { return c.id; }); \
                     ids.join(',') === 'x,z'"
                ));
            },
        );
    }

    #[test]
    fn remove_child_takes_node_off_its_actual_parent() {
        with_shared_frame(
            "<html><body><div id='box'><b id='inner'>t</b></div></body></html>",
            true,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     var inner = d.getElementById('inner'); \
                     d.getElementById('box').removeChild(inner) === inner && \
                     d.getElementById('box').children.length === 0 && \
                     inner.parentElement === null"
                ));
            },
        );
    }

    /// BUG-480 срез 19: навигация фрейма ЗАМЕЩАЕТ биндинг того же хоста, а не
    /// добавляет второй, — и `bid` при этом сохраняется.
    ///
    /// Проверяется тем, что видно из JS, а не длиной вектора: поиск по
    /// `host_nid` берёт первое совпадение, поэтому лишний биндинг проявился бы
    /// как «`contentDocument` вечно отдаёт выброшенный документ» — ровно то, на
    /// чём поймала живая проба.
    #[test]
    fn navigating_a_frame_replaces_its_binding_in_place() {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        let make = |html: &str, url: &str| FrameDocBinding {
            host_nid: 7,
            doc: Arc::new(Mutex::new(lumen_html_parser::parse(html))),
            url: url.to_owned(),
            name: None,
            accessible: true,
            opaque_id: None,
            peer: None,
        };
        let first = upsert_binding(
            &mut registry.lock().unwrap(),
            make("<html><head><title>one</title></head><body></body></html>", "https://e/1"),
        );
        assert!(eval_bool(&rt, "_lumen_frame_content_document(7).title === 'one'"));
        let second = upsert_binding(
            &mut registry.lock().unwrap(),
            make("<html><head><title>two</title></head><body></body></html>", "https://e/2"),
        );
        assert_eq!((first, second), (0, 0), "bid вложенного контекста обязан пережить навигацию");
        assert_eq!(registry.lock().unwrap().frames.len(), 1, "второго биндинга того же хоста нет");
        assert!(eval_bool(&rt, "_lumen_frame_content_document(7).title === 'two'"));
        assert!(eval_bool(&rt, "_lumen_frame_content_window(7).location.href === 'https://e/2'"));
        // Чужой хост по-прежнему добавляется отдельной записью.
        let mut other = make("<html><body></body></html>", "https://e/x");
        other.host_nid = 8;
        assert_eq!(upsert_binding(&mut registry.lock().unwrap(), other), 1);
    }

    #[test]
    fn cross_binding_facade_argument_is_silently_ignored() {
        // Два биндинга в одном изоляте (разные host_nid): фасад документа
        // первого не принимает узел второго — проверка __bid__ режет аргумент
        // до натива. Владение nid'ом на нативном уровне обеспечивает именно
        // эта JS-граница: нативы модуля приватны и доверяют своим фасадам,
        // как нативы главного документа доверяют его врапперам.
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        for (host, html) in [
            (7u32, "<html><body><p id='first'>a</p></body></html>"),
            (8u32, "<html><body><p id='second'>b</p></body></html>"),
        ] {
            registry.lock().unwrap().frames.push(FrameDocBinding {
                host_nid: host,
                doc: Arc::new(Mutex::new(lumen_html_parser::parse(html))),
                url: "about:srcdoc".to_owned(),
                name: None,
                accessible: true,
                opaque_id: None,
                peer: None,
            });
        }
        assert!(eval_bool(
            &rt,
            "var d0 = _lumen_frame_content_document(7); \
             var d1 = _lumen_frame_content_document(8); \
             var alien = d1.createElement('div'); \
             alien.__bid__ !== d0.body.__bid__ && \
             d0.body.appendChild(alien) === undefined && \
             d0.body.children.length === 1 && \
             d1.body.children.length === 1"
        ));
        // Чужой фасад и в removeChild/insertBefore игнорируется.
        assert!(eval_bool(
            &rt,
            "var d0 = _lumen_frame_content_document(7); \
             var d1 = _lumen_frame_content_document(8); \
             d0.body.removeChild(d1.createElement('span')) === undefined && \
             d0.body.insertBefore(d1.createElement('i'), null) === undefined && \
             d0.getElementById('first').textContent === 'a'"
        ));
    }

    #[test]
    fn inaccessible_frame_mutations_are_no_ops() {
        with_shared_frame(
            "<html><body><p>x</p></body></html>",
            false,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d === null && \
                     _lumen_f_create_element(7, 'div') === -1 && \
                     _lumen_f_append_child(7, 3, 4) === false && \
                     _lumen_f_set_attr(7, 3, 'id', 'hax') === false"
                ));
            },
        );
    }

    // ── Срез 25: relayout ребёнка после мутации через мост ──────────────────

    #[test]
    fn bridge_mutation_marks_frame_dirty_and_drains_once() {
        with_shared_frame(
            "<html><body><p id='p'>x</p></body></html>",
            true,
            |rt, doc| {
                let key = Arc::as_ptr(doc) as usize;
                // Ничего не менялось — флага нет.
                assert!(!take_frame_dom_dirty(key));
                assert!(eval_bool(
                    rt,
                    "var el = _lumen_frame_content_document(7).getElementById('p'); \
                     el.setAttribute('data-x', '1'); \
                     el.getAttribute('data-x') === '1'"
                ));
                // Забирается ровно один раз — второй запрос без новой мутации пуст.
                assert!(take_frame_dom_dirty(key));
                assert!(!take_frame_dom_dirty(key));
            },
        );
    }

    /// BUG-1110: флаг умершего документа не достаётся новому документу,
    /// которому аллокатор выдал бы тот же адрес.
    #[test]
    fn dirty_flag_of_dropped_document_is_not_inherited() {
        let dead = Arc::new(Mutex::new(lumen_html_parser::parse("<p>a</p>")));
        let key = Arc::as_ptr(&dead) as usize;
        mark_frame_dom_dirty(&dead);
        drop(dead);
        assert!(!take_frame_dom_dirty(key));
        let live = Arc::new(Mutex::new(lumen_html_parser::parse("<p>b</p>")));
        let live_key = Arc::as_ptr(&live) as usize;
        mark_frame_dom_dirty(&live);
        assert!(take_frame_dom_dirty(live_key));
    }

    #[test]
    fn inaccessible_bridge_mutation_does_not_mark_dirty() {
        with_shared_frame("<html><body><p>x</p></body></html>", false, |_rt, doc| {
            let key = Arc::as_ptr(doc) as usize;
            // Натив выше уже проверил, что запись сюда — no-op; здесь — что
            // no-op не поднимает флаг, который заставил бы шелл впустую
            // пересчитать фрейм.
            assert!(!take_frame_dom_dirty(key));
        });
    }

    #[test]
    fn bridge_read_does_not_mark_dirty() {
        with_shared_frame("<html><body><p id='p'>x</p></body></html>", true, |rt, doc| {
            let key = Arc::as_ptr(doc) as usize;
            assert!(eval_bool(
                rt,
                "_lumen_frame_content_document(7).getElementById('p').textContent === 'x'"
            ));
            assert!(!take_frame_dom_dirty(key));
        });
    }

    #[test]
    fn cycle_append_is_rejected_without_corrupting_tree() {
        with_shared_frame(
            "<html><body><div id='deep'>t</div></body></html>",
            true,
            |rt, _| {
                // documentElement нельзя вставить под собственного потомка.
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d.body.appendChild(d.documentElement) === undefined && \
                     d.documentElement.parentElement === null"
                ));
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d.getElementById('deep').textContent === 't' && \
                     d.body.children.length === 1"
                ));
            },
        );
    }

    #[test]
    fn f_children_native_binding_panic_does_not_abort_process() {
        // BUG-418: an invalid NodeId reaching `Document::get`/`get_mut` used
        // to panic inside V8's `extern "C"` callback boundary, which Rust
        // refuses to unwind through, aborting the whole process.
        // `native_fn_trampoline` now wraps native dispatch in `catch_unwind`
        // (same trampoline `register_native` uses for `_lumen_f_*`, not just
        // the `reg!`-registered natives) — this test proves the guard still
        // holds for this registration path too.
        //
        // BUG-1031: every direct-from-JS `doc.get(nid)` in `dom_core.rs`/
        // `platform.rs` has since been bounds-checked (BUG-986/BUG-1024/
        // BUG-1030's chain), so the panic input moved here — `_lumen_f_children`
        // is one of this file's cross-frame read natives, which `checked_node`'s
        // doc comment above documents as deliberately still trusting the raw
        // facade `nid` for reads (only writes reject a foreign index).
        with_frame("<html><body></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "var caught = ''; \
                 try { _lumen_f_children(0, 4294967295); } \
                 catch (e) { caught = e.name; } \
                 caught === 'Error'"
            ));
        });
    }

    #[test]
    fn title_setter_creates_title_inside_head() {
        with_shared_frame("<html><head></head><body>x</body></html>", true, |rt, _| {
            assert!(eval_bool(
                rt,
                "var d = _lumen_frame_content_document(7); \
                 d.title === '' && (d.title = 'framed') || true; \
                 d.title === 'framed'"
            ));
        });
        // Повторная запись идёт в существующий <title>.
        with_shared_frame(
            "<html><head><title>old</title></head><body></body></html>",
            true,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d.title === 'old' && ((d.title = 'new') || true) && d.title === 'new'"
                ));
            },
        );
    }

    #[test]
    fn created_text_node_round_trips_through_facade() {
        with_shared_frame("<html><body></body></html>", true, |rt, _| {
            assert!(eval_bool(
                rt,
                "var d = _lumen_frame_content_document(7); \
                 var t = d.createTextNode('plain'); \
                 t.nodeType === 3 && t.textContent === 'plain' && \
                 d.body.appendChild(t) === t && \
                 d.body.textContent === 'plain'"
            ));
        });
    }

    // ── Срез 6: события через границу изолятов ────────────────────────────────

    /// Индекс первого элемента с тегом `tag` в дереве `doc`.
    fn element_index(doc: &lumen_dom::Document, tag: &str) -> Option<u32> {
        find_first_matching(doc, doc.root(), &|node| {
            node.element_name()
                .map(|n| n.local.eq_ignore_ascii_case(tag))
                .unwrap_or(false)
        })
        .map(|n| n.raw())
    }

    fn clicks(rt: &V8JsRuntime) -> Vec<u32> {
        match rt.eval("JSON.stringify(__clicks)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Доставки среза 7: `[вид, nid, ...аргументы]` на каждый конверт.
    fn acts(rt: &V8JsRuntime) -> Vec<serde_json::Value> {
        match rt.eval("JSON.stringify(__acts)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn facade_click_from_parent_delivers_on_child_pump() {
        let (rt_parent, rt_child, _pd, cd) = with_parent_child_pair_docs(true, true);
        rt_parent
            .eval(
                "var b = _lumen_frame_content_document(7).querySelector('b'); \
                 globalThis.__bnid = b.__nid__; \
                 b.click(); b.click();",
            )
            .unwrap();
        // До пумпы доставки нет — конверты лежат в ящике.
        assert!(clicks(&rt_child).is_empty(), "до пумпы ничего не доставлено");
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let expected = {
            let d = cd.lock().unwrap();
            element_index(&d, "b").expect("<b> в дереве ребёнка")
        };
        // nid фасада — индекс в арене ребёнка; оба клика доставлены по адресу.
        let got = clicks(&rt_child);
        assert_eq!(got, vec![expected, expected]);
        match rt_parent.eval("__bnid") {
            Ok(JsValue::Number(n)) => assert_eq!(n as u32, expected),
            other => panic!("__bnid не число: {other:?}"),
        }
        // Ящик разобран: повторная пумпа ничего не добавляет.
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(clicks(&rt_child).len(), 2);
    }

    #[test]
    fn child_facade_click_waits_for_the_parent_pump() {
        let (rt_parent, rt_child, pd, _cd) = with_parent_child_pair_docs(true, true);
        rt_child
            .eval("window.parent.document.body.click();")
            .unwrap();
        // Пумпа ребёнка не трогает чужой ящик — конверт адресован родителю.
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(clicks(&rt_child).is_empty());
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        let expected = {
            let d = pd.lock().unwrap();
            element_index(&d, "body").expect("body в дереве родителя")
        };
        assert_eq!(clicks(&rt_parent), vec![expected]);
    }

    #[test]
    fn click_into_inaccessible_child_is_dropped() {
        // Второй флаг false: слот frames родителя cross-origin/opaque — фасад
        // документа не выдаётся и натив постановки режет конверт до ящика.
        let (rt_parent, rt_child, _pd, cd) = with_parent_child_pair_docs(true, false);
        let b_nid = {
            let d = cd.lock().unwrap();
            element_index(&d, "b").expect("<b> в дереве ребёнка")
        };
        assert!(eval_bool(
            &rt_parent,
            &format!(
                "_lumen_frame_content_document(7) === null && \
                 _lumen_f_queue_event(0, {b_nid}, '{{\"kind\":\"click\"}}') === false"
            )
        ));
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(clicks(&rt_child).is_empty());
    }

    #[test]
    fn click_queue_rejects_non_elements_and_bad_args() {
        with_shared_frame("<html><body><b>x</b></body></html>", true, |rt, doc| {
            let text_nid = {
                let d = doc.lock().unwrap();
                let b = find_first_matching(&d, d.root(), &|node| {
                    node.element_name()
                        .map(|n| n.local.eq_ignore_ascii_case("b"))
                        .unwrap_or(false)
                })
                .expect("<b> существует");
                d.get(b).children[0].raw()
            };
            // Текстовый узел, nid за границей арены и несуществующий bid —
            // все три дают «нет» и ничего не ставят в ящик.
            assert!(eval_bool(
                rt,
                &format!(
                    "_lumen_f_queue_event(0, {text_nid}, '{{\"kind\":\"click\"}}') === false && \
                     _lumen_f_queue_event(0, 4294967295, '{{\"kind\":\"click\"}}') === false && \
                     _lumen_f_queue_event(5, 1, '{{\"kind\":\"click\"}}') === false"
                )
            ));
        });
    }

    // ── Срез 7: focus()/blur()/dispatchEvent через фасад ──────────────────────

    #[test]
    fn facade_focus_and_blur_deliver_on_child_pump_in_order() {
        let (rt_parent, rt_child, _pd, cd) = with_parent_child_pair_docs(true, true);
        rt_parent
            .eval(
                "var b = _lumen_frame_content_document(7).querySelector('b'); \
                 b.focus({ preventScroll: true }); \
                 b.blur();",
            )
            .unwrap();
        assert!(acts(&rt_child).is_empty(), "до пумпы ничего не доставлено");
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let expected = {
            let d = cd.lock().unwrap();
            element_index(&d, "b").expect("<b> в дереве ребёнка")
        };
        let a = acts(&rt_child);
        assert_eq!(
            a,
            vec![
                serde_json::json!(["focus", expected, true]),
                serde_json::json!(["blur", expected]),
            ],
            "порядок focus → blur сохранён, preventScroll перенесён"
        );
    }

    #[test]
    fn facade_dispatch_event_round_trips_flags_and_detail() {
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_parent
            .eval(
                "_lumen_frame_content_document(7).querySelector('b') \
                 .dispatchEvent({ type: 'ping', bubbles: true, cancelable: true, \
                                  detail: { n: 7, tag: 'x' } });",
            )
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let a = acts(&rt_child);
        assert_eq!(a.len(), 1);
        assert_eq!(
            a[0],
            serde_json::json!(["dom", a[0][1], "ping", true, {"n": 7, "tag": "x"}]),
            "type/bubbles/detail дошли до хука получателя"
        );
        // detail — не мусор от сериализации, а исходное значение.
        assert!(eval_bool(
            &rt_child,
            "__acts[0][4].n === 7 && __acts[0][4].tag === 'x'"
        ));
    }

    #[test]
    fn child_facade_actions_address_the_parent_doc() {
        // Симметрия с кликом среза 6: конверт адресован документу-владельцу
        // элемента, пумпа отправителя его не трогает.
        let (rt_parent, rt_child, _pd, _cd) = with_parent_child_pair_docs(true, true);
        rt_child
            .eval("window.parent.document.getElementById('p').focus();")
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(acts(&rt_child).is_empty());
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        let a = acts(&rt_parent);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0][0], serde_json::json!("focus"));
    }

    #[test]
    fn actions_into_inaccessible_child_are_dropped() {
        let (rt_parent, rt_child, _pd, cd) = with_parent_child_pair_docs(true, false);
        let b_nid = {
            let d = cd.lock().unwrap();
            element_index(&d, "b").expect("<b> в дереве ребёнка")
        };
        assert!(eval_bool(
            &rt_parent,
            &format!(
                "_lumen_f_queue_event(0, {b_nid}, '{{\"kind\":\"focus\"}}') === false && \
                 _lumen_f_queue_event(0, {b_nid}, '{{\"kind\":\"blur\"}}') === false && \
                 _lumen_f_queue_event(0, {b_nid}, '{{\"kind\":\"dom\",\"type\":\"x\"}}') === false"
            )
        ));
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(acts(&rt_child).is_empty());
    }

    #[test]
    fn queue_event_rejects_malformed_specs_without_touching_outbox() {
        with_shared_frame("<html><body><b>x</b></body></html>", true, |rt, _| {
            // Невалидный JSON, чужой kind, пустой/нестроковый/слишком длинный
            // type — все отклоняются; после этого валидный blur по-прежнему
            // проходит (ящик жив, мусор его не заблокировал).
            assert!(eval_bool(
                rt,
                "_lumen_f_queue_event(0, 2, 'not json') === false && \
                 _lumen_f_queue_event(0, 2, '{\"kind\":\"zap\"}') === false && \
                 _lumen_f_queue_event(0, 2, '{\"kind\":\"dom\",\"type\":\"\"}') === false && \
                 _lumen_f_queue_event(0, 2, '{\"kind\":\"dom\",\"type\":5}') === false && \
                 _lumen_f_queue_event(0, 2, '{\"kind\":\"dom\"}') === false"
            ));
            let long_type = "x".repeat(129);
            assert!(eval_bool(
                rt,
                &format!(
                    "_lumen_f_queue_event(0, 2, '{}') === false",
                    serde_json::json!({"kind": "dom", "type": long_type})
                )
            ));
            assert!(eval_bool(
                rt,
                "_lumen_f_queue_event(0, 2, '{\"kind\":\"blur\"}') === true"
            ));
        });
    }

    // ── Срез 8: исполнение вставленных из родителя <script> ──────────────────

    #[test]
    fn facade_appended_script_queues_run_script_with_its_nid() {
        let (rt_parent, rt_child, _pd, _cd) = with_parent_child_pair_docs(true, true);
        // Хук среза 8 поверх пары: складываем nid пришедших RunScript.
        rt_child
            .eval(
                "globalThis.__runs = []; \
                 __lumen_C._lumen_deliver_frame_run_script = function(nid) { \
                     __runs.push(nid); \
                 };",
            )
            .unwrap();
        rt_parent
            .eval(
                "var d = _lumen_frame_content_document(7); \
                 var div = d.createElement('div'); \
                 d.body.appendChild(div); \
                 var s = d.createElement('script'); \
                 s.textContent = 'void 0;'; \
                 d.body.appendChild(s); \
                 var s2 = d.createElement('script'); \
                 s2.textContent = 'void 1;'; \
                 d.body.insertBefore(s2, div); \
                 globalThis.__snid = [s.__nid__, s2.__nid__];",
            )
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        // nid скриптов читаем из JS родителя (индексы арены согласованы по
        // определению — фасады строятся над тем же деревом, что читает ребёнок).
        let expected: Vec<u32> = match rt_parent.eval("JSON.stringify(__snid)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        };
        let got: Vec<u32> = match rt_child.eval("JSON.stringify(__runs)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        };
        assert_eq!(
            got, expected,
            "оба скрипта доставлены со своими nid; <div> конверт не ставил"
        );
    }

    #[test]
    fn run_script_envelope_requires_accessible_binding() {
        let (rt_parent, rt_child, _pd, _cd) = with_parent_child_pair_docs(true, false);
        rt_child
            .eval(
                "globalThis.__runs = []; \
                 __lumen_C._lumen_deliver_frame_run_script = function(nid) { \
                     __runs.push(nid); \
                 };",
            )
            .unwrap();
        // cross-origin: фасада документа нет вовсе — вставки не было.
        assert!(eval_bool(
            &rt_parent,
            "_lumen_frame_content_document(7) === null"
        ));
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let got: Vec<u32> = match rt_child.eval("JSON.stringify(__runs)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        };
        assert!(got.is_empty());
        // Прямая постановка в cross-origin биндинг тоже режется (native
        // мутации вернул false ещё до queue_run_script_if_script).
        assert!(eval_bool(
            &rt_parent,
            "_lumen_f_append_child(0, 3, 4) === false"
        ));
    }

    // ── Срез 9: URL-рефлексии фасадов и поздний src скрипта ───────────────────

    /// Поздний `setAttribute('src', …)` на `<script>`-фасаде ставит ВТОРОЙ
    /// конверт RunScript — тот же триггер подготовки, что и вставка (HTML LS
    /// §4.12.1). До среза конверт ставился только фактом вставки, поэтому
    /// скрипт, вставленный пустым и получивший src после первой доставки,
    /// не готовился никогда.
    #[test]
    fn facade_late_src_on_script_queues_second_run_script() {
        let (rt_parent, rt_child, _pd, _cd) = with_parent_child_pair_docs(true, true);
        rt_child
            .eval(
                "globalThis.__runs = []; \
                 __lumen_C._lumen_deliver_frame_run_script = function(nid) { \
                     __runs.push(nid); \
                 };",
            )
            .unwrap();
        rt_parent
            .eval(
                "var d = _lumen_frame_content_document(7); \
                 var s = d.createElement('script'); \
                 d.body.appendChild(s); \
                 globalThis.__snid = s.__nid__;",
            )
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(
            match rt_child.eval("JSON.stringify(__runs)") {
                Ok(JsValue::String(s)) => serde_json::from_str::<Vec<u32>>(&s).unwrap_or_default(),
                _ => Vec::new(),
            }
            .len(),
            1,
            "первая доставка — от вставки"
        );
        // Каноничная форма `s.src = url` идёт через ту же запись атрибута.
        rt_parent.eval("s.src = 'https://child.example/x.js';").unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let got: Vec<u32> = match rt_child.eval("JSON.stringify(__runs)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        };
        let snid = u32::try_from(
            match rt_parent.eval("__snid") {
                Ok(JsValue::Number(n)) => n as i64,
                _ => -1,
            },
        )
        .unwrap_or(u32::MAX);
        assert_eq!(got, vec![snid, snid], "поздний src доставил второй конверт с тем же nid");
    }

    /// Рефлексии src/href фасада: сеттер хранит строку ДОСЛОВНО в атрибуте,
    /// геттер отдаёт '' для отсутствующего атрибута и разрешает значение
    /// против базы под-документа (href первого `<base>` против URL документа).
    /// `_url_resolve` в минимальном изоляте отсутствует — тест ставит детерминированный
    /// stub, делающий видимость обоих уровней разрешения.
    #[test]
    fn facade_src_href_reflections_store_verbatim_and_resolve_against_base() {
        let (rt_parent, _rt_child, _pd, _cd) = with_parent_child_pair_docs(true, true);
        rt_parent
            .eval(
                "globalThis._url_resolve = function(rel, base) { \
                     return 'resolved:' + base + '|' + rel; \
                 };",
            )
            .unwrap();
        rt_parent
            .eval(
                "var d = _lumen_frame_content_document(7); \
                 var img = d.createElement('img'); \
                 globalThis.__img = img; \
                 globalThis.__absentSrc = img.src; \
                 globalThis.__absentHref = img.href; \
                 img.src = 'pic.png'; \
                 globalThis.__storedRaw = _lumen_f_attr(0, img.__nid__, 'src'); \
                 globalThis.__resolvedSrc = img.src; \
                 var b = d.createElement('base'); \
                 b.setAttribute('href', '/dir/'); \
                 d.head.appendChild(b); \
                 globalThis.__afterBase = img.src;",
            )
            .unwrap();
        // Геттер без атрибута — '' (не null, не URL документа).
        assert_eq!(
            rt_parent.eval("__absentSrc").ok(),
            Some(JsValue::String(String::new()))
        );
        assert_eq!(
            rt_parent.eval("__absentHref").ok(),
            Some(JsValue::String(String::new()))
        );
        // Сеттер сохранил атрибут дословно.
        assert_eq!(
            rt_parent.eval("__storedRaw").ok(),
            Some(JsValue::String("pic.png".to_owned()))
        );
        // Геттер разрешил против базы; после вставки <base href="/dir/"> базой
        // стало разрешённое значение base-href (двухуровневая композиция §4.2.3).
        assert_eq!(
            rt_parent.eval("__resolvedSrc").ok(),
            Some(JsValue::String(
                "resolved:about:srcdoc|pic.png".to_owned()
            ))
        );
        assert_eq!(
            rt_parent.eval("__afterBase").ok(),
            Some(JsValue::String(
                "resolved:resolved:about:srcdoc|/dir/|pic.png".to_owned()
            ))
        );
        // href — та же пара аксессоров над другим атрибутом (base уже
        // вставлен выше, поэтому разрешение идёт через двухуровневую базу).
        rt_parent.eval("img.href = 'a.html';").unwrap();
        assert_eq!(
            rt_parent.eval("_lumen_f_attr(0, img.__nid__, 'href')").ok(),
            Some(JsValue::String("a.html".to_owned()))
        );
        assert_eq!(
            rt_parent.eval("img.href").ok(),
            Some(JsValue::String(
                "resolved:resolved:about:srcdoc|/dir/|a.html".to_owned()
            ))
        );
    }

    /// BUG-970: `frameElem.attributes` was missing entirely — reading it threw
    /// `TypeError: Cannot read properties of undefined`. Covers length, named
    /// and indexed access, iteration and the mutating methods, all live over
    /// the real child document (not a snapshot taken at `.attributes` read time).
    #[test]
    fn frame_facade_attributes_named_node_map() {
        with_shared_frame(
            "<html><body><p id=\"x\" data-from=\"parent\">hi</p></body></html>",
            true,
            |rt, _doc| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     var p = d.getElementById('x'); \
                     var attrs = p.attributes; \
                     attrs.length === 2 && \
                     attrs.getNamedItem('id').value === 'x' && \
                     attrs.getNamedItem('data-from').value === 'parent' && \
                     attrs.getNamedItem('missing') === null && \
                     attrs['id'].value === 'x' && \
                     attrs[0].name !== undefined && attrs[1].name !== undefined"
                ));
                assert!(eval_bool(
                    rt,
                    "var names = Array.from(d.getElementById('x').attributes).map(function(a) { return a.name; }).sort(); \
                     names.length === 2 && names[0] === 'data-from' && names[1] === 'id'"
                ));
                // Live: a mutation after the read is visible through the same getter.
                assert!(eval_bool(
                    rt,
                    "var p2 = d.getElementById('x'); \
                     p2.setAttribute('new-one', 'v'); \
                     p2.attributes.length === 3 && p2.attributes.getNamedItem('new-one').value === 'v'"
                ));
                assert!(eval_bool(
                    rt,
                    "var p3 = d.getElementById('x'); \
                     var removed = p3.attributes.removeNamedItem('new-one'); \
                     removed.value === 'v' && p3.attributes.length === 2 && !p3.hasAttribute('new-one')"
                ));
            },
        );
    }

    /// BUG-1643: контекст ребёнка рядом с братом. Родитель `<iframe id=a>` (сам
    /// ребёнок) и `<iframe id=b>` (брат с `sib_html`); реестр родителя
    /// зарегистрирован под ключом документа родителя, как это делает
    /// `install_dom` прода. Возвращает рантаймы ребёнка и брата и сам
    /// документ брата.
    fn with_sibling_frames(
        sib_html: &str,
        sib_accessible: bool,
        f: impl FnOnce(&V8JsRuntime, &V8JsRuntime, &Arc<Mutex<lumen_dom::Document>>),
    ) {
        let parent_doc = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<body><iframe id=a></iframe><iframe id=b></iframe></body>",
        )));
        let hosts: Vec<u32> = lumen_dom::collect_iframes(&parent_doc.lock().unwrap())
            .iter()
            .map(|i| i.node.index() as u32)
            .collect();
        assert_eq!(hosts.len(), 2);
        let child_doc = Arc::new(Mutex::new(lumen_html_parser::parse("<p>child")));
        let sib_doc = Arc::new(Mutex::new(lumen_html_parser::parse(sib_html)));

        let parent_reg: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        let binding = |host_nid: u32, doc: &Arc<Mutex<lumen_dom::Document>>, url: &str, accessible: bool| {
            FrameDocBinding {
                host_nid,
                doc: Arc::clone(doc),
                url: url.to_owned(),
                name: None,
                accessible,
                opaque_id: None,
                peer: None,
            }
        };
        {
            let mut reg = parent_reg.lock().unwrap();
            reg.frames.push(binding(hosts[0], &child_doc, "https://a.test/child", true));
            reg.frames.push(binding(hosts[1], &sib_doc, "https://b.test/sib", sib_accessible));
        }
        register_frame_registry(Arc::as_ptr(&parent_doc) as usize, &parent_reg);

        let make_context = |own: &Arc<Mutex<lumen_dom::Document>>, host_nid: u32| {
            let rt = V8JsRuntime::new().unwrap();
            let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
            rt.eval("var window = globalThis;").unwrap();
            install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
            {
                let mut reg = registry.lock().unwrap();
                reg.self_key = Some(Arc::as_ptr(own) as usize);
                reg.self_origin = "https://a.test".to_owned();
                reg.parent = Some(binding(host_nid, &parent_doc, "https://a.test/", true));
            }
            rt.eval(
                "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
            )
            .unwrap();
            rt
        };
        let child = make_context(&child_doc, hosts[0]);
        let sibling = make_context(&sib_doc, hosts[1]);
        f(&child, &sibling, &sib_doc);
    }

    /// BUG-1643: `parent.document.getElementById(id).contentWindow` из дочернего
    /// фрейма — окно соседа; чтение документа соседа и собственный iframe.
    #[test]
    fn child_reaches_sibling_content_window_through_parent_document() {
        with_sibling_frames("<body><p id=x>sib</p></body>", true, |child, _sibling, _| {
            assert!(eval_bool(
                child,
                "var pd = window.parent.document; \
                 var w = pd.getElementById('b').contentWindow; \
                 w !== null && w !== undefined && typeof w.postMessage === 'function' \
                   && w === pd.getElementById('b').contentWindow \
                   && w.document.getElementById('x').textContent === 'sib' \
                   && pd.getElementById('b').contentDocument === w.document"
            ));
            // Собственный iframe ребёнка — настоящее окно этого контекста.
            assert!(eval_bool(
                child,
                "var me = window.parent.document.getElementById('a'); \
                 me.contentWindow === window && me.contentDocument === window.document"
            ));
            // Не iframe и тег без свойства: как и раньше, undefined.
            assert!(eval_bool(
                child,
                "window.parent.document.querySelector('body').contentWindow === undefined"
            ));
            // parent брата — окно родителя.
            assert!(eval_bool(
                child,
                "var sw = window.parent.document.getElementById('b').contentWindow; \
                 sw.parent === window.parent && sw.frameElement.id === 'b'"
            ));
        });
    }

    /// BUG-1643: сосед из другого origin — `contentWindow` есть (postMessage
    /// доступен), `contentDocument` и `.document` — null.
    #[test]
    fn cross_origin_sibling_window_exists_but_hides_document() {
        with_sibling_frames("<body><p id=x>sib</p></body>", false, |child, _sibling, _| {
            assert!(eval_bool(
                child,
                "var pd = window.parent.document; var el = pd.getElementById('b'); \
                 el.contentWindow !== null && typeof el.contentWindow.postMessage === 'function' \
                   && el.contentDocument === null && el.contentWindow.document === null"
            ));
        });
    }

    /// BUG-1643: сообщение ребёнка в окно соседа попадает в ящик соседа, и тот
    /// разбирает его с `source` — фасадом ребёнка в своём реестре.
    #[test]
    fn post_message_between_siblings_reaches_the_sibling_with_a_source() {
        with_sibling_frames("<body></body>", true, |child, sibling, _| {
            assert!(eval_bool(
                child,
                "window.parent.document.getElementById('b').contentWindow.postMessage('hi', '*'); true"
            ));
            let JsValue::String(raw) = sibling.eval("_lumen_frame_take_messages()").unwrap() else {
                panic!("take_messages must return a string");
            };
            let msgs: serde_json::Value = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{e}: {raw}"));
            assert_eq!(msgs.as_array().unwrap().len(), 1, "{raw}");
            assert_eq!(msgs[0]["data"], "hi");
            assert_eq!(msgs[0]["origin"], "https://a.test");
            let bid = msgs[0]["bid"].as_u64().unwrap_or_else(|| panic!("source bid missing: {raw}"));
            assert!(bid >= u64::from(SIBLING_BASE), "{raw}");
            // Принятый source — рабочее окно: постинг обратно доходит до ребёнка.
            assert!(eval_bool(
                sibling,
                &format!("_lumen_f_post_message({bid}, '\"back\"', '*')"),
            ));
        });
    }
