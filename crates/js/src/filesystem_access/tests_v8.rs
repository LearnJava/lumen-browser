    // `panic!` — штатный способ провалить тест; исключение из clippy.toml не
    // достаёт до хелперов модуля (docs/lint-policy.md §10).
    #![allow(clippy::panic, clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Origin every V8 test here installs under; registry entries must be
    /// allocated with the same string or the natives reject them (BUG-371).
    const TEST_ORIGIN: &str = "https://fsal.test";

    #[test]
    fn writable_write_accumulates() {
        let tmp = std::env::temp_dir().join("lumen_fsal_write_test.txt");
        let handle_id = super::write_reg().lock().unwrap().allocate(tmp.clone(), TEST_ORIGIN);
        super::write_reg().lock().unwrap().write_bytes(&handle_id, TEST_ORIGIN, 0, b"hello");
        super::write_reg().lock().unwrap().write_bytes(&handle_id, TEST_ORIGIN, 5, b" world");
        super::write_reg().lock().unwrap().close(&handle_id, TEST_ORIGIN);
        let content = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(content, "hello world");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn writable_close_writes_file() {
        let tmp = std::env::temp_dir().join("lumen_fsal_close_test.txt");
        let handle_id = super::write_reg().lock().unwrap().allocate(tmp.clone(), TEST_ORIGIN);
        super::write_reg().lock().unwrap().write_bytes(&handle_id, TEST_ORIGIN, 0, b"lumen test content");
        let ok = super::write_reg().lock().unwrap().close(&handle_id, TEST_ORIGIN);
        assert!(ok);
        let content = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(content, "lumen test content");
        let _ = std::fs::remove_file(&tmp);
    }

    /// BUG-371 scenario 3: a save handle another page opened must not be
    /// writable — that let any page substitute the bytes the user agreed to
    /// save, at the path the user confirmed.
    #[test]
    fn writable_rejects_foreign_origin() {
        let tmp = std::env::temp_dir().join("lumen_fsal_foreign_write_test.txt");
        let _ = std::fs::write(&tmp, b"honest");
        let handle_id = super::write_reg()
            .lock()
            .unwrap()
            .allocate(tmp.clone(), "https://honest.example");
        assert!(
            !super::write_reg().lock().unwrap().write_bytes(&handle_id, "https://evil.example", 0, b"pwn"),
            "append from a foreign origin must be refused"
        );
        assert!(
            !super::write_reg().lock().unwrap().close(&handle_id, "https://evil.example"),
            "close from a foreign origin must be refused"
        );
        assert_eq!(std::fs::read_to_string(&tmp).unwrap_or_default(), "honest");
        // The legitimate owner still owns the handle after the failed attempts.
        assert!(super::write_reg().lock().unwrap().close(&handle_id, "https://honest.example"));
        let _ = std::fs::remove_file(&tmp);
    }

    /// BUG-371 scenario 2: a directory grant is not reachable from another
    /// origin, so `_lumen_dir_get_file` cannot mint read tokens for it.
    #[test]
    fn dir_grant_rejects_foreign_origin() {
        let tmp = std::env::temp_dir();
        let id = super::dir_reg().lock().unwrap().allocate(tmp, "https://honest.example", false);
        assert!(super::dir_reg().lock().unwrap().get(&id, "https://honest.example").is_some());
        assert!(super::dir_reg().lock().unwrap().get(&id, "https://evil.example").is_none());
    }

    /// BUG-371: ids were `1, 2, 3, …`, so `_lumen_dir_entries(1)` listed
    /// whatever directory the user had last granted.
    #[test]
    fn registry_ids_are_unguessable() {
        let tmp = std::env::temp_dir();
        let id = super::dir_reg().lock().unwrap().allocate(tmp.clone(), TEST_ORIGIN, false);
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        for n in 1..=64u32 {
            assert!(
                super::dir_reg().lock().unwrap().get(&n.to_string(), TEST_ORIGIN).is_none(),
                "small integer {n} must never be a valid directory id"
            );
        }
    }

    /// BUG-371 point 3: a reload revokes what the previous document held.
    #[test]
    fn revoke_origin_drops_only_that_origins_grants() {
        let tmp = std::env::temp_dir();
        let keep = super::dir_reg().lock().unwrap().allocate(tmp.clone(), "https://keep.example", false);
        let drop = super::dir_reg().lock().unwrap().allocate(tmp, "https://drop.example", false);
        super::dir_reg().lock().unwrap().revoke_origin("https://drop.example");
        assert!(super::dir_reg().lock().unwrap().get(&keep, "https://keep.example").is_some());
        assert!(super::dir_reg().lock().unwrap().get(&drop, "https://drop.example").is_none());
    }

    #[test]
    fn json_escape_quotes() {
        let s = r#"say "hello""#;
        let e = super::json_escape(s);
        // Every `"` must be preceded by `\`.
        assert_eq!(e, r#"say \"hello\""#);
    }

    #[test]
    fn json_escape_backslash() {
        let s = r"path\to\file";
        let e = super::json_escape(s);
        assert!(e.contains("\\\\"));
    }

    #[test]
    fn file_entry_json_for_existing_file() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join("lumen_fsal_fej_test.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(b"data").unwrap();
        }
        let json_opt = super::file_entry_json(&tmp, TEST_ORIGIN, false);
        assert!(json_opt.is_some());
        let json = json_opt.unwrap();
        assert!(json.contains("\"name\""));
        // BUG-371: the token is a quoted 32-hex-char string, not a bare number.
        assert!(json.contains("\"token\":\""));
        assert!(json.contains("\"size\":4"));
        let _ = std::fs::remove_file(&tmp);
    }

    // Minimal DOM stubs `install_file_input_bindings_v8`'s shim needs — mirrors
    // `file_input::tests::STUBS` (private to that module, so duplicated here).
    //
    // `setInterval`/`clearInterval` are here because `FileSystemObserver` drives
    // its polling off them and this runtime has no event loop at all. Collecting
    // the callbacks instead of scheduling them also makes the tick *drivable*:
    // `__tick()` is the test's way to say "a poll interval elapsed" without the
    // test depending on wall-clock time.
    //
    // `TextEncoder` is here for the same reason: on a page it arrives with
    // `dom.rs`'s shim, which this harness does not evaluate, and the writable
    // stream encodes every string chunk through it.
    const STUBS: &str = r#"
        var window = globalThis;
        function Blob(blobParts, options) {}
        function _lumen_set_attr(nid, name, val) {}
        function _lumen_get_attr(nid, name) { return undefined; }
        function _lumen_dispatch_bubble(nid, type) {}
        function _lumen_make_element(nid) { return {__nid__: nid}; }
        __lumen_C._lumen_make_element = _lumen_make_element;
        var __intervals = [];
        function setInterval(fn, ms) { __intervals.push(fn); return __intervals.length; }
        function clearInterval(id) { if (id) __intervals[id - 1] = null; }
        function __tick() {
            for (var i = 0; i < __intervals.length; i++) {
                if (__intervals[i]) __intervals[i]();
            }
        }
        function TextEncoder() {}
        TextEncoder.prototype.encode = function(str) {
            var out = [];
            for (var i = 0; i < str.length; i++) {
                var c = str.charCodeAt(i);
                if (c >= 0xD800 && c <= 0xDBFF && (i + 1) < str.length) {
                    var lo = str.charCodeAt(i + 1);
                    if (lo >= 0xDC00 && lo <= 0xDFFF) {
                        c = 0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00);
                        i++;
                    }
                }
                if (c < 0x80) { out.push(c); }
                else if (c < 0x800) { out.push(0xC0 | (c >> 6), 0x80 | (c & 63)); }
                else if (c < 0x10000) {
                    out.push(0xE0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
                } else {
                    out.push(0xF0 | (c >> 18), 0x80 | ((c >> 12) & 63),
                             0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
                }
            }
            return new Uint8Array(out);
        };
    "#;

    /// Two handles every shape test needs, resolved from the mocked pickers.
    ///
    /// BUG-374 removed the public constructors, so a test cannot say
    /// `new FileSystemFileHandle('a.txt', '', 0)` any more — which is the point:
    /// the only way to a handle is the way a page has, through an API that
    /// hands one out.
    const RESOLVE_HANDLES: &str = r#"
        var __fh = null, __dh = null;
        showOpenFilePicker().then(function(list) { __fh = list[0]; });
        showDirectoryPicker().then(function(dir) { __dh = dir; });
    "#;

    /// Private factory the OPFS root is built through (`__lumen_fsa_internal`,
    /// BUG-374) — the only way for a test to turn a directory grant it allocated
    /// itself into a handle, now that the constructor is gone. `storage_manager`
    /// deletes this global on a real page; nothing installs that shim here.
    const ROOT: &str = "__lumen_fsa_internal.makeDirectoryHandle";

    fn with_fsa() -> V8JsRuntime {
        with_fsa_for(TEST_ORIGIN)
    }

    /// Same harness under a caller-chosen origin.
    ///
    /// Tests that allocate their own grants need one: installing revokes every
    /// grant of the origin it installs under (BUG-371 point 3), and the test
    /// binary runs these in parallel over one process-wide registry — sharing
    /// `TEST_ORIGIN` would let one test revoke another's directory mid-run.
    fn with_fsa_for(origin: &str) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        // On a page `install_dom` runs first and brings `DOMException` with it;
        // here nothing does, so every `throw new DOMException(...)` in the shim
        // used to become a `ReferenceError` and no test could look at what the
        // module actually rejects with (BUG-373).
        rt.eval(crate::v8_runtime::DOM_EXCEPTION_POLYFILL).unwrap();
        crate::file_input::install_file_input_bindings_v8(&rt, origin).unwrap();
        super::install_filesystem_access_v8(&rt, origin).unwrap();
        // The real `_lumen_show_*_picker` natives spawn a blocking native OS dialog
        // (PowerShell Windows.Forms on Windows). `showXPicker()`'s `Promise.resolve().then(...)`
        // callback runs as a microtask that V8 drains at the end of THIS `eval()` call (unlike
        // the removed rquickjs harness, which never auto-ran pending jobs — S12b-B8 finding) —
        // so calling `showOpenFilePicker()` etc. here would pop a real dialog and hang the test.
        // Override with non-blocking mocks that resolve the promise instead.
        //
        // BUG-371: since the shim now *captures* the natives at eval time
        // (they are deleted from the global object on a real page), overwriting
        // the globals alone no longer reaches it — the shim has to be
        // re-evaluated afterwards so its IIFE picks the mocks up.
        rt.eval(
            r#"
            __lumen_C._lumen_show_open_file_picker =
              function() { return '{"name":"mock.txt","token":"","size":0}'; };
            __lumen_C._lumen_show_save_file_picker = function() { return 'mock-write-id'; };
            __lumen_C._lumen_show_directory_picker =
              function() { return '{"name":"mockdir","path_id":"mock-dir-id"}'; };
            "#,
        )
        .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt.eval(RESOLVE_HANDLES).unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn fsfh_constructor_exists() {
        let rt = with_fsa();
        assert!(bool_eval(&rt, "typeof window.FileSystemFileHandle === 'function'"));
    }

    #[test]
    fn fsdh_constructor_exists() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.FileSystemDirectoryHandle === 'function'"
        ));
    }

    #[test]
    fn fsws_constructor_exists() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.FileSystemWritableFileStream === 'function'"
        ));
    }

    #[test]
    fn show_open_file_picker_is_function() {
        let rt = with_fsa();
        assert!(bool_eval(&rt, "typeof window.showOpenFilePicker === 'function'"));
    }

    #[test]
    fn show_save_file_picker_is_function() {
        let rt = with_fsa();
        assert!(bool_eval(&rt, "typeof window.showSaveFilePicker === 'function'"));
    }

    #[test]
    fn show_directory_picker_is_function() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.showDirectoryPicker === 'function'"
        ));
    }

    // ── BUG-374: the WebIDL shape of the interface hierarchy ─────────────────

    /// Point 1: `FileSystemHandle` is the base interface, exposed on the global,
    /// and both handle classes inherit from it — instance *and* interface object.
    /// Without it `if (window.FileSystemHandle)` feature-detects concluded the
    /// API was missing and the prototype chain ended at `Object`.
    #[test]
    fn handle_hierarchy_is_webidl() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.FileSystemHandle === 'function' && \
             __fh instanceof FileSystemHandle && __fh instanceof FileSystemFileHandle && \
             __dh instanceof FileSystemHandle && __dh instanceof FileSystemDirectoryHandle && \
             !(__fh instanceof FileSystemDirectoryHandle) && \
             Object.getPrototypeOf(FileSystemFileHandle.prototype) === FileSystemHandle.prototype && \
             Object.getPrototypeOf(FileSystemDirectoryHandle.prototype) === FileSystemHandle.prototype && \
             Object.getPrototypeOf(FileSystemFileHandle) === FileSystemHandle && \
             __fh.constructor === FileSystemFileHandle"
        ));
    }

    /// Point 2: none of the four interfaces declares a constructor. They used to
    /// be publicly constructible *and* took the internal grant id as an
    /// argument, so forging a handle was one line.
    #[test]
    fn constructors_are_illegal() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "function threw(f) { try { f(); return false; } catch (e) { return e instanceof TypeError; } } \
             threw(function() { new FileSystemHandle(); }) && \
             threw(function() { new FileSystemFileHandle('a.txt', 'tok', 0); }) && \
             threw(function() { new FileSystemDirectoryHandle('d', 'pid'); }) && \
             threw(function() { new FileSystemWritableFileStream('wid'); })"
        ));
    }

    /// Point 3: `kind`/`name` are `readonly attribute`s — accessors on the
    /// prototype, not own data properties. `fileHandle.kind = 'directory'` used
    /// to be accepted, after which the handle misreported its own type.
    #[test]
    fn kind_and_name_are_readonly_prototype_accessors() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "var d = Object.getOwnPropertyDescriptor(FileSystemHandle.prototype, 'kind'); \
             __fh.kind = 'directory'; __fh.name = 'other.txt'; \
             typeof d.get === 'function' && d.set === undefined && \
             Object.getOwnPropertyNames(__fh).length === 0 && \
             Object.getOwnPropertyNames(__dh).length === 0 && \
             __fh.kind === 'file' && __fh.name === 'mock.txt' && __dh.kind === 'directory'"
        ));
    }

    /// Point 3, second half: the grant ids stay in private slots (BUG-371).
    #[test]
    fn handle_internals_are_not_web_visible() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "__fh._token === undefined && __dh._pathId === undefined && \
             JSON.stringify(__fh) === '{}' && JSON.stringify(__dh) === '{}'"
        ));
    }

    /// Point 4.
    #[test]
    fn handles_have_a_to_string_tag() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "Object.prototype.toString.call(__fh) === '[object FileSystemFileHandle]' && \
             Object.prototype.toString.call(__dh) === '[object FileSystemDirectoryHandle]'"
        ));
    }

    /// Point 5: the base interface's members live on the base prototype, and
    /// `queryPermission`/`requestPermission`/`remove`/`getUniqueId` exist at all.
    #[test]
    fn base_members_are_on_the_base_prototype() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "var p = FileSystemHandle.prototype; \
             ['isSameEntry','queryPermission','requestPermission','remove','getUniqueId'] \
               .every(function(m) { return typeof p[m] === 'function' && \
                                          !Object.prototype.hasOwnProperty.call( \
                                             FileSystemFileHandle.prototype, m); }) && \
             typeof FileSystemFileHandle.prototype.getFile === 'function' && \
             typeof FileSystemFileHandle.prototype.createWritable === 'function' && \
             typeof FileSystemFileHandle.prototype.move === 'function' && \
             ['entries','values','keys','getFileHandle','getDirectoryHandle','removeEntry','resolve'] \
               .every(function(m) { \
                 return typeof FileSystemDirectoryHandle.prototype[m] === 'function'; })"
        ));
    }

    /// Point 6: `async iterable<USVString, FileSystemHandle>` — the handle
    /// itself is iterable, not only the object `entries()` returns.
    #[test]
    fn directory_handle_is_async_iterable() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof __dh[Symbol.asyncIterator] === 'function' && \
             typeof __dh.entries()[Symbol.asyncIterator] === 'function' && \
             typeof __dh.values()[Symbol.asyncIterator] === 'function' && \
             typeof __dh.keys()[Symbol.asyncIterator] === 'function'"
        ));
    }

    /// A method called on a foreign object must not read someone else's state —
    /// the brand check is what a private-slot design buys.
    #[test]
    fn base_members_reject_a_foreign_this() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "var thrown = false; \
             try { Object.getOwnPropertyDescriptor(FileSystemHandle.prototype, 'kind') \
                     .get.call({}); } catch (e) { thrown = e instanceof TypeError; } \
             var rejected = false; \
             FileSystemHandle.prototype.getUniqueId.call({}) \
               .catch(function(e) { rejected = e instanceof TypeError; }); \
             thrown"
        ));
    }

    #[test]
    fn dir_entries_empty_for_unknown_id() {
        let rt = with_fsa();
        let r = rt.eval("_lumen_dir_entries('9999999')").unwrap();
        assert_eq!(r, JsValue::String("[]".into()));
    }

    #[test]
    fn dir_entries_returns_json_array_for_real_dir() {
        let tmp = std::env::temp_dir();
        let pid = super::dir_reg().lock().unwrap().allocate(tmp, TEST_ORIGIN, false);
        let rt = with_fsa();
        let r = rt.eval(&format!("_lumen_dir_entries('{pid}')")).unwrap();
        match r {
            JsValue::String(s) => {
                assert!(s.starts_with('['), "expected JSON array, got: {s}");
            }
            other => panic!("expected string JSON, got {other:?}"),
        }
    }

    #[test]
    fn show_open_file_picker_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.showOpenFilePicker().then === 'function'"
        ));
    }

    #[test]
    fn show_save_file_picker_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.showSaveFilePicker({suggestedName:'out.txt'}).then === 'function'"
        ));
    }

    #[test]
    fn show_directory_picker_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.showDirectoryPicker().then === 'function'"
        ));
    }

    // ── BUG-372: the origin private file system ──────────────────────────────

    /// Origin, runtime, fresh empty directory and a grant on it, as OPFS hands
    /// one out. Allocated *after* the install that would have revoked it.
    fn opfs_case(tag: &str, writable: bool) -> (V8JsRuntime, std::path::PathBuf, String) {
        let origin = format!("https://{tag}.opfs.test");
        let rt = with_fsa_for(&origin);
        let dir = std::env::temp_dir().join(format!("lumen_opfs_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let id = super::dir_reg()
            .lock()
            .unwrap()
            .allocate(dir.clone(), &origin, writable);
        (rt, dir, id)
    }

    fn string_eval(rt: &V8JsRuntime, expr: &str) -> String {
        match rt.eval(expr).unwrap() {
            JsValue::String(s) => s,
            other => panic!("expected a string, got {other:?}"),
        }
    }

    /// Two origins never share an OPFS directory, however similar they look
    /// once the unusable characters are stripped out.
    #[test]
    fn origin_slug_separates_similar_origins() {
        let a = super::origin_slug("https://example.com");
        let b = super::origin_slug("https://example.com:8443");
        let c = super::origin_slug("http://example.com");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
        // Long opaque origins (a `file:` URL is its own origin here) stay usable
        // as a directory name.
        let long = super::origin_slug(&format!("file:///{}", "d/".repeat(200)));
        assert!(long.len() <= 49, "slug too long for a path component: {long}");
        assert!(!long.contains('/'), "slug must be a single path component");
    }

    /// A writable grant really creates and really removes. The OPFS stub this
    /// replaces resolved `removeEntry()` while removing nothing, and answered
    /// `getFileHandle()` with an object literal that had no `getFile()`.
    #[test]
    fn writable_grant_creates_and_removes_entries() {
        let (rt, dir, pid) = opfs_case("create_remove", true);

        let created = string_eval(&rt, &format!("_lumen_dir_get_file('{pid}', 'made.txt', true)"));
        assert!(created.contains(r#""token":""#), "no token in {created}");
        assert!(dir.join("made.txt").is_file(), "the file was never created");

        let removed = string_eval(&rt, &format!("_lumen_dir_remove_entry('{pid}', 'made.txt', false)"));
        assert_eq!(removed, "", "removeEntry reported {removed}");
        assert!(!dir.join("made.txt").exists(), "the file was never removed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A non-empty directory needs `{recursive:true}` — the one removal failure
    /// the caller is expected to handle.
    #[test]
    fn remove_entry_needs_recursive_for_a_non_empty_directory() {
        let (rt, dir, pid) = opfs_case("recursive", true);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("leaf.txt"), b"x").unwrap();

        let err = string_eval(&rt, &format!("_lumen_dir_remove_entry('{pid}', 'sub', false)"));
        assert_eq!(err, "InvalidModificationError");
        assert!(dir.join("sub").is_dir());

        let ok = string_eval(&rt, &format!("_lumen_dir_remove_entry('{pid}', 'sub', true)"));
        assert_eq!(ok, "");
        assert!(!dir.join("sub").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A picked directory stays a read grant: `{create:true}` on it is refused,
    /// and nothing appears on disk.
    #[test]
    fn read_only_grant_refuses_to_create_or_remove() {
        let (rt, dir, pid) = opfs_case("read_only", false);
        std::fs::write(dir.join("present.txt"), b"x").unwrap();

        assert_eq!(
            string_eval(&rt, &format!("_lumen_dir_get_file('{pid}', 'new.txt', true)")),
            r#"{"error":"NotAllowedError"}"#
        );
        assert!(!dir.join("new.txt").exists());
        assert_eq!(
            string_eval(&rt, &format!("_lumen_dir_remove_entry('{pid}', 'present.txt', false)")),
            "NotAllowedError"
        );
        assert!(dir.join("present.txt").is_file());
        // Reading what the grant does cover still works.
        assert!(
            string_eval(&rt, &format!("_lumen_dir_get_file('{pid}', 'present.txt', false)"))
                .contains(r#""token":""#)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An entry name is one path component. Without this check `{create:true}`
    /// would let page script write anywhere the process can reach.
    #[test]
    fn entry_names_cannot_escape_the_directory() {
        let (rt, dir, pid) = opfs_case("traversal", true);
        for name in ["", ".", "..", "../escaped.txt", "sub/child.txt", "sub\\\\child.txt"] {
            assert_eq!(
                string_eval(&rt, &format!("_lumen_dir_get_file('{pid}', '{name}', true)")),
                r#"{"error":"TypeError"}"#,
                "name {name:?} was accepted"
            );
            assert_eq!(
                string_eval(&rt, &format!("_lumen_dir_get_subdir('{pid}', '{name}', true)")),
                r#"{"error":"TypeError"}"#,
                "name {name:?} was accepted as a directory"
            );
        }
        assert!(!dir.parent().unwrap().join("escaped.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `resolve()` answers with the segments between the two handles, `[]` for
    /// the handle itself and `null` for anything that is not below it. The stub
    /// answered `null` to all four, so a caller could not tell "not a
    /// descendant" from "not implemented".
    #[test]
    fn resolve_reports_the_path_of_a_descendant() {
        let (rt, dir, pid) = opfs_case("resolve", true);
        let out = string_eval(
            &rt,
            &format!(
                r#"
                var sub = JSON.parse(_lumen_dir_get_subdir('{pid}', 'sub', true));
                var leaf = JSON.parse(_lumen_dir_get_file(sub.path_id, 'leaf.txt', true));
                var r1 = _lumen_fs_resolve('{pid}', sub.path_id, '');
                var r2 = _lumen_fs_resolve('{pid}', '', leaf.token);
                var r3 = _lumen_fs_resolve(sub.path_id, '{pid}', '');
                var r4 = _lumen_fs_resolve('{pid}', '{pid}', '');
                [r1, r2, (r3 == null ? 'null' : r3), r4].join(';')
                "#
            ),
        );
        assert_eq!(out, r#"["sub"];["sub","leaf.txt"];null;[]"#);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A handle from the origin's own sandbox writes straight to its file. If
    /// it fell through to the save picker instead, the mocked picker id would
    /// belong to no write handle and the bytes would go nowhere — which is what
    /// the assertion on the file content catches.
    #[test]
    fn opfs_file_handle_writes_without_the_save_picker() {
        let (rt, dir, pid) = opfs_case("writable_handle", true);
        rt.eval(&format!(
            r#"
            var __w = null;
            {ROOT}('root', '{pid}')
              .getFileHandle('out.txt', {{ create: true }})
              .then(function(fh) {{ return fh.createWritable(); }})
              .then(function(s) {{ __w = s; }});
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__w !== null"), "createWritable never resolved");
        rt.eval("__w.write('hello opfs');").unwrap();
        rt.eval("__w.close();").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.txt")).unwrap_or_default(),
            "hello opfs"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A picked file keeps needing the save picker: a read grant must not turn
    /// into a write grant just because the page asked twice.
    #[test]
    fn picked_file_token_grants_no_direct_write() {
        const ORIGIN: &str = "https://picked.opfs.test";
        let rt = with_fsa_for(ORIGIN);
        let tmp = std::env::temp_dir().join("lumen_opfs_picked_read_only.txt");
        std::fs::write(&tmp, b"original").unwrap();
        let token = crate::file_input::register_file_token(tmp.to_str().unwrap(), ORIGIN);
        assert!(bool_eval(
            &rt,
            &format!("_lumen_writable_from_token('{token}') == null")
        ));
        let _ = std::fs::remove_file(&tmp);
    }

    /// The root handed to `navigator.storage.getDirectory()` is an ordinary
    /// entry of the one directory registry, writable, under `data/opfs/`.
    #[test]
    fn opfs_root_entry_is_a_writable_dir_grant() {
        const ORIGIN: &str = "https://opfs-root.test";
        let json = super::opfs_root_entry_json(ORIGIN).expect("no OPFS root");
        assert!(json.starts_with(r#"{"name":"","path_id":""#), "{json}");
        let id = json
            .rsplit_once(r#""path_id":""#)
            .and_then(|(_, rest)| rest.split('"').next())
            .unwrap_or_default()
            .to_string();
        let grant = super::dir_reg().lock().unwrap().get_grant(&id, ORIGIN);
        let (path, writable) = grant.expect("root id is not a grant of that origin");
        assert!(writable, "the OPFS root must be writable");
        assert!(path.is_dir(), "the OPFS root was not created: {path:?}");
        assert!(path.ends_with(super::origin_slug(ORIGIN)));
        assert!(super::dir_reg().lock().unwrap().get_grant(&id, "https://other.test").is_none());
        let _ = std::fs::remove_dir_all(&path);
    }

    // ── BUG-373: which field carries the error name ──────────────────────────

    /// The harness of `with_fsa_for`, but every picker native answers `null` —
    /// the "user closed the dialog" path. The shim captures its natives at eval
    /// time (BUG-371), so the mocks go in before `FSAL_SHIM` is re-evaluated.
    fn with_cancelling_pickers(origin: &str) -> V8JsRuntime {
        let rt = with_fsa_for(origin);
        rt.eval(
            r#"
            __lumen_C._lumen_show_open_file_picker  = function() { return null; };
            __lumen_C._lumen_show_save_file_picker  = function() { return null; };
            __lumen_C._lumen_show_directory_picker  = function() { return null; };
            "#,
        )
        .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt
    }

    /// Settle `expr` (a promise the caller expects to reject) and report the
    /// rejection as `"<is a DOMException>|<name>|<message>"`.
    ///
    /// Every older test here stops at "the call returned a promise", which is
    /// exactly why nine swapped constructor arguments stayed green (BUG-373):
    /// a rejection can only be told apart by `name`, so a test that never reads
    /// it cannot see the two fields trade places.
    fn await_rejection(rt: &V8JsRuntime, expr: &str) -> String {
        rt.eval(&format!(
            r#"
            var __rej = 'never settled';
            ({expr}).then(
              function() {{ __rej = 'resolved'; }},
              function(e) {{
                __rej = (e instanceof DOMException) + '|' + e.name + '|' + e.message;
              }});
            "#
        ))
        .unwrap();
        string_eval(rt, "String(__rej)")
    }

    /// A cancelled open picker is the case the spec expects callers to swallow
    /// (`if (e.name === 'AbortError') return;`). With the arguments swapped the
    /// name was the human sentence, so a cancel was indistinguishable from a
    /// real failure and every dialog dismissal reached the error branch.
    #[test]
    fn cancelled_open_picker_rejects_with_abort_error() {
        let rt = with_cancelling_pickers("https://abort-open.fsal.test");
        assert_eq!(
            await_rejection(&rt, "window.showOpenFilePicker()"),
            "true|AbortError|The user aborted a request."
        );
    }

    #[test]
    fn cancelled_save_picker_rejects_with_abort_error() {
        let rt = with_cancelling_pickers("https://abort-save.fsal.test");
        assert_eq!(
            await_rejection(&rt, "window.showSaveFilePicker()"),
            "true|AbortError|The user aborted a request."
        );
    }

    #[test]
    fn cancelled_directory_picker_rejects_with_abort_error() {
        let rt = with_cancelling_pickers("https://abort-dir.fsal.test");
        assert_eq!(
            await_rejection(&rt, "window.showDirectoryPicker()"),
            "true|AbortError|The user aborted a request."
        );
    }

    /// The one non-picker site: a handle with no sandbox grant falls through to
    /// the save dialog, and a refused dialog must say `NotAllowedError`.
    ///
    /// Only the *save* picker is cancelled here: the open picker still has to
    /// hand out the grant-less handle the test then tries to write through.
    #[test]
    fn refused_write_permission_rejects_with_not_allowed_error() {
        let rt = with_fsa_for("https://abort-write.fsal.test");
        rt.eval("__lumen_C._lumen_show_save_file_picker = function() { return null; };")
            .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt.eval(RESOLVE_HANDLES).unwrap();
        assert_eq!(
            await_rejection(&rt, "__fh.createWritable()"),
            "true|NotAllowedError|Write permission denied or user cancelled"
        );
    }

    /// The `fsThrow` path already ordered its arguments correctly; this pins it
    /// so the two ways this module raises a DOMException cannot drift apart.
    /// It also covers the live probe from the bug report verbatim:
    /// `getFileHandle('nope.txt')` reported `name='File not found: nope.txt'`.
    #[test]
    fn missing_entry_rejects_with_not_found_error() {
        let (rt, dir, pid) = opfs_case("missing_entry", true);
        assert_eq!(
            await_rejection(
                &rt,
                &format!("{ROOT}('root', '{pid}').getFileHandle('nope.txt')")
            ),
            "true|NotFoundError|Cannot open file: nope.txt"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── BUG-374 points 7, 9, 10: what the objects actually do ────────────────

    /// Settle `expr` and report the rejection's `name` (or `resolved`).
    ///
    /// Picker-option validation raises plain `TypeError`s, which
    /// [`await_rejection`] cannot tell apart from each other by message alone.
    fn rejection_name(rt: &V8JsRuntime, expr: &str) -> String {
        rt.eval(&format!(
            "var __n = 'never settled'; \
             ({expr}).then(function() {{ __n = 'resolved'; }}, \
                           function(e) {{ __n = String(e && e.name); }});"
        ))
        .unwrap();
        string_eval(rt, "String(__n)")
    }

    /// A writable stream over a real file in a writable grant.
    fn opfs_writable(tag: &str) -> (V8JsRuntime, std::path::PathBuf) {
        let (rt, dir, pid) = opfs_case(tag, true);
        rt.eval(&format!(
            r#"
            var __w = null;
            {ROOT}('root', '{pid}')
              .getFileHandle('out.bin', {{ create: true }})
              .then(function(fh) {{ return fh.createWritable(); }})
              .then(function(s) {{ __w = s; }});
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__w !== null"), "createWritable never resolved");
        (rt, dir)
    }

    /// Point 7: `truncate()` used to return a resolved promise and do nothing,
    /// so code that truncated a file before rewriting it got the old bytes with
    /// the new ones appended.
    #[test]
    fn truncate_shortens_the_written_file() {
        let (rt, dir) = opfs_writable("truncate");
        rt.eval("__w.write('hello world'); __w.truncate(5); __w.close();").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.bin")).unwrap_or_default(),
            "hello"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 7: `seek()` was the other no-op — every write appended, wherever
    /// the caller had positioned the stream.
    #[test]
    fn seek_writes_at_the_position() {
        let (rt, dir) = opfs_writable("seek");
        rt.eval("__w.write('abc'); __w.seek(0); __w.write('X'); __w.close();").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.bin")).unwrap_or_default(),
            "Xbc"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// FS §7.1's `WriteParams` dictionary — `write({type, position, size, data})`
    /// used to fall into `String(data)` whole and land in the file as
    /// `[object Object]`.
    #[test]
    fn write_params_dictionary_is_understood() {
        let (rt, dir) = opfs_writable("write_params");
        rt.eval(
            "__w.write({type:'write', data:'abcdef'}); \
             __w.write({type:'write', position:1, data:'ZZ'}); \
             __w.write({type:'truncate', size:4}); \
             __w.close();",
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.bin")).unwrap_or_default(),
            "aZZd"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `seek` command with no position and an unknown command type are the two
    /// shapes the spec rejects rather than silently ignores.
    #[test]
    fn malformed_write_commands_reject() {
        let (rt, dir) = opfs_writable("write_params_bad");
        assert_eq!(rejection_name(&rt, "__w.write({type:'seek'})"), "SyntaxError");
        assert_eq!(rejection_name(&rt, "__w.write({type:'truncate'})"), "SyntaxError");
        assert_eq!(rejection_name(&rt, "__w.write({type:'nope', data:'x'})"), "TypeError");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 7: bytes are bytes. `write()` used to push everything through
    /// `String(data)`, so a typed array became the text `0,1,255` and a `Blob`
    /// became the literal `[object Blob]`.
    #[test]
    fn binary_chunks_are_written_verbatim() {
        let (rt, dir) = opfs_writable("binary");
        rt.eval("__w.write(new Uint8Array([0, 1, 255, 65])); __w.close();").unwrap();
        assert_eq!(
            std::fs::read(dir.join("out.bin")).unwrap_or_default(),
            vec![0u8, 1, 255, 65]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Non-ASCII text survives the base64 hop to Rust as UTF-8.
    #[test]
    fn utf8_text_round_trips() {
        let (rt, dir) = opfs_writable("utf8");
        rt.eval(r#"__w.write('привет \u{1F600}'); __w.close();"#).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.bin")).unwrap_or_default(),
            "привет \u{1F600}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 6, the half that mattered at runtime: iterating a directory yields
    /// handles that *work*. They used to be built on an empty grant id, so the
    /// names were right and everything behind them was empty.
    #[test]
    fn iterated_entries_carry_a_working_grant() {
        let (rt, dir, pid) = opfs_case("iterate", true);
        std::fs::write(dir.join("a.txt"), b"payload").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("inner.txt"), b"deep").unwrap();
        rt.eval(&format!(
            r#"
            var __names = [], __sizes = [], __inner = 'none';
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              for await (var pair of root) {{
                __names.push(pair[0] + ':' + pair[1].kind);
                if (pair[1].kind === 'file') {{
                  var f = await pair[1].getFile();
                  __sizes.push(pair[0] + '=' + f.size);
                }} else {{
                  for await (var child of pair[1]) {{ __inner = child[0]; }}
                }}
              }}
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(string_eval(&rt, "__names.sort().join(',')"), "a.txt:file,sub:directory");
        assert_eq!(string_eval(&rt, "__sizes.join(',')"), "a.txt=7");
        assert_eq!(string_eval(&rt, "__inner"), "inner.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 5: two handles on the same entry compare equal even though each
    /// carries its own freshly minted grant token.
    #[test]
    fn is_same_entry_sees_through_distinct_tokens() {
        let (rt, dir, pid) = opfs_case("same_entry", true);
        rt.eval(&format!(
            r#"
            var __same = null, __diff = null, __uid = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var a = await root.getFileHandle('a.txt', {{ create: true }});
              var b = await root.getFileHandle('a.txt', {{ create: true }});
              var c = await root.getFileHandle('c.txt', {{ create: true }});
              __same = await a.isSameEntry(b);
              __diff = await a.isSameEntry(c);
              __uid  = (await a.getUniqueId()) === (await b.getUniqueId());
            }})();
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__same === true"), "two handles on one file compared unequal");
        assert!(bool_eval(&rt, "__diff === false"));
        assert!(bool_eval(&rt, "__uid === true"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 5: `queryPermission` answers from the grant behind the handle — a
    /// writable sandbox grant is `granted` for both modes, a picked read-only
    /// directory says `prompt` for `readwrite` rather than claiming a write
    /// permission the next call would contradict.
    #[test]
    fn permission_answers_follow_the_grant() {
        let (rt, dir, pid) = opfs_case("perm_rw", true);
        let (rt_ro, dir_ro, pid_ro) = opfs_case("perm_ro", false);
        rt.eval(&format!("var __d = {ROOT}('root', '{pid}');")).unwrap();
        rt_ro.eval(&format!("var __d = {ROOT}('root', '{pid_ro}');")).unwrap();

        assert_eq!(string_eval_await(&rt, "__d.queryPermission()"), "granted");
        assert_eq!(
            string_eval_await(&rt, "__d.queryPermission({mode:'readwrite'})"),
            "granted"
        );
        assert_eq!(string_eval_await(&rt_ro, "__d.queryPermission()"), "granted");
        assert_eq!(
            string_eval_await(&rt_ro, "__d.requestPermission({mode:'readwrite'})"),
            "prompt"
        );
        assert_eq!(rejection_name(&rt, "__d.queryPermission({mode:'write'})"), "TypeError");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir_ro);
    }

    /// Settle `expr` and read the fulfilled value back as a string.
    fn string_eval_await(rt: &V8JsRuntime, expr: &str) -> String {
        rt.eval(&format!(
            "var __s = 'never settled'; \
             ({expr}).then(function(v) {{ __s = String(v); }}, \
                           function(e) {{ __s = 'rejected:' + (e && e.name); }});"
        ))
        .unwrap();
        string_eval(rt, "String(__s)")
    }

    /// Point 5: `remove()` and `move()` — both were missing outright.
    #[test]
    fn remove_and_move_act_on_the_file_system() {
        let (rt, dir, pid) = opfs_case("remove_move", true);
        rt.eval(&format!(
            r#"
            var __moved = 'none', __removed = 'none';
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('before.txt', {{ create: true }});
              await f.move('after.txt');
              __moved = f.name;
              var g = await root.getFileHandle('gone.txt', {{ create: true }});
              await g.remove();
              __removed = 'ok';
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(string_eval(&rt, "__moved"), "after.txt");
        assert_eq!(string_eval(&rt, "__removed"), "ok");
        assert!(dir.join("after.txt").is_file(), "move() did not rename the file");
        assert!(!dir.join("before.txt").exists(), "the old name survived move()");
        assert!(!dir.join("gone.txt").exists(), "remove() did not delete the file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A read-only grant refuses both, rather than reporting success.
    #[test]
    fn remove_refuses_a_read_only_grant() {
        let (rt, dir, pid) = opfs_case("remove_ro", false);
        std::fs::write(dir.join("keep.txt"), b"x").unwrap();
        rt.eval(&format!(
            r#"
            var __p = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              __p = await root.getFileHandle('keep.txt');
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(rejection_name(&rt, "__p.remove()"), "NotAllowedError");
        assert!(dir.join("keep.txt").is_file(), "a read-only grant deleted a file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 10: the pickers took an options dictionary and ignored it whole.
    /// Every case here is a `TypeError` the spec states, and every one of them
    /// used to open the dialog instead.
    #[test]
    fn picker_options_are_validated() {
        let rt = with_fsa();
        let cases = [
            "showOpenFilePicker({types:[], excludeAcceptAllOption:true})",
            "showOpenFilePicker({types:[{accept:{'image':['.png']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain;charset=utf-8':['.txt']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain':['txt']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain':['.TXT']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain':['.txt.']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain':['.abcdefghijklmnop']}}]})",
            "showOpenFilePicker({types:[{}]})",
            "showOpenFilePicker({startIn:'nowhere'})",
            "showOpenFilePicker({id:'has spaces'})",
            "showSaveFilePicker({id:'0123456789012345678901234567890123'})",
            "showDirectoryPicker({startIn:'nowhere'})",
        ];
        for case in cases {
            assert_eq!(rejection_name(&rt, case), "TypeError", "accepted: {case}");
        }
    }

    /// …and the shapes the spec allows still reach the picker.
    #[test]
    fn valid_picker_options_are_accepted() {
        let rt = with_fsa();
        let cases = [
            "showOpenFilePicker({types:[{description:'Text', accept:{'text/plain':['.txt','.md']}}]})",
            "showOpenFilePicker({startIn:'documents', id:'lumen_test-1'})",
            "showOpenFilePicker({types:[], excludeAcceptAllOption:false})",
            "showDirectoryPicker({startIn:__dh})",
        ];
        for case in cases {
            assert_eq!(rejection_name(&rt, case), "resolved", "rejected: {case}");
        }
    }

    /// Point 9: all three interfaces are `[Serializable]`. A clone used to come
    /// back as a plain `{}` — same properties, no class, no grant.
    ///
    /// The registry is the one `dom.rs`'s shim publishes; this harness does not
    /// evaluate that shim, so the test stands one in of the same shape and
    /// checks what the *cloner* produces.
    #[test]
    fn handles_register_a_structured_clone() {
        let (rt, dir, pid) = opfs_case("serializable", true);
        rt.eval(
            r#"
            var CLONERS = [];
            Object.defineProperty(__lumen_C, '__lumen_platform_cloners', {
              value: {
                register: function(test, clone) { CLONERS.push([test, clone]); },
                find: function(v) {
                  for (var i = 0; i < CLONERS.length; i++) {
                    if (CLONERS[i][0](v)) return CLONERS[i][1];
                  }
                  return null;
                }
              },
              configurable: true
            });
            "#,
        )
        .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt.eval(&format!(
            r#"
            var __ok = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('cloned.txt', {{ create: true }});
              var clone = __lumen_C.__lumen_platform_cloners.find(f)(f);
              __ok = (clone instanceof FileSystemFileHandle)
                  && (clone !== f) && clone.name === 'cloned.txt'
                  && (await clone.isSameEntry(f))
                  && __lumen_C.__lumen_platform_cloners.find(root)(root) instanceof
                       FileSystemDirectoryHandle;
            }})();
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__ok === true"), "the clone lost its class or its grant");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 7, the inheritance half: the class extends whatever
    /// `WritableStream` the runtime has, and the stream's sink is the FS write
    /// algorithm — so `getWriter().write(chunk)` commits the same bytes as
    /// `stream.write(chunk)`.
    ///
    /// The base here is a stand-in (the real one comes from `dom.rs`'s shim,
    /// which this harness does not evaluate). What it can prove is the wiring:
    /// the prototype chain, and that the sink handed to the base is the one that
    /// reaches Rust. Like the real one, the stand-in refuses a call without
    /// `new` (BUG-684), so the subclass has to construct its base.
    #[test]
    fn writable_extends_the_runtime_writable_stream() {
        let (rt, dir, pid) = opfs_case("stream_base", true);
        rt.eval(
            r#"
            function WritableStream(sink) {
              if (new.target === undefined) throw new TypeError('use new');
              this._ws_sink = sink; this._ws_state = 'writable';
            }
            WritableStream.prototype.getWriter = function() {
              var sink = this._ws_sink;
              return { write: function(chunk) { return sink.write(chunk); } };
            };
            WritableStream.prototype.close = function() {
              var sink = this._ws_sink;
              return Promise.resolve().then(function() { return sink.close(); });
            };
            globalThis.WritableStream = WritableStream;
            "#,
        )
        .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt.eval(&format!(
            r#"
            var __w = null, __chain = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('stream.txt', {{ create: true }});
              __w = await f.createWritable();
              __chain = (__w instanceof FileSystemWritableFileStream)
                     && (__w instanceof WritableStream);
              await __w.getWriter().write('through the writer');
              await __w.close();
            }})();
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__chain === true"), "not a WritableStream subclass");
        assert_eq!(
            std::fs::read_to_string(dir.join("stream.txt")).unwrap_or_default(),
            "through the writer"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }


    // ── BUG-374 point 8: FileSystemSyncAccessHandle ──────────────────────────

    /// The interface was missing outright. It is the OPFS write path every
    /// wasm-backed database uses, and unlike the writable stream it is
    /// unbuffered: what one call writes, the next call reads back.
    #[test]
    fn sync_access_handle_reads_back_what_it_wrote() {
        let (rt, dir, pid) = opfs_case("sync_rw", true);
        rt.eval(&format!(
            r#"
            var __shape = null, __written = null, __size = null, __read = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('sync.bin', {{ create: true }});
              var h = await f.createSyncAccessHandle();
              __shape = (h instanceof FileSystemSyncAccessHandle) + ',' +
                        Object.prototype.toString.call(h);
              __written = h.write(new Uint8Array([115, 121, 110, 99, 33]), {{ at: 0 }});
              __size = h.getSize();
              var buf = new Uint8Array(2);
              var n = h.read(buf, {{ at: 2 }});
              __read = n + ':' + buf[0] + ',' + buf[1];
              h.flush();
              h.close();
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(
            string_eval(&rt, "String(__shape)"),
            "true,[object FileSystemSyncAccessHandle]"
        );
        assert_eq!(string_eval(&rt, "String(__written)"), "5");
        assert_eq!(string_eval(&rt, "String(__size)"), "5");
        assert_eq!(string_eval(&rt, "String(__read)"), "2:110,99");
        assert_eq!(std::fs::read(dir.join("sync.bin")).unwrap_or_default(), b"sync!".to_vec());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `truncate()` is real here too, `close()` is idempotent, and every other
    /// member throws `InvalidStateError` once the handle is closed.
    #[test]
    fn sync_access_handle_truncates_and_closes() {
        let (rt, dir, pid) = opfs_case("sync_close", true);
        rt.eval(&format!(
            r#"
            var __after = null, __closed = null, __again = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('sync.bin', {{ create: true }});
              var h = await f.createSyncAccessHandle();
              h.write(new Uint8Array([1, 2, 3, 4, 5, 6]), {{ at: 0 }});
              h.truncate(3);
              __after = h.getSize();
              h.close();
              try {{ h.getSize(); __closed = 'no throw'; }} catch (e) {{ __closed = e.name; }}
              try {{ h.close(); __again = 'ok'; }} catch (e) {{ __again = e.name; }}
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(string_eval(&rt, "String(__after)"), "3");
        assert_eq!(string_eval(&rt, "String(__closed)"), "InvalidStateError");
        assert_eq!(string_eval(&rt, "String(__again)"), "ok");
        assert_eq!(std::fs::read(dir.join("sync.bin")).unwrap_or_default(), vec![1u8, 2, 3]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// FS §7.2 puts this interface on OPFS handles only. A picked file carries a
    /// read grant, so it must be refused rather than silently opened for writing.
    #[test]
    fn sync_access_handle_refuses_a_read_only_grant() {
        let (rt, dir, pid) = opfs_case("sync_ro", false);
        std::fs::write(dir.join("keep.txt"), b"x").unwrap();
        rt.eval(&format!(
            r#"
            var __h = null;
            (async function() {{
              __h = await {ROOT}('root', '{pid}').getFileHandle('keep.txt');
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(
            rejection_name(&rt, "__h.createSyncAccessHandle()"),
            "NotAllowedError"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The size a handle was created with goes stale the moment anything writes
    /// through it: `getFile()` reported the old length while `text()` already
    /// returned the new contents (found by the live probe, `hello/0`).
    #[test]
    fn get_file_reports_the_current_size() {
        let (rt, dir, pid) = opfs_case("stale_size", true);
        rt.eval(&format!(
            r#"
            var __size = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('grow.txt', {{ create: true }});
              var w = await f.createWritable();
              await w.write('0123456789');
              await w.close();
              __size = (await f.getFile()).size;
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(string_eval(&rt, "String(__size)"), "10");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── BUG-389: FileSystemObserver (FS Observer §2-§3) ──────────────────────

    /// A runtime already observing a fresh empty directory, with every delivered
    /// record appended to `__records` and every callback invocation counted.
    ///
    /// The observation snapshot is taken inside `observe()`, so anything the
    /// test creates afterwards is a change against an empty tree. `__tick()`
    /// (see `STUBS`) is what stands in for a poll interval elapsing.
    fn observer_case(tag: &str, recursive: bool) -> (V8JsRuntime, std::path::PathBuf) {
        let (rt, dir, id) = opfs_case(tag, true);
        rt.eval(&format!(
            r#"
            var __records = [];
            var __calls = 0;
            var __failed = null;
            var __root = {ROOT}('root', '{id}');
            var __obs = new FileSystemObserver(function(records, observer) {{
              __calls++;
              for (var i = 0; i < records.length; i++) __records.push(records[i]);
            }});
            __obs.observe(__root, {{ recursive: {recursive} }})
              .catch(function(e) {{ __failed = String(e); }});
            "#
        ))
        .unwrap();
        assert_eq!(string_eval(&rt, "String(__failed)"), "null", "observe() rejected");
        (rt, dir)
    }

    /// `record.type` of every record delivered so far, joined — the shape every
    /// test below asserts on.
    fn types_of(rt: &V8JsRuntime) -> String {
        string_eval(rt, "__records.map(function(r) { return r.type; }).join(',')")
    }

    #[test]
    fn observer_interfaces_exist() {
        let rt = with_fsa();
        assert!(bool_eval(&rt, "typeof window.FileSystemObserver === 'function'"));
        assert!(bool_eval(&rt, "typeof window.FileSystemChangeRecord === 'function'"));
    }

    /// The five `typeof … === 'undefined'` lines BUG-389 was filed on, asserted
    /// together: the report is about one API surface, so the thing that says it
    /// is closed is the whole list answering at once, not five scattered tests.
    #[test]
    fn bug_389_symptom_list_is_gone() {
        let rt = with_fsa();
        for expr in [
            "window.FileSystemSyncAccessHandle",
            "window.FileSystemObserver",
            "window.FileSystemHandle",
            "window.FileSystemFileHandle.prototype.createSyncAccessHandle",
            "window.FileSystemFileHandle.prototype.move",
        ] {
            assert!(
                bool_eval(&rt, &format!("typeof {expr} === 'function'")),
                "{expr} is still not a function"
            );
        }
        // The base class is a real ancestor, which is what `instanceof
        // FileSystemHandle` rides on — not just a same-named global.
        assert!(bool_eval(
            &rt,
            "Object.getPrototypeOf(window.FileSystemFileHandle) === window.FileSystemHandle"
        ));
        assert!(bool_eval(
            &rt,
            "Object.getPrototypeOf(window.FileSystemDirectoryHandle) === window.FileSystemHandle"
        ));
    }

    /// The callback is not optional, and neither interface may be built from a
    /// page: `FileSystemChangeRecord` has no constructor at all in FS Observer §2.
    #[test]
    fn observer_construction_is_guarded() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "(function() { try { new FileSystemObserver(); return false; }
                           catch (e) { return e instanceof TypeError; } })()"
        ));
        assert!(bool_eval(
            &rt,
            "(function() { try { new FileSystemObserver(42); return false; }
                           catch (e) { return e instanceof TypeError; } })()"
        ));
        assert!(bool_eval(
            &rt,
            "(function() { try { new FileSystemChangeRecord(); return false; }
                           catch (e) { return e instanceof TypeError; } })()"
        ));
    }

    /// `observe()` rejects on anything that is not a handle rather than throwing
    /// synchronously — it returns a promise, so a bad argument must reject.
    #[test]
    fn observe_rejects_a_non_handle() {
        let rt = with_fsa_for("https://reject.observer.test");
        rt.eval(
            r#"
            var __err = null;
            new FileSystemObserver(function() {}).observe({})
              .then(function() { __err = 'resolved'; },
                    function(e) { __err = e.constructor.name; });
            "#,
        )
        .unwrap();
        assert_eq!(string_eval(&rt, "String(__err)"), "TypeError");
    }

    /// A new file is one `appeared` record, and the record carries the entry it
    /// is about: the relative path, a usable handle, and the observation root.
    #[test]
    fn appeared_record_describes_the_new_entry() {
        let (rt, dir) = observer_case("appeared", false);
        std::fs::write(dir.join("fresh.txt"), b"hi").unwrap();
        rt.eval("__tick();").unwrap();

        assert_eq!(types_of(&rt), "appeared");
        assert_eq!(string_eval(&rt, "__records[0].relativePathComponents.join('/')"), "fresh.txt");
        assert_eq!(string_eval(&rt, "__records[0].changedHandle.kind"), "file");
        assert_eq!(string_eval(&rt, "__records[0].changedHandle.name"), "fresh.txt");
        assert!(bool_eval(&rt, "__records[0].root === __root"));
        assert!(bool_eval(&rt, "__records[0].relativePathMovedFrom === null"));
        // FrozenArray<USVString>: editing a handed-out path must not stick.
        assert!(bool_eval(&rt, "Object.isFrozen(__records[0].relativePathComponents)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nothing changed between two polls is *no* callback, not an empty one —
    /// a page that counted calls would otherwise see a change on every tick.
    #[test]
    fn a_quiet_tick_does_not_call_back() {
        let (rt, dir) = observer_case("quiet", false);
        rt.eval("__tick(); __tick();").unwrap();
        assert_eq!(string_eval(&rt, "String(__calls)"), "0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Rewriting a file is `modified`, deleting it is `disappeared` — and the
    /// snapshot advances each poll, so one change is reported exactly once.
    #[test]
    fn modified_then_disappeared() {
        let (rt, dir) = observer_case("modified", false);
        let file = dir.join("edit.txt");
        std::fs::write(&file, b"a").unwrap();
        rt.eval("__tick();").unwrap();
        std::fs::write(&file, b"much longer body").unwrap();
        rt.eval("__tick();").unwrap();
        std::fs::remove_file(&file).unwrap();
        rt.eval("__tick(); __tick();").unwrap();

        assert_eq!(types_of(&rt), "appeared,modified,disappeared");
        assert_eq!(string_eval(&rt, "__records[2].relativePathComponents.join('/')"), "edit.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A rename inside the observed tree is one `moved` record naming both ends,
    /// not the `disappeared` + `appeared` pair two raw snapshots would suggest.
    #[test]
    fn rename_is_reported_as_moved() {
        let (rt, dir) = observer_case("moved", false);
        std::fs::write(dir.join("before.txt"), b"body").unwrap();
        rt.eval("__tick();").unwrap();
        std::fs::rename(dir.join("before.txt"), dir.join("after.txt")).unwrap();
        rt.eval("__tick();").unwrap();

        assert_eq!(types_of(&rt), "appeared,moved");
        assert_eq!(string_eval(&rt, "__records[1].relativePathComponents.join('/')"), "after.txt");
        assert_eq!(string_eval(&rt, "__records[1].relativePathMovedFrom.join('/')"), "before.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `recursive: false` reports the subdirectory itself and stops there;
    /// `recursive: true` reports what is inside it, with the full relative path.
    #[test]
    fn recursion_decides_how_deep_changes_are_seen() {
        let (flat, flat_dir) = observer_case("flat", false);
        std::fs::create_dir(flat_dir.join("sub")).unwrap();
        std::fs::write(flat_dir.join("sub").join("deep.txt"), b"x").unwrap();
        flat.eval("__tick();").unwrap();
        assert_eq!(types_of(&flat), "appeared");
        assert_eq!(string_eval(&flat, "__records[0].relativePathComponents.join('/')"), "sub");
        assert_eq!(string_eval(&flat, "__records[0].changedHandle.kind"), "directory");
        let _ = std::fs::remove_dir_all(&flat_dir);

        let (deep, deep_dir) = observer_case("deep", true);
        std::fs::create_dir(deep_dir.join("sub")).unwrap();
        std::fs::write(deep_dir.join("sub").join("deep.txt"), b"x").unwrap();
        deep.eval("__tick();").unwrap();
        assert_eq!(types_of(&deep), "appeared,appeared");
        assert_eq!(
            string_eval(&deep, "__records.map(function(r) { return r.relativePathComponents.join('/'); }).sort().join('|')"),
            "sub|sub/deep.txt"
        );
        let _ = std::fs::remove_dir_all(&deep_dir);
    }

    /// `disconnect()` ends every observation the observer holds: a change made
    /// afterwards reaches nobody, however many ticks go by.
    #[test]
    fn disconnect_stops_delivery() {
        let (rt, dir) = observer_case("disconnect", false);
        rt.eval("__obs.disconnect();").unwrap();
        std::fs::write(dir.join("ignored.txt"), b"x").unwrap();
        rt.eval("__tick(); __tick();").unwrap();
        assert_eq!(string_eval(&rt, "String(__calls)"), "0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `unobserve()` takes *an entry*, not the object identity of the handle that
    /// started the observation: a page that re-derived its handle (a fresh grant,
    /// hence a different object and a different token) must still be able to stop.
    #[test]
    fn unobserve_matches_a_re_derived_handle() {
        let (rt, dir) = observer_case("unobserve", false);
        let second = super::dir_reg()
            .lock()
            .unwrap()
            .allocate(dir.clone(), "https://unobserve.opfs.test", true);
        rt.eval(&format!("__obs.unobserve({ROOT}('root', '{second}'));")).unwrap();
        std::fs::write(dir.join("ignored.txt"), b"x").unwrap();
        rt.eval("__tick();").unwrap();
        assert_eq!(string_eval(&rt, "String(__calls)"), "0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Losing the observed root is one `errored` record and then silence — §3
    /// ends the observation rather than reporting each descendant as gone, and
    /// the shim must stop polling a dead observation instead of re-erroring.
    #[test]
    fn a_vanished_root_errors_once_and_ends() {
        let (rt, dir) = observer_case("errored", false);
        std::fs::write(dir.join("doomed.txt"), b"x").unwrap();
        rt.eval("__tick();").unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        rt.eval("__tick(); __tick(); __tick();").unwrap();

        assert_eq!(types_of(&rt), "appeared,errored");
        // `changedHandle` is not nullable: the root is what an `errored` record
        // is about.
        assert!(bool_eval(&rt, "__records[1].changedHandle === __root"));
    }

    /// Re-observing an entry replaces the earlier observation instead of stacking
    /// a second one on it — otherwise one change would arrive twice.
    #[test]
    fn re_observing_replaces_the_observation() {
        let (rt, dir) = observer_case("reobserve", false);
        rt.eval("__obs.observe(__root).catch(function(e) { __failed = String(e); });").unwrap();
        assert_eq!(string_eval(&rt, "String(__failed)"), "null");
        std::fs::write(dir.join("once.txt"), b"x").unwrap();
        rt.eval("__tick();").unwrap();
        assert_eq!(types_of(&rt), "appeared");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A throwing callback is contained: the observation stays live and the next
    /// change is still delivered, because one page's bug must not take down the
    /// tick every other observer's delivery rides on.
    #[test]
    fn a_throwing_callback_does_not_kill_the_observation() {
        let (rt, dir, id) = opfs_case("throwing", true);
        rt.eval(&format!(
            r#"
            var __seen = 0;
            var __root = {ROOT}('root', '{id}');
            var __obs = new FileSystemObserver(function(records) {{
              __seen += records.length;
              throw new Error('from the callback');
            }});
            __obs.observe(__root);
            "#
        ))
        .unwrap();
        std::fs::write(dir.join("one.txt"), b"x").unwrap();
        rt.eval("__tick();").unwrap();
        std::fs::write(dir.join("two.txt"), b"x").unwrap();
        rt.eval("__tick();").unwrap();
        assert_eq!(string_eval(&rt, "String(__seen)"), "2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Installing for a new document drops the previous one's observations, the
    /// same way it drops its grants (BUG-371 point 3) — a page that navigated
    /// away must not keep costing a directory walk on every tick.
    #[test]
    fn install_revokes_the_previous_documents_observations() {
        let (rt, dir, id) = opfs_case("revoke-obs", true);
        rt.eval(&format!(
            r#"
            var __id = _lumen_fs_observe('{id}', '', false);
            "#
        ))
        .unwrap();
        let observation = string_eval(&rt, "String(__id)");
        assert_ne!(observation, "null");
        assert!(super::obs_reg().lock().unwrap().contains_key(&observation));

        super::install_filesystem_access_v8(&rt, "https://revoke-obs.opfs.test").unwrap();
        assert!(!super::obs_reg().lock().unwrap().contains_key(&observation));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Snapshot diffing (the engine under `_lumen_fs_poll_changes`) ─────────

    fn entry(is_dir: bool, len: u64, mtime_ms: u64) -> super::ObsEntry {
        super::ObsEntry { is_dir, len, mtime_ms }
    }

    fn snapshot(items: &[(&str, super::ObsEntry)]) -> std::collections::BTreeMap<Vec<String>, super::ObsEntry> {
        items
            .iter()
            .map(|(path, e)| (path.split('/').map(str::to_string).collect(), e.clone()))
            .collect()
    }

    /// A rename is only *reported* as a move when the poll saw exactly one
    /// departure and one arrival carrying identical metadata. Two unrelated
    /// changes that happen to land in the same tick stay two honest records
    /// rather than one invented `moved`.
    #[test]
    fn move_detection_needs_an_unambiguous_pair() {
        let file = entry(false, 12, 1_700_000_000_000);
        let moved = super::diff_snapshots(&snapshot(&[("a.txt", file.clone())]), &snapshot(&[("b.txt", file.clone())]));
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].kind, "moved");
        assert_eq!(moved[0].moved_from.as_deref(), Some(["a.txt".to_string()].as_slice()));

        // Same shape, but the arrival is a *different* file: `rename` preserves
        // length and mtime, a fresh write does not, so this is not a move.
        let other = entry(false, 34, 1_700_000_000_000);
        let pair = super::diff_snapshots(&snapshot(&[("a.txt", file.clone())]), &snapshot(&[("b.txt", other)]));
        let mut kinds: Vec<&str> = pair.iter().map(|c| c.kind).collect();
        kinds.sort_unstable();
        assert_eq!(kinds, ["appeared", "disappeared"]);

        // Two departures and two arrivals: ambiguous, so four plain records.
        let ambiguous = super::diff_snapshots(
            &snapshot(&[("a.txt", file.clone()), ("b.txt", file.clone())]),
            &snapshot(&[("c.txt", file.clone()), ("d.txt", file.clone())]),
        );
        assert_eq!(ambiguous.len(), 4);
        assert!(ambiguous.iter().all(|c| c.kind != "moved"));
    }

    /// An entry present in both snapshots is `modified` only when something a
    /// *write* changes moved. Re-reading a file touches its access time, which
    /// the snapshot deliberately does not carry.
    #[test]
    fn unchanged_entries_report_nothing() {
        let same = snapshot(&[("keep.txt", entry(false, 7, 42)), ("sub", entry(true, 0, 9))]);
        assert!(super::diff_snapshots(&same, &same).is_empty());

        let grown = snapshot(&[("keep.txt", entry(false, 9, 42)), ("sub", entry(true, 0, 9))]);
        let changes = super::diff_snapshots(&same, &grown);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, "modified");
        assert_eq!(changes[0].path, vec!["keep.txt".to_string()]);
    }

    /// A directory walk stops at [`super::OBS_MAX_DEPTH`], so a pathologically
    /// deep tree cannot cost unbounded stack on a timer tick.
    #[test]
    fn recursive_snapshot_is_depth_capped() {
        let root = std::env::temp_dir().join("lumen_obs_depth");
        let _ = std::fs::remove_dir_all(&root);
        let mut deep = root.clone();
        for _ in 0..(super::OBS_MAX_DEPTH + 5) {
            deep = deep.join("d");
        }
        std::fs::create_dir_all(&deep).unwrap();

        let snap = super::snapshot_tree(&root, false, true).unwrap();
        let deepest = snap.keys().map(Vec::len).max().unwrap_or(0);
        assert!(deepest <= super::OBS_MAX_DEPTH + 1, "walked {deepest} levels deep");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file handle is observed as itself: the snapshot holds exactly the root,
    /// keyed by the empty relative path.
    #[test]
    fn observing_a_file_snapshots_only_that_file() {
        let file = std::env::temp_dir().join("lumen_obs_single.txt");
        std::fs::write(&file, b"body").unwrap();
        let snap = super::snapshot_tree(&file, true, false).unwrap();
        assert_eq!(snap.len(), 1);
        assert!(snap.contains_key(&Vec::<String>::new()));
        // The same path observed as a directory is not observable at all.
        assert!(super::snapshot_tree(&file, false, false).is_none());
        let _ = std::fs::remove_file(&file);
    }
