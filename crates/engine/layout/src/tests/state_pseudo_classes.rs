use super::*;

    // ── :placeholder-shown (CSS Selectors L4 §15.1) ──

    fn first_named(doc: &lumen_dom::Document, root: &LayoutBox, local: &str) -> Color {
        for c in walk_layout(root) {
            if let lumen_dom::NodeData::Element { name, .. } = &doc.get(c.node).data
                && name.local == local
            {
                return c.style.color;
            }
        }
        panic!("element <{local}> not found");
    }

    fn walk_layout(root: &LayoutBox) -> Vec<&LayoutBox> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(b) = stack.pop() {
            out.push(b);
            for c in b.children.iter().rev() {
                stack.push(c);
            }
        }
        out
    }

    #[test]
    fn placeholder_shown_matches_input_with_placeholder() {
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="Name">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn placeholder_shown_no_placeholder_attr_no_match() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn placeholder_shown_whitespace_only_placeholder_no_match() {
        // " " после trim — пустая строка → не матчит.
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="   ">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn placeholder_shown_filled_input_no_match() {
        // value-атрибут с непустым контентом → placeholder скрыт.
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="Name" value="John">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn placeholder_shown_empty_value_still_matches() {
        // value="" — пользователь ничего не ввёл, placeholder виден.
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="Name" value="">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn placeholder_shown_textarea_matches_when_empty() {
        // <textarea> с placeholder и без текстового контента → матчит.
        let (root, doc) = lay_with_doc(
            r#"<textarea placeholder="Bio"></textarea>"#,
            "textarea:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 255);
    }

    #[test]
    fn placeholder_shown_textarea_with_text_does_not_match() {
        // <textarea> с текстом — значение задано через DOM children,
        // placeholder скрыт.
        let (root, doc) = lay_with_doc(
            r#"<textarea placeholder="Bio">My biography</textarea>"#,
            "textarea:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 0);
    }

    #[test]
    fn placeholder_shown_non_form_control_skipped() {
        // <div placeholder="...">x</div> — placeholder не имеет смысла на
        // не-form элементе; pseudo-class не матчит.
        let (root, doc) = lay_with_doc(
            r#"<div placeholder="hint">x</div>"#,
            "div:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 0);
    }

    /// Цвет первого layout-box-а с указанным `id`-атрибутом. `panic!`, если
    /// такого нет. Используется в form-state pseudo тестах, где нужно
    /// различать несколько input-ов в одном документе.
    fn color_by_id(doc: &lumen_dom::Document, root: &LayoutBox, id: &str) -> Color {
        for c in walk_layout(root) {
            if let lumen_dom::NodeData::Element { .. } = &doc.get(c.node).data
                && let Some(v) = doc.get(c.node).get_attr("id")
                && v == id
            {
                return c.style.color;
            }
        }
        panic!("element id={id} not found");
    }

    // ──────────────── :required / :optional ────────────────

    #[test]
    fn required_matches_input_with_required_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input required>"#,
            "input:required { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn required_no_match_without_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:required { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn optional_matches_input_without_required_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:optional { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn optional_no_match_when_required_present() {
        let (root, doc) = lay_with_doc(
            r#"<input required>"#,
            "input:optional { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn required_matches_select_and_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<select id="s" required></select><textarea id="t" required></textarea>"#,
            ":required { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
        assert_eq!(color_by_id(&doc, &root, "t").r, 255);
    }

    #[test]
    fn required_skipped_for_hidden_input() {
        // <input type="hidden"> не поддерживает required (HTML5 §4.10.3).
        let (root, doc) = lay_with_doc(
            r#"<input type="hidden" required>"#,
            "input:required { color: red; } input:optional { color: blue; }",
        );
        let c = first_named(&doc, &root, "input");
        assert_eq!(c.r, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn required_matches_checkbox_radio_file() {
        let (root, doc) = lay_with_doc(
            r#"<input id="c" type="checkbox" required>
               <input id="r" type="radio" required>
               <input id="f" type="file" required>"#,
            ":required { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "c").r, 255);
        assert_eq!(color_by_id(&doc, &root, "r").r, 255);
        assert_eq!(color_by_id(&doc, &root, "f").r, 255);
    }

    #[test]
    fn required_skipped_for_button_and_div() {
        let (root, doc) = lay_with_doc(
            r#"<button id="b" required></button><div id="d" required>x</div>"#,
            ":required { color: red; } :optional { color: blue; }",
        );
        let b = color_by_id(&doc, &root, "b");
        assert_eq!((b.r, b.b), (0, 0), "<button> не имеет required");
        let d = color_by_id(&doc, &root, "d");
        assert_eq!((d.r, d.b), (0, 0), "<div> не имеет required");
    }

    // ──────────────── :read-only / :read-write ────────────────

    #[test]
    fn read_write_matches_plain_input() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn read_only_matches_readonly_input() {
        let (root, doc) = lay_with_doc(
            r#"<input readonly>"#,
            "input:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn read_only_matches_disabled_input() {
        let (root, doc) = lay_with_doc(
            r#"<input disabled>"#,
            "input:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn read_write_matches_plain_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<textarea></textarea>"#,
            "textarea:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 255);
    }

    #[test]
    fn read_only_matches_readonly_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<textarea readonly></textarea>"#,
            "textarea:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 255);
    }

    #[test]
    fn read_only_matches_non_text_input_types() {
        // Не-text-like input types — `:read-only` per HTML5 §4.16.4.
        let (root, doc) = lay_with_doc(
            r#"<input id="h" type="hidden">
               <input id="s" type="submit">
               <input id="r" type="range">
               <input id="c" type="checkbox">"#,
            ":read-only { color: red; } :read-write { color: blue; }",
        );
        assert_eq!(color_by_id(&doc, &root, "h").r, 255);
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
        assert_eq!(color_by_id(&doc, &root, "r").r, 255);
        assert_eq!(color_by_id(&doc, &root, "c").r, 255);
    }

    #[test]
    fn read_write_matches_contenteditable_true() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="true">x</div>"#,
            "div:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_write_matches_contenteditable_empty_attr() {
        // HTML5: contenteditable="" эквивалентно "true".
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable>x</div>"#,
            "div:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_only_matches_contenteditable_false() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="false">x</div>"#,
            "div:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_only_matches_default_div() {
        // Per spec: «matches all other HTML elements» — обычный <div> read-only.
        let (root, doc) = lay_with_doc(
            r#"<div>x</div>"#,
            "div:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_write_inherits_contenteditable_from_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="true"><p id="inner">x</p></div>"#,
            "p:read-write { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "inner").r, 255);
    }

    #[test]
    fn read_only_when_descendant_overrides_to_false() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="true"><p contenteditable="false" id="inner">x</p></div>"#,
            "p:read-only { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "inner").r, 255);
    }

    // ──────────────── :disabled / :enabled ────────────────

    #[test]
    fn disabled_matches_input_with_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input disabled>"#,
            "input:disabled { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn enabled_matches_input_without_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:enabled { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn disabled_matches_button_select_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<button id="b" disabled>x</button>
               <select id="s" disabled></select>
               <textarea id="t" disabled></textarea>"#,
            ":disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
        assert_eq!(color_by_id(&doc, &root, "t").r, 255);
    }

    #[test]
    fn disabled_matches_fieldset_self() {
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled></fieldset>"#,
            "fieldset:disabled { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "fieldset").r, 255);
    }

    #[test]
    fn disabled_inherited_from_fieldset_ancestor() {
        // Inputs внутри <fieldset disabled> вне <legend> — disabled.
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled>
                 <input id="i">
                 <select id="s"></select>
               </fieldset>"#,
            ":disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "i").r, 255);
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
    }

    #[test]
    fn enabled_inside_first_legend_of_disabled_fieldset() {
        // HTML5 §4.10.16: input внутри первого <legend> ребёнка
        // disabled-<fieldset> сохраняет enabled-state.
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled>
                 <legend><input id="legend_input"></legend>
                 <input id="body_input">
               </fieldset>"#,
            ":disabled { color: red; } :enabled { color: blue; }",
        );
        let legend = color_by_id(&doc, &root, "legend_input");
        assert_eq!((legend.r, legend.b), (0, 255), "input в legend остаётся :enabled");
        let body = color_by_id(&doc, &root, "body_input");
        assert_eq!((body.r, body.b), (255, 0), "input вне legend — :disabled");
    }

    #[test]
    fn second_legend_in_disabled_fieldset_still_disabled() {
        // Только ПЕРВЫЙ <legend>-ребёнок «спасает» от disabled. Второй —
        // обычный потомок, попадает под disabled.
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled>
                 <legend>first</legend>
                 <legend><input id="second_legend_input"></legend>
               </fieldset>"#,
            ":disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "second_legend_input").r, 255);
    }

    #[test]
    fn disabled_option_via_optgroup_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<select>
                 <optgroup disabled>
                   <option id="o">x</option>
                 </optgroup>
               </select>"#,
            "option:disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "o").r, 255);
    }

    #[test]
    fn disabled_option_via_own_attr() {
        let (root, doc) = lay_with_doc(
            r#"<select><option id="o" disabled>x</option></select>"#,
            "option:disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "o").r, 255);
    }

    #[test]
    fn disabled_does_not_apply_to_div() {
        // <div disabled> — disabled на не-form элементе игнорируется. Ни
        // :disabled, ни :enabled не матчат.
        let (root, doc) = lay_with_doc(
            r#"<div disabled>x</div>"#,
            ":disabled { color: red; } :enabled { color: blue; }",
        );
        let c = first_named(&doc, &root, "div");
        assert_eq!((c.r, c.b), (0, 0));
    }

    // ──────────────── :checked / :indeterminate / :default ────────────────

    #[test]
    fn checked_matches_checkbox_with_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox" checked>"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn checked_matches_checkbox_empty_attr_value() {
        // checked="" — атрибут присутствует, значение спецификацией не
        // используется (HTML5 §2.4.2 boolean attribute).
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox" checked="">"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn checked_no_match_without_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox">"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn checked_matches_radio_with_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input type="radio" checked>"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn checked_does_not_match_text_input() {
        // text-input с атрибутом `checked` — атрибут не имеет смысла,
        // :checked не матчит.
        let (root, doc) = lay_with_doc(
            r#"<input type="text" checked>"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn checked_matches_option_with_selected() {
        let (root, doc) = lay_with_doc(
            r#"<select><option id="a">a</option><option id="b" selected>b</option></select>"#,
            "option:checked { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
    }

    #[test]
    fn checked_does_not_match_div() {
        let (root, doc) = lay_with_doc(
            r#"<div checked>x</div>"#,
            ":checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 0);
    }

    #[test]
    fn indeterminate_radio_group_no_checked() {
        // Группа из двух radio с одинаковым name, ни один не checked →
        // оба :indeterminate.
        let (root, doc) = lay_with_doc(
            r#"<form><input type="radio" name="g" id="a"><input type="radio" name="g" id="b"></form>"#,
            "input:indeterminate { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
    }

    #[test]
    fn indeterminate_radio_group_one_checked_no_match() {
        // Один из группы checked → оба НЕ :indeterminate.
        let (root, doc) = lay_with_doc(
            r#"<form><input type="radio" name="g" id="a" checked><input type="radio" name="g" id="b"></form>"#,
            "input:indeterminate { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
    }

    #[test]
    fn indeterminate_radio_distinct_groups_isolated() {
        // Две группы с разным `name`: checked в одной не влияет на другую.
        let (root, doc) = lay_with_doc(
            r#"<form><input type="radio" name="g1" id="a" checked><input type="radio" name="g2" id="b"></form>"#,
            "input:indeterminate { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
    }

    #[test]
    fn indeterminate_checkbox_never_in_phase_0() {
        // Phase 0 без runtime: атрибут indeterminate (если бы такой существовал)
        // не передаёт DOM-флаг; checkbox всегда вне :indeterminate.
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox">"#,
            "input:indeterminate { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn indeterminate_progress_without_value() {
        // <progress> без атрибута value → indeterminate progress.
        let (root, doc) = lay_with_doc(
            r#"<progress></progress>"#,
            "progress:indeterminate { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "progress").r, 255);
    }

    #[test]
    fn indeterminate_progress_with_value_no_match() {
        let (root, doc) = lay_with_doc(
            r#"<progress value="0.5"></progress>"#,
            "progress:indeterminate { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "progress").r, 0);
    }

    #[test]
    fn default_matches_option_with_selected() {
        let (root, doc) = lay_with_doc(
            r#"<select><option id="a">a</option><option id="b" selected>b</option></select>"#,
            "option:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
    }

    #[test]
    fn default_matches_checked_checkbox() {
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox" checked>"#,
            "input:default { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn default_matches_first_submit_button_of_form() {
        // Первая submit-кнопка в DOM-порядке формы — default-submit.
        let (root, doc) = lay_with_doc(
            r#"<form><button id="a" type="submit">A</button><button id="b" type="submit">B</button></form>"#,
            "button:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
    }

    #[test]
    fn default_matches_button_without_type_attr() {
        // <button> без `type` имеет default type=submit (HTML5 §4.10.8).
        let (root, doc) = lay_with_doc(
            r#"<form><button id="a">go</button></form>"#,
            "button:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
    }

    #[test]
    fn default_matches_input_type_submit() {
        let (root, doc) = lay_with_doc(
            r#"<form><input id="a" type="submit"></form>"#,
            "input:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
    }

    #[test]
    fn default_no_match_for_submit_button_outside_form() {
        // Без <form>-предка submit-кнопка не считается default-submit.
        let (root, doc) = lay_with_doc(
            r#"<button id="a" type="submit">go</button>"#,
            "button:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
    }

    #[test]
    fn default_button_type_button_no_match() {
        // type=button — не submit, не default.
        let (root, doc) = lay_with_doc(
            r#"<form><button id="a" type="button">x</button></form>"#,
            "button:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
    }

    // ──────────────── :lang(...) (CSS Selectors L4 §11) ────────────────

    #[test]
    fn lang_matches_self_lang_attr() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="en">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_matches_prefix_with_region() {
        // RFC 4647 basic filtering: range "en" matches tag "en-US".
        let (root, doc) = lay_with_doc(
            r#"<p lang="en-US">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_no_match_different_prefix() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="fr">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn lang_no_match_substring_not_prefix() {
        // "en" не должен матчить "fr-en" — `en` здесь регион, не язык.
        let (root, doc) = lay_with_doc(
            r#"<p lang="fr-en">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn lang_inherited_from_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<div lang="ru"><p>x</p></div>"#,
            "p:lang(ru) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_case_insensitive_match() {
        // BCP 47: language tags case-insensitive. lang="EN-us" matches :lang(en).
        let (root, doc) = lay_with_doc(
            r#"<p lang="EN-us">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_comma_list_any_matches() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="fr">x</p>"#,
            "p:lang(en, fr, ru) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_no_match_when_no_lang_attr() {
        // Ни один ancestor не имеет lang → элемент без языка → не матчит.
        let (root, doc) = lay_with_doc(
            r#"<p>x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn lang_empty_attr_treated_as_no_language() {
        // <p lang=""> — HTML5 «явно неизвестен», не наследует, не матчит.
        let (root, doc) = lay_with_doc(
            r#"<div lang="ru"><p lang="">x</p></div>"#,
            "p:lang(ru) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn lang_xml_lang_fallback() {
        // xml:lang атрибут используется как fallback (XHTML legacy).
        let (root, doc) = lay_with_doc(
            r#"<p xml:lang="ja">x</p>"#,
            "p:lang(ja) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_nearest_ancestor_wins() {
        // Внутренний `lang` overrideит ancestor: внутри `lang="ru"`, p имеет
        // `lang="en"` → matches en, не ru.
        let (root, doc) = lay_with_doc(
            r#"<div lang="ru"><p lang="en">x</p></div>"#,
            "p:lang(ru) { color: red; } p:lang(en) { color: blue; }",
        );
        let c = first_named(&doc, &root, "p");
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :dir(ltr|rtl) (CSS Selectors L4 §13.2) ────────────────

    #[test]
    fn dir_ltr_matches_by_default() {
        // Без `dir`-атрибута — default ltr (HTML5 §3.2.6.1).
        let (root, doc) = lay_with_doc(
            r#"<p>x</p>"#,
            "p:dir(ltr) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_rtl_does_not_match_by_default() {
        let (root, doc) = lay_with_doc(
            r#"<p>x</p>"#,
            "p:dir(rtl) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn dir_rtl_matches_when_attr_set() {
        let (root, doc) = lay_with_doc(
            r#"<p dir="rtl">x</p>"#,
            "p:dir(rtl) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_rtl_inherited_from_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<div dir="rtl"><p>x</p></div>"#,
            "p:dir(rtl) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_nearest_ancestor_wins() {
        // Внутренний `dir="ltr"` overrideит ancestor `dir="rtl"`.
        let (root, doc) = lay_with_doc(
            r#"<div dir="rtl"><p dir="ltr">x</p></div>"#,
            "p:dir(rtl) { color: red; } p:dir(ltr) { color: blue; }",
        );
        let c = first_named(&doc, &root, "p");
        assert_eq!((c.r, c.b), (0, 255));
    }

    #[test]
    fn dir_attr_case_insensitive() {
        let (root, doc) = lay_with_doc(
            r#"<p dir="RTL">x</p>"#,
            "p:dir(rtl) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_auto_treated_as_ltr_in_phase_0() {
        // `dir="auto"` в Phase 0 без bidi-движка трактуется как ltr.
        let (root, doc) = lay_with_doc(
            r#"<p dir="auto">x</p>"#,
            "p:dir(ltr) { color: red; } p:dir(rtl) { color: blue; }",
        );
        let c = first_named(&doc, &root, "p");
        assert_eq!((c.r, c.b), (255, 0));
    }

    #[test]
    fn dir_invalid_value_treated_as_ltr() {
        // `dir="invalid"` — fallback на ltr (как и `auto`).
        let (root, doc) = lay_with_doc(
            r#"<p dir="invalid">x</p>"#,
            "p:dir(ltr) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_auto_finalizes_directionality_does_not_inherit() {
        // `dir="auto"` на самом элементе — финализирует direction (Phase 0:
        // ltr); ancestor `dir="rtl"` НЕ должен пробить — атрибут на элементе
        // имеет приоритет, даже если значение `auto`.
        let (root, doc) = lay_with_doc(
            r#"<div dir="rtl"><p dir="auto">x</p></div>"#,
            "p:dir(rtl) { color: red; } p:dir(ltr) { color: blue; }",
        );
        let c = first_named(&doc, &root, "p");
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :link / :visited / :any-link (CSS Selectors L4 §6.2) ────────────────

    /// Computes color для первого element-child указанного тега в DOM (без
    /// layout-tree, чтобы тесты ловили inline-элементы вроде `<a>` / `<area>`
    /// / `<link>` независимо от того, попадают они в LayoutBox или нет).
    pub(crate) fn element_color(html: &str, css: &str, tag: &str) -> Color {
        use crate::style::compute_style;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_style = ComputedStyle::root();
        let target = find_first_element(&doc, doc.root(), tag).expect("element not found");
        compute_style(&doc, target, &sheet, &root_style, Size::new(800.0, 600.0), false).color
    }

    fn find_first_element(
        doc: &lumen_dom::Document,
        node: lumen_dom::NodeId,
        tag: &str,
    ) -> Option<lumen_dom::NodeId> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(node).data
            && name.local == tag
        {
            return Some(node);
        }
        for &child in &doc.get(node).children {
            if let Some(found) = find_first_element(doc, child, tag) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn any_link_matches_a_with_href() {
        let c = element_color(
            r#"<a href="https://example.com">x</a>"#,
            "a:any-link { color: red; }",
            "a",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn any_link_does_not_match_a_without_href() {
        // <a> без href — не hyperlink (HTML5 §4.6.1).
        let c = element_color(
            r#"<a>x</a>"#,
            "a:any-link { color: red; }",
            "a",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn any_link_matches_area_with_href() {
        // `<area>` внутри `<map>` — image-map link.
        let c = element_color(
            r##"<map><area href="#x"></map>"##,
            "area:any-link { color: red; }",
            "area",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn any_link_matches_link_with_href() {
        let c = element_color(
            r#"<link href="style.css" rel="stylesheet">"#,
            "link:any-link { color: red; }",
            "link",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn link_pseudo_matches_a_with_href_in_phase_0() {
        // В Phase 0 без visited-runtime `:link` эквивалентен `:any-link`.
        let c = element_color(
            r#"<a href="x">a</a>"#,
            "a:link { color: red; }",
            "a",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn link_pseudo_does_not_match_without_href() {
        let c = element_color(
            r#"<a>x</a>"#,
            "a:link { color: red; }",
            "a",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn visited_pseudo_never_matches_in_phase_0() {
        // Phase 0 без history-runtime — никакая ссылка не считается посещённой.
        // Безопасный default per privacy-by-default.
        let c = element_color(
            r#"<a href="x">a</a>"#,
            "a:visited { color: red; }",
            "a",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn link_pseudos_do_not_match_div_with_href() {
        // `href` на не-hyperlink-элементе игнорируется (только a/area/link).
        let c = element_color(
            r#"<div href="x">x</div>"#,
            ":any-link { color: red; } :link { color: blue; }",
            "div",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn any_link_specificity_class_level() {
        // `:any-link` имеет specificity class-уровня (0,1,0). Equal-specificity
        // — более позднее правило выигрывает (source-order).
        let c = element_color(
            r#"<a href="x">a</a>"#,
            "a:any-link { color: red; } a:link { color: blue; }",
            "a",
        );
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :scope (CSS Selectors L4 §4.2) ────────────────

    #[test]
    fn scope_matches_root_element() {
        // В author-CSS без querySelector-runtime `:scope` matches document
        // root element (эквивалентно `:root`).
        let c = element_color(
            "<html><body><p>x</p></body></html>",
            ":scope { color: red; }",
            "html",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn scope_does_not_match_descendants() {
        // `:scope` matches root only, не вложенные элементы.
        let c = element_color(
            "<html><body><p>x</p></body></html>",
            ":scope { color: red; }",
            "body",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn scope_equivalent_to_root_in_author_css() {
        // В author-CSS без runtime querySelector `:scope` и `:root` дают
        // одинаковый результат — оба matches root element.
        let c1 = element_color(
            "<html><body>x</body></html>",
            ":scope { color: red; }",
            "html",
        );
        let c2 = element_color(
            "<html><body>x</body></html>",
            ":root { color: red; }",
            "html",
        );
        assert_eq!(c1.r, c2.r);
    }

    // ──────────────── :target (CSS Selectors L4 §9.6) ────────────────

    /// Computes color для первого element-child указанного тега с указанным
    /// target_id, выставленным в Document перед каскадом. Эквивалент
    /// `element_color`, но с `Document::set_target(...)`.
    fn element_color_with_target(
        html: &str,
        css: &str,
        tag: &str,
        target: Option<&str>,
    ) -> Color {
        use crate::style::compute_style;
        let mut doc = lumen_html_parser::parse(html);
        doc.set_target(target);
        let sheet = lumen_css_parser::parse(css);
        let root_style = ComputedStyle::root();
        let target_node = find_first_element(&doc, doc.root(), tag).expect("element not found");
        compute_style(&doc, target_node, &sheet, &root_style, Size::new(800.0, 600.0), false).color
    }

    #[test]
    fn target_matches_element_with_matching_id() {
        let c = element_color_with_target(
            r#"<html><body><h2 id="intro">x</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            Some("intro"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_does_not_match_other_elements() {
        // Только element с совпадающим id матчит — sibling с другим id нет.
        let c = element_color_with_target(
            r#"<html><body><h2 id="intro">x</h2><h2 id="other">y</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            Some("other"),
        );
        // Первый h2 (id="intro") — не матчит, color остаётся default (black).
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_returns_false_when_no_fragment() {
        // Document::target() == None — никакой element не матчит.
        let c = element_color_with_target(
            r#"<html><body><h2 id="intro">x</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            None,
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_returns_false_for_empty_fragment() {
        // Пустой fragment («#» в URL) трактуется как None — Document::set_target
        // фильтрует empty string. Поведение совпадает с major-браузерами.
        let c = element_color_with_target(
            r#"<html><body><h2 id="">x</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            Some(""),
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_is_case_sensitive() {
        // HTML id case-sensitive (HTML LS §3.2.6) — `Intro` != `intro`.
        let c = element_color_with_target(
            r#"<html><body><h2 id="Intro">x</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            Some("intro"),
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_compound_with_type() {
        // `h2:target` — compound selector с type matcher-ом.
        let c = element_color_with_target(
            r#"<html><body><h2 id="t">x</h2></body></html>"#,
            "h2:target { color: red; }",
            "h2",
            Some("t"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_specificity_pseudo_class_level() {
        // `:target` имеет specificity (0,1,0) — class-уровень. Equal-specificity
        // — выигрывает более позднее правило (source-order).
        let c = element_color_with_target(
            r#"<html><body><h2 id="t" class="c">x</h2></body></html>"#,
            "h2.c { color: red; } h2:target { color: blue; }",
            "h2",
            Some("t"),
        );
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :target-within (CSS Selectors L4 §9.7) ────────────────

    #[test]
    fn target_within_matches_target_element_itself() {
        // Element, который сам :target, также матчит :target-within
        // (spec: «matches elements that are themselves matching :target or
        // that have a descendant which matches»).
        let c = element_color_with_target(
            r#"<html><body><h2 id="t">x</h2></body></html>"#,
            ":target-within { color: red; }",
            "h2",
            Some("t"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_within_matches_ancestor_of_target() {
        // `<section>` сам не :target, но contains `<h2 id="t">` — матчит.
        let c = element_color_with_target(
            r#"<html><body><section><h2 id="t">x</h2></section></body></html>"#,
            "section:target-within { color: red; }",
            "section",
            Some("t"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_within_matches_distant_ancestor() {
        // `<body>` глубоко выше `<h2 id="t">` — всё равно матчит (любой
        // descendant — не только прямой ребёнок).
        let c = element_color_with_target(
            r#"<html><body><div><section><h2 id="t">x</h2></section></div></body></html>"#,
            "body:target-within { color: red; }",
            "body",
            Some("t"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_within_does_not_match_sibling() {
        // Sibling рядом с target-ом не матчит — `:target-within` не bubble-ит
        // через parent наверх (только subtree containment).
        let c = element_color_with_target(
            r#"<html><body><h2 id="t">x</h2><p>sibling</p></body></html>"#,
            "p:target-within { color: red; }",
            "p",
            Some("t"),
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_within_returns_false_when_no_fragment() {
        // Без `Document::target()` matcher всегда false — даже для элементов
        // с descendant-ами, имеющими этот id.
        let c = element_color_with_target(
            r#"<html><body><h2 id="t">x</h2></body></html>"#,
            "body:target-within { color: red; }",
            "body",
            None,
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_within_does_not_match_unrelated_element() {
        // Element без target-descendant и не target сам — false.
        let c = element_color_with_target(
            r#"<html><body><section><h2 id="t">x</h2></section><aside>y</aside></body></html>"#,
            "aside:target-within { color: red; }",
            "aside",
            Some("t"),
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_within_specificity_pseudo_class_level() {
        // `:target-within` — specificity (0,1,0); equal-specificity tie-break
        // by source-order.
        let c = element_color_with_target(
            r#"<html><body><section class="c"><h2 id="t">x</h2></section></body></html>"#,
            "section.c { color: red; } section:target-within { color: blue; }",
            "section",
            Some("t"),
        );
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :in-range / :out-of-range (CSS Selectors L4 §14.5) ────────────────

    #[test]
    fn in_range_number_value_within_min_max() {
        let c = element_color(
            r#"<input type="number" min="1" max="10" value="5">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn out_of_range_number_value_above_max() {
        let c = element_color(
            r#"<input type="number" min="1" max="10" value="15">"#,
            "input:out-of-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn out_of_range_number_value_below_min() {
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="-5">"#,
            "input:out-of-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn in_range_value_equals_max_endpoint() {
        // Spec §4.10.21.4: «greater than max» = strict. Value == max → in-range.
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="10">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn in_range_only_min_attribute() {
        // Range exists даже если только min — :in-range / :out-of-range
        // зависят от значения (max = +∞).
        let c = element_color(
            r#"<input type="number" min="0" value="100">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn out_of_range_only_min_attribute_value_below() {
        let c = element_color(
            r#"<input type="number" min="0" value="-1">"#,
            "input:out-of-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn neither_when_no_min_no_max() {
        // Нет range-limitations → не матчит ни одну pseudo.
        let c = element_color(
            r#"<input type="number" value="5">"#,
            "input:in-range { color: red; } input:out-of-range { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn neither_when_value_missing() {
        // Нет displayed value (для number) → не матчит ни одну.
        let c = element_color(
            r#"<input type="number" min="1" max="10">"#,
            "input:in-range { color: red; } input:out-of-range { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn neither_when_value_invalid() {
        // Невалидное value → нет displayed numeric value → не матчит.
        let c = element_color(
            r#"<input type="number" min="1" max="10" value="abc">"#,
            "input:in-range { color: red; } input:out-of-range { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn in_range_text_input_skipped() {
        // type=text не поддерживает range — :in-range не матчит даже если
        // min/max выставлены.
        let c = element_color(
            r#"<input type="text" min="1" max="10" value="5">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn in_range_textarea_skipped() {
        // <textarea> не имеет range-checks.
        let c = element_color(
            r#"<textarea min="1" max="10">5</textarea>"#,
            "textarea:in-range { color: red; }",
            "textarea",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn in_range_range_input_default_min_max() {
        // type=range без атрибутов: дефолтный диапазон [0, 100], default
        // value = середина = 50 → :in-range.
        let c = element_color(
            r#"<input type="range">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn out_of_range_range_input_value_above_max() {
        let c = element_color(
            r#"<input type="range" min="0" max="100" value="150">"#,
            "input:out-of-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn in_range_fractional_number() {
        // Дробные значения должны парситься как f64.
        let c = element_color(
            r#"<input type="number" min="1.5" max="2.5" value="2.0">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn neither_for_date_type_phase_0() {
        // Phase 0: date / month / week / time / datetime-local пока не
        // поддерживаются — pseudo не матчит (см. doc к matches_in_range).
        let c = element_color(
            r#"<input type="date" min="2025-01-01" max="2025-12-31" value="2025-06-15">"#,
            "input:in-range { color: red; } input:out-of-range { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn in_range_specificity_is_class_level() {
        // pseudo-class contributes (0, 1, 0) к specificity. Type + pseudo
        // (0,1,1) > type-only (0,0,1) — правило с pseudo выигрывает несмотря
        // на DOM source-order.
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="5">"#,
            "input:in-range { color: red; } input { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (255, 0));
    }

    // ──────────────── :valid / :invalid ────────────────

    #[test]
    fn valid_matches_non_required_input() {
        // Без required — value не может быть missing, элемент valid.
        let c = element_color(
            r#"<input type="text">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid должен матчить input без required");
    }

    #[test]
    fn invalid_matches_required_input_without_value() {
        let c = element_color(
            r#"<input type="text" required>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — required + нет value");
    }

    #[test]
    fn valid_matches_required_input_with_value() {
        let c = element_color(
            r#"<input type="text" required value="hello">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — required + value присутствует");
    }

    #[test]
    fn invalid_email_typemismatch() {
        let c = element_color(
            r#"<input type="email" value="notanemail">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — email без @");
    }

    #[test]
    fn valid_email_with_at_and_domain() {
        let c = element_color(
            r#"<input type="email" value="user@example.com">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — корректный email");
    }

    #[test]
    fn valid_email_empty_value_not_required() {
        // Пустой value при отсутствии required — valid.
        let c = element_color(
            r#"<input type="email">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — пустой email без required");
    }

    #[test]
    fn invalid_url_typemismatch() {
        let c = element_color(
            r#"<input type="url" value="not-a-url">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — url без схемы");
    }

    #[test]
    fn valid_url_with_scheme() {
        let c = element_color(
            r#"<input type="url" value="https://example.com">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — корректный url");
    }

    #[test]
    fn invalid_number_out_of_range() {
        // :invalid покрывает rangeOverflow так же, как :out-of-range.
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="99">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — out-of-range number");
    }

    #[test]
    fn valid_number_within_range() {
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="5">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — number in range");
    }

    #[test]
    fn valid_invalid_not_match_div() {
        // :valid/:invalid не применимы к не-form-control элементам.
        let c = element_color(
            r#"<div>x</div>"#,
            "div:valid { color: green; } div:invalid { color: red; }",
            "div",
        );
        assert_eq!((c.r, c.g), (0, 0), ":valid/:invalid не матчат <div>");
    }

    #[test]
    fn valid_invalid_not_match_hidden_input() {
        // <input type="hidden"> не является кандидатом для constraint validation.
        let c = element_color(
            r#"<input type="hidden" required>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 0), "hidden input — не матчит ни :valid, ни :invalid");
    }

    #[test]
    fn valid_invalid_not_match_disabled_input() {
        // Disabled — barred from constraint validation.
        let c = element_color(
            r#"<input type="text" required disabled>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 0), "disabled input — не матчит ни :valid, ни :invalid");
    }

    #[test]
    fn invalid_required_checkbox_unchecked() {
        let c = element_color(
            r#"<input type="checkbox" required>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — required checkbox без checked");
    }

    #[test]
    fn valid_required_checkbox_checked() {
        let c = element_color(
            r#"<input type="checkbox" required checked>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — required checkbox с checked");
    }

    #[test]
    fn valid_required_textarea_with_value() {
        let c = element_color(
            r#"<textarea required>hello</textarea>"#,
            "textarea:valid { color: green; } textarea:invalid { color: red; }",
            "textarea",
        );
        // textarea: значение в content, не в value-атрибуте — Phase 0: смотрим
        // только value-атрибут, потому элемент valid при его отсутствии.
        assert_eq!((c.r, c.g), (0, 128), ":valid — textarea без value-атрибута при required");
    }

    #[test]
    fn user_valid_user_invalid_always_false() {
        // Phase 0: без интерактивного состояния :user-valid/:user-invalid = false.
        let c = element_color(
            r#"<input type="text">"#,
            "input:user-valid { color: green; } input:user-invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 0), ":user-valid/:user-invalid always false в Phase 0");
    }

    #[test]
    fn id_wins_over_class() {
        // id specificity (1,0,0) > class (0,1,0). Порядок правил в CSS — class
        // после id — не должен пересилить.
        let root = lay(
            r#"<p id="x" class="c">v</p>"#,
            "#x { color: red; } .c { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255, "id should win over class");
        assert_eq!(p.style.color.b, 0);
    }

    #[test]
    fn class_wins_over_type() {
        // class (0,1,0) > type (0,0,1). Type идёт после в порядке — но проиграет.
        let root = lay(r#"<p class="c">v</p>"#, ".c { color: red; } p { color: blue; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    #[test]
    fn equal_specificity_last_wins() {
        let root = lay("<p>v</p>", "p { color: red; } p { color: blue; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255);
    }

    // ── Тесты inline-flow ───────────────────────────────────────────────────

    /// <span> внутри <p> не разрывает строку: высота = одна линия.
    #[test]
    fn inline_span_does_not_break_line() {
        let root = lay_measured("<p>hello <span>world</span></p>", "", 800.0);
        // "hello world" = 11 слов × 8px = 88px; при 800px — одна строка.
        assert!(
            (root.rect.height - 19.2).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// <a> получает цвет из CSS, текст соседнего текстового узла — родительский.
    #[test]
    fn inline_link_inherits_own_color() {
        let root = lay("<p>text <a>link</a></p>", "a { color: blue; }");
        let p = first_element_child(&root);
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { segments, .. } = &inline.kind {
            // Первый сегмент — текстовый узел "text " (наследует цвет <p>)
            assert_eq!(segments[0].style.color.b, 0, "text node must not be blue");
            // Второй сегмент — текст внутри <a> (синий)
            assert_eq!(segments[1].style.color.b, 255, "link must be blue");
        } else {
            panic!("expected InlineRun");
        }
    }

    /// Inline-ран переносится так же, как обычный текст.
    #[test]
    fn inline_run_wraps_across_viewport() {
        // "aa bb" = 5 × 8 = 40px при Fixed8. Viewport 30px → перенос после "aa".
        let root = lay_measured("<p>aa <em>bb</em></p>", "", 30.0);
        // 2 строки × 19.2 = 38.4
        assert!(
            (root.rect.height - 38.4).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// Блочные элементы между inline-контентом не смешиваются в один InlineRun.
    #[test]
    fn block_between_inline_creates_separate_run() {
        // <div> — блочный элемент; текст до и после — разные InlineRun-ы.
        let root = lay("<p>before</p><div>mid</div><p>after</p>", "");
        // 3 блока по 19.2 = 57.6
        assert!(
            (root.rect.height - 57.6).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// BUG-013: display:none между inline-элементами не должен разрывать InlineRun.
    /// До фикса: `<span style="display:none">` вызывал break, и соседние <span>
    /// попадали в разные строки, удваивая высоту параграфа.
    #[test]
    fn display_none_does_not_break_inline_context() {
        // Три <span>: первый и третий видимые, второй — display:none.
        // Ожидание: все три в одном inline-контексте → высота = одна строка (19.2).
        let root = lay_measured(
            "<p><span>hello</span><span style=\"display:none\">x</span><span>world</span></p>",
            "",
            800.0,
        );
        assert!(
            (root.rect.height - 19.2).abs() < 0.5,
            "display:none разрывает inline-контекст: height={} (ожидалось 19.2)",
            root.rect.height,
        );
    }

    // ── Функциональные pseudo: :nth-*, :*-of-type, :not ───────────────────

    /// Собирает все элементы с тегом `tag` из children корневого LayoutBox.
    fn block_children_by_tag<'a>(
        root: &'a LayoutBox,
        doc: &lumen_dom::Document,
        tag: &str,
    ) -> Vec<&'a LayoutBox> {
        root.children
            .iter()
            .filter(|c| {
                matches!(
                    &doc.get(c.node).data,
                    lumen_dom::NodeData::Element { name, .. } if name.local == tag
                )
            })
            .collect()
    }

    #[test]
    fn nth_child_odd_matches_1_3_5() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p><p>d</p><p>e</p>",
            "p:nth-child(odd) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps.len(), 5);
        for (i, p) in ps.iter().enumerate() {
            let one_based = (i + 1) as i32;
            let expected_red = one_based % 2 == 1;
            assert_eq!(
                p.style.color.r == 255,
                expected_red,
                "index={one_based}"
            );
        }
    }

    #[test]
    fn nth_child_specific_index() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-child(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn nth_child_formula_2n() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p><p>d</p>",
            "p:nth-child(2n) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // 2n: 2, 4, ...
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
        assert_eq!(ps[3].style.color.r, 255);
    }

    #[test]
    fn nth_last_child_matches_from_end() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-last-child(1) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // Последний матчит.
        assert_eq!(ps[2].style.color.r, 255);
        assert_eq!(ps[0].style.color.r, 0);
    }

    #[test]
    fn nth_of_type_counts_only_matching_tag() {
        // <h1><p1><h2><p2><p3> — :nth-of-type(2) для p должен попасть в p2.
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><p>p1</p><h2>x</h2><p>p2</p><p>p3</p>",
            "p:nth-of-type(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // p1 — это of-type index 1 → 0, p2 → 2 → 255, p3 → 3 → 0.
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn nth_child_of_selector_filters_pool() {
        // CSS Selectors L4 §6.6.5.1: `:nth-child(odd of .v)` нумерует ТОЛЬКО
        // элементы с классом `v`, остальные siblings не участвуют. Из
        // .v#a (index 1), .v#b (2), .v#c (3) — odd = a и c.
        let (root, doc) = lay_with_doc(
            r#"<p>x</p><p class="v" id="a">x</p><p>x</p><p class="v" id="b">x</p><p class="v" id="c">x</p>"#,
            "p:nth-child(odd of .v) { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 255);
    }

    #[test]
    fn nth_child_of_selector_does_not_match_non_filtered() {
        // Элемент, не матчащий of-selector, никогда не матчит pseudo —
        // независимо от того, какой у него index среди ВСЕХ siblings.
        let (root, doc) = lay_with_doc(
            r#"<p class="v" id="a">x</p><p id="b">x</p><p class="v" id="c">x</p>"#,
            "p:nth-child(1 of .v) { color: red; }",
        );
        // .v#a — первый матчащий .v → matches.
        // #b — не .v, не матчит вообще.
        // .v#c — второй матчащий .v → не matches 1.
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 0);
    }

    #[test]
    fn nth_last_child_of_selector_filters_from_end() {
        let (root, doc) = lay_with_doc(
            r#"<p class="v" id="a">x</p><p class="v" id="b">x</p><p id="c">x</p><p class="v" id="d">x</p>"#,
            "p:nth-last-child(1 of .v) { color: red; }",
        );
        // С конца: первый .v — d (matches), второй .v — b (no), третий — a (no).
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 0);
        assert_eq!(color_by_id(&doc, &root, "d").r, 255);
    }

    #[test]
    fn nth_child_of_selector_list_union() {
        // of-clause принимает selector-list через запятую: соответствие
        // хотя бы одному → элемент в pool.
        let (root, doc) = lay_with_doc(
            r#"<p class="x" id="a">x</p><p id="b">x</p><p class="y" id="c">x</p><p class="x" id="d">x</p>"#,
            "p:nth-child(odd of .x, .y) { color: red; }",
        );
        // Pool по «.x OR .y»: a, c, d. odd-index в этом pool: a(1), d(3).
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 0);
        assert_eq!(color_by_id(&doc, &root, "d").r, 255);
    }

    #[test]
    fn nth_child_backward_compat_without_of() {
        // Базовое поведение без of-clause не должно регрессировать.
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-child(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn first_of_type_matches() {
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><p>p1</p><p>p2</p>",
            "p:first-of-type { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn last_of_type_matches() {
        let (root, doc) = lay_with_doc(
            "<p>p1</p><p>p2</p><h1>x</h1>",
            "p:last-of-type { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        // p2 — последний `<p>` (h1 после него — другой тип), значит матчит.
        assert_eq!(ps[1].style.color.r, 255);
    }

    #[test]
    fn not_class_excludes() {
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p><p>c</p>"#,
            "p:not(.hl) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "a should match");
        assert_eq!(ps[1].style.color.r, 0, "b.hl should NOT match");
        assert_eq!(ps[2].style.color.r, 255, "c should match");
    }

    #[test]
    fn not_with_compound_excludes_full() {
        // :not(p.hl) — исключает только p с классом hl, не любой <p> и не любой `.hl`.
        // Используем scope через body-класс чтобы не загрязнять html/body.
        let (root, doc) = lay_with_doc(
            r#"<body class="t"><p>x</p><p class="hl">y</p><div class="hl">z</div></body>"#,
            "body.t *:not(p.hl) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        let divs = block_children_by_tag(&root, &doc, "div");
        assert_eq!(ps[0].style.color.r, 255, "p без класса — матчит");
        assert_eq!(ps[1].style.color.r, 0, "p.hl — исключается");
        assert_eq!(divs[0].style.color.r, 255, "div.hl — не исключается");
    }

    #[test]
    fn not_selector_list_l4() {
        // CSS Selectors L4 §5.4: список селекторов внутри `:not(...)` —
        // элемент исключается, если матчит ХОТЯ БЫ ОДИН селектор списка.
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p><p id="x">c</p><p>d</p>"#,
            "p:not(.hl, #x) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "a — матчит");
        assert_eq!(ps[1].style.color.r, 0, "b.hl — исключается");
        assert_eq!(ps[2].style.color.r, 0, "c#x — исключается");
        assert_eq!(ps[3].style.color.r, 255, "d — матчит");
    }

    #[test]
    fn not_complex_with_descendant_combinator_l4() {
        // CSS Selectors L4 §5.4: combinator-ы внутри `:not` разрешены.
        // Исключаем <p>, у которых внутри (descendant) есть <a>.
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p>b <a>link</a></p><p>c</p>"#,
            "p:not(:has(a)) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "p без <a> — матчит");
        assert_eq!(ps[1].style.color.r, 0, "p с <a> — исключается");
        assert_eq!(ps[2].style.color.r, 255, "p без <a> — матчит");
    }

    #[test]
    fn not_nested_double_negation_l4() {
        // CSS Selectors L4 §5.4: nested `:not(:not(...))` разрешён.
        // `:not(:not(.hl))` ≡ `.hl` (двойное отрицание).
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p>"#,
            "p:not(:not(.hl)) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0, "a (нет .hl) — не матчит");
        assert_eq!(ps[1].style.color.r, 255, "b.hl — матчит (двойное :not)");
    }

