//! Тесты привязки модели к панелям хрома: omnibox, find bar, загрузки,
//! архив, popover, оверлеи, настройки, история и закладки.

use super::*;

#[test]
fn omnibox_value_is_written_to_the_input_element() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        omnibox: OmniboxModel { value: "https://example.com".to_owned(), ..Default::default() },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let input = doc.find_by_id(crate::ids::OMNI_INPUT).expect("asset has #omniInput");
    assert_eq!(doc.get(input).get_attr("value"), Some("https://example.com"));
}

#[test]
fn omnibox_zoom_indicator_follows_the_model() {
    let mut doc = parse_asset();
    let btn = doc.find_by_id(crate::ids::ZOOM_BTN).expect("asset has #zoomBtn");
    let label = doc.find_by_id(crate::ids::ZOOM_LABEL).expect("asset has #zoomLabel");
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, btn, "show"));
    let model = ChromeModel {
        omnibox: OmniboxModel { zoom_percent: Some(150), ..Default::default() },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    assert!(has_class(&doc, btn, "show"));
    let text = match &doc.get(doc.get(label).children[0]).data {
        NodeData::Text(t) => t.clone(),
        _ => String::new(),
    };
    assert_eq!(text, "150%");
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, btn, "show"));
}

#[test]
fn omnibox_help_chip_follows_the_model() {
    let mut doc = parse_asset();
    let chip = doc.find_by_id(crate::ids::HELP_CHIP).expect("asset has #helpChip");
    let text = doc.find_by_id(crate::ids::HELP_CHIP_TEXT).expect("asset has #helpChipText");
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, chip, "show"));
    let model = ChromeModel {
        omnibox: OmniboxModel { help_chip: Some("Пустая страница?".to_owned()), ..Default::default() },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    assert!(has_class(&doc, chip, "show"));
    let shown = match &doc.get(doc.get(text).children[0]).data {
        NodeData::Text(t) => t.clone(),
        _ => String::new(),
    };
    assert_eq!(shown, "Пустая страница?");
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, chip, "show"));
}

#[test]
fn omnibox_warning_shows_the_warn_banner_with_its_message() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        omnibox: OmniboxModel { value: String::new(), warning: Some("spoof risk".to_owned()), ..Default::default() },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let warn = doc.find_by_id(crate::ids::OMNI_WARN).expect("asset has #omniWarn");
    assert!(has_class(&doc, warn, "show"));
    let span = doc
        .get(warn)
        .children
        .iter()
        .copied()
        .find(|&c| matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case("span")))
        .expect("#omniWarn rebuilds a <span> with the warning message");
    let text: String = doc
        .get(span)
        .children
        .iter()
        .filter_map(|&c| match &doc.get(c).data {
            NodeData::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "spoof risk");
}

#[test]
fn no_warning_hides_the_warn_banner() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let warn = doc.find_by_id(crate::ids::OMNI_WARN).expect("asset has #omniWarn");
    assert!(!has_class(&doc, warn, "show"));
}

#[test]
fn dropdown_is_rebuilt_from_suggestions_and_toggles_open() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        dropdown: ChromeDropdownModel {
            open: true,
            suggestions: vec![ChromeSuggestionModel {
                idx: 0,
                label: "figma.com".to_owned(),
                sub_label: "Посещено вчера".to_owned(),
                color: "#0B6FE0".to_owned(),
                tag: "история".to_owned(),
            }],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::OMNI_DROPDOWN).expect("asset has #omniDropdown");
    assert!(has_class(&doc, container, "open"));
    let rows: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "dd-row")).collect();
    assert_eq!(rows.len(), 1, "old demo rows must be gone, only the 1 model suggestion remains");
    assert_eq!(doc.get(rows[0]).get_attr("data-sugg-idx"), Some("0"));
    let tag = find_descendant_by_class(&doc, rows[0], "dd-tag").expect("row has .dd-tag");
    assert_eq!(text_of(&doc, tag), "история");
}

#[test]
fn dropdown_closed_hides_the_open_class() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let container = doc.find_by_id(crate::ids::OMNI_DROPDOWN).expect("asset has #omniDropdown");
    assert!(!has_class(&doc, container, "open"));
}

#[test]
fn find_bar_binds_value_count_and_open_state() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        find: ChromeFindModel {
            open: true,
            value: "needle".to_owned(),
            count_label: "2/5".to_owned(),
            error: false,
            layout_fix: String::new(),
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let bar = doc.find_by_id(crate::ids::FIND_BAR).expect("asset has #findBar");
    assert!(has_class(&doc, bar, "open"));
    let input = doc.find_by_id(crate::ids::FIND_INPUT).expect("asset has #findInput");
    assert_eq!(doc.get(input).get_attr("value"), Some("needle"));
    let count = doc.find_by_id(crate::ids::FIND_COUNT).expect("asset has #findCount");
    let text: String = doc
        .get(count)
        .children
        .iter()
        .filter_map(|&c| match &doc.get(c).data {
            NodeData::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "2/5");
    assert!(!has_class(&doc, count, "error"));
}

#[test]
fn find_bar_shows_layout_fix_note_only_when_applied() {
    let bind = |layout_fix: &str| {
        let mut doc = parse_asset();
        let model = ChromeModel {
            find: ChromeFindModel {
                open: true,
                value: "ghbdtn".to_owned(),
                count_label: "1/1".to_owned(),
                error: false,
                layout_fix: layout_fix.to_owned(),
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        doc
    };
    let doc = bind("привет");
    let fix = doc.find_by_id(crate::ids::FIND_FIX).expect("asset has #findFix");
    assert!(has_class(&doc, fix, "open"));
    let text = doc.find_by_id(crate::ids::FIND_FIX_TEXT).expect("asset has #findFixText");
    assert_eq!(text_of(&doc, text), "Показано: «привет»");
    let link = doc.find_by_id(crate::ids::FIND_FIX_ORIGINAL).expect("asset has #findFixOriginal");
    assert_eq!(doc.get(link).get_attr("data-action"), Some("find-use-original"));
    let doc = bind("");
    let fix = doc.find_by_id(crate::ids::FIND_FIX).expect("asset has #findFix");
    assert!(!has_class(&doc, fix, "open"));
}

#[test]
fn find_bar_toggles_count_error_class() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        find: ChromeFindModel {
            open: true,
            value: "(".to_owned(),
            count_label: "ERR".to_owned(),
            error: true,
            layout_fix: String::new(),
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let count = doc.find_by_id(crate::ids::FIND_COUNT).expect("asset has #findCount");
    assert!(has_class(&doc, count, "error"));
}

#[test]
fn downloads_panel_is_rebuilt_from_entries_and_toggles_open() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        downloads_open: true,
        downloads: vec![ChromeDownloadModel {
            id: 1,
            ext_label: "PDF".to_owned(),
            name: "report.pdf".to_owned(),
            meta: "1.0 MB — идёт загрузка…".to_owned(),
            progress_fraction: Some(0.5),
            actions: vec![ChromeDownloadAction::Cancel],
        }],
        downloads_clearable: true,
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let panel = doc.find_by_id(crate::ids::DOWNLOADS_PANEL).expect("asset has #downloadsPanel");
    assert!(has_class(&doc, panel, "open"));
    let list = find_by_class(&doc, "dl-list").expect("asset has .dl-list");
    let cards: Vec<NodeId> =
        doc.get(list).children.iter().copied().filter(|&c| has_class(&doc, c, "dl-card")).collect();
    assert_eq!(cards.len(), 1, "old demo cards must be gone, only the 1 model entry remains");
    assert_eq!(doc.get(cards[0]).get_attr("data-dl-id"), Some("1"));
    let fill = doc
        .get(cards[0])
        .children
        .iter()
        .find(|&&c| has_class(&doc, c, "dl-progress-track"))
        .and_then(|&track| doc.get(track).children.iter().copied().find(|&c| has_class(&doc, c, "dl-progress-fill")))
        .expect("in-flight entry must render a progress fill");
    assert_eq!(doc.get(fill).get_attr("style"), Some("width:50%"));
    let header = find_by_class(&doc, "dl-header").expect("asset has .dl-header");
    assert!(has_class(&doc, header, "has-finished"));
    let actions = doc.get(cards[0]).children.iter().copied().find(|&c| has_class(&doc, c, "dl-actions")).expect(".dl-actions");
    let btn = doc.get(actions).children[0];
    assert_eq!(doc.get(btn).get_attr("data-dl"), Some("cancel"));
    assert_eq!(doc.get(btn).get_attr("data-dl-id"), Some("1"));
}

#[test]
fn empty_downloads_clears_all_cards() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let list = find_by_class(&doc, "dl-list").expect("asset has .dl-list");
    let cards = doc.get(list).children.iter().filter(|&&c| has_class(&doc, c, "dl-card")).count();
    assert_eq!(cards, 0);
}

#[test]
fn archive_panel_is_rebuilt_from_entries_and_toggles_open() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        archive_open: true,
        archive: vec![ChromeArchiveEntryModel {
            id: 7,
            fav_letter: "L".to_owned(),
            title: "Lumen design".to_owned(),
            url: "https://lumen.example/design".to_owned(),
            container_color: Some("#0066FF".to_owned()),
        }],
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let panel = doc.find_by_id(crate::ids::ARCHIVE_PANEL).expect("asset has #archivePanel");
    assert!(has_class(&doc, panel, "open"));
    let list = find_by_class(&doc, "arc-list").expect("asset has .arc-list");
    let cards: Vec<NodeId> =
        doc.get(list).children.iter().copied().filter(|&c| has_class(&doc, c, "arc-card")).collect();
    assert_eq!(cards.len(), 1, "old demo card must be gone, only the 1 model entry remains");

    let restore = find_descendants_by_class(&doc, cards[0], "arc-restore");
    let restore = restore.first().copied().expect("card has .arc-restore");
    assert_eq!(doc.get(restore).get_attr("data-action"), Some("archive-restore"));
    assert_eq!(doc.get(restore).get_attr("data-archive-id"), Some("7"));

    let dismiss = find_descendants_by_class(&doc, cards[0], "arc-dismiss");
    let dismiss = dismiss.first().copied().expect("card has .arc-dismiss");
    assert_eq!(doc.get(dismiss).get_attr("data-action"), Some("archive-dismiss"));
    assert_eq!(doc.get(dismiss).get_attr("data-archive-id"), Some("7"));

    let stripe = find_descendants_by_class(&doc, cards[0], "arc-stripe");
    let stripe = stripe.first().copied().expect("colour-tagged entry renders a stripe");
    assert_eq!(doc.get(stripe).get_attr("style"), Some("background:#0066FF"));
}

#[test]
fn archive_entry_without_container_omits_the_stripe() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        archive_open: true,
        archive: vec![ChromeArchiveEntryModel {
            id: 1,
            fav_letter: "A".to_owned(),
            title: "A".to_owned(),
            url: String::new(),
            container_color: None,
        }],
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let list = find_by_class(&doc, "arc-list").expect("asset has .arc-list");
    let card = doc.get(list).children.iter().copied().find(|&c| has_class(&doc, c, "arc-card")).unwrap();
    assert!(find_descendants_by_class(&doc, card, "arc-stripe").is_empty());
}

#[test]
fn empty_archive_clears_all_cards() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let list = find_by_class(&doc, "arc-list").expect("asset has .arc-list");
    let cards = doc.get(list).children.iter().filter(|&&c| has_class(&doc, c, "arc-card")).count();
    assert_eq!(cards, 0);
}

#[test]
fn compat_label_uses_russian_plural_forms() {
    assert_eq!(compat_fixes_label(1), "К сайту применено 1 исправление совместимости");
    assert_eq!(compat_fixes_label(2), "К сайту применено 2 исправления совместимости");
    assert_eq!(compat_fixes_label(5), "К сайту применено 5 исправлений совместимости");
    assert_eq!(compat_fixes_label(11), "К сайту применено 11 исправлений совместимости");
    assert_eq!(compat_fixes_label(21), "К сайту применено 21 исправление совместимости");
}

#[test]
fn popover_compat_row_shows_only_with_applied_fixes() {
    let node_text = |doc: &Document, id: NodeId| -> String {
        doc.get(id)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    };
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let row = doc.find_by_id(crate::ids::COMPAT_ROW).expect("asset has #compatRow");
    assert!(!has_class(&doc, row, "show"), "no fixes — row stays hidden");

    bind_model(&mut doc, &ChromeModel { compat_fixes: 2, ..ChromeModel::default() });
    assert!(has_class(&doc, row, "show"));
    let text = doc.find_by_id(crate::ids::COMPAT_TEXT).expect("asset has #compatText");
    assert_eq!(node_text(&doc, text), "К сайту применено 2 исправления совместимости");

    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, row, "show"), "after the reload N=0 — row hidden again");
}

#[test]
fn fix_panel_shows_exactly_one_block_at_a_time() {
    let node_text = |doc: &Document, id: NodeId| -> String {
        doc.get(id)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    };
    let mut doc = parse_asset();
    let steps = doc.find_by_id(crate::ids::FIX_STEPS).expect("asset has #fixSteps");
    let confirm = doc.find_by_id(crate::ids::FIX_CONFIRM).expect("asset has #fixConfirm");
    let ask = doc.find_by_id(crate::ids::FIX_ASK).expect("asset has #fixAsk");
    let fix = |f: ChromeFixPanelModel| ChromeModel { fix: f, ..ChromeModel::default() };

    bind_model(&mut doc, &fix(ChromeFixPanelModel { steps_enabled: true, ..Default::default() }));
    assert!(!has_class(&doc, steps, "hide"));
    assert!(!has_class(&doc, confirm, "show") && !has_class(&doc, ask, "show"));

    bind_model(
        &mut doc,
        &fix(ChromeFixPanelModel { steps_enabled: true, confirm_reset: true, ..Default::default() }),
    );
    assert!(has_class(&doc, steps, "hide") && has_class(&doc, confirm, "show"));

    bind_model(
        &mut doc,
        &fix(ChromeFixPanelModel {
            steps_enabled: true,
            asking: Some("Заработало после шага «Обновить без кэша»?".into()),
            tried: [true, false, false],
            ..Default::default()
        }),
    );
    assert!(has_class(&doc, steps, "hide") && has_class(&doc, ask, "show"));
    assert!(!has_class(&doc, confirm, "show"));
    let text = doc.find_by_id(crate::ids::FIX_ASK_TEXT).expect("asset has #fixAskText");
    assert!(node_text(&doc, text).contains("Обновить без кэша"));
    let no_cache = doc.find_by_id(crate::ids::FIX_NO_CACHE).expect("asset has #fixNoCache");
    assert!(has_class(&doc, no_cache, "tried"));

    bind_model(&mut doc, &fix(ChromeFixPanelModel { note: "Для этой страницы шагов нет".into(), ..Default::default() }));
    assert!(has_class(&doc, steps, "hide"), "not an http(s) page — no ladder");
    let note = doc.find_by_id(crate::ids::FIX_NOTE).expect("asset has #fixNote");
    assert_eq!(node_text(&doc, note), "Для этой страницы шагов нет");
}

#[test]
fn popover_binds_blocked_total_and_permission_rows() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        popover_open: true,
        blocked_total: 42,
        permissions: [
            ChromePermState::Allow,
            ChromePermState::Deny,
            ChromePermState::Deny,
            ChromePermState::Allow,
            ChromePermState::Allow,
        ],
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let popover = doc.find_by_id(crate::ids::PERM_POPOVER).expect("asset has #permPopover");
    assert!(has_class(&doc, popover, "open"));
    let stat = doc.find_by_id(crate::ids::STAT_TRACKERS).expect("asset has #statTrackers");
    let text: String = doc
        .get(stat)
        .children
        .iter()
        .filter_map(|&c| match &doc.get(c).data {
            NodeData::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "42");

    let rows: Vec<NodeId> =
        doc.get(popover).children.iter().copied().filter(|&c| has_class(&doc, c, "perm-row")).collect();
    // BUG-411 added the Notifications/Clipboard rows the design was missing;
    // UX-USER-ACTIVATION added the pop-ups row.
    assert_eq!(rows.len(), 5);
    let camera_actions = doc.get(rows[0]).children.iter().copied().find(|&c| has_class(&doc, c, "perm-actions")).unwrap();
    let camera_allow = doc.get(camera_actions).children.iter().copied().find(|&c| has_class(&doc, c, "allow")).unwrap();
    assert!(has_class(&doc, camera_allow, "selected-allow"));

    let mic_actions = doc.get(rows[1]).children.iter().copied().find(|&c| has_class(&doc, c, "perm-actions")).unwrap();
    let mic_deny = doc.get(mic_actions).children.iter().copied().find(|&c| has_class(&doc, c, "deny")).unwrap();
    assert!(has_class(&doc, mic_deny, "selected-deny"));

    // BUG-411: rows 3/4 (Notifications, Clipboard) were unreachable before
    // the asset gained them — bind them from the same array.
    let notif_actions = doc.get(rows[2]).children.iter().copied().find(|&c| has_class(&doc, c, "perm-actions")).unwrap();
    let notif_deny = doc.get(notif_actions).children.iter().copied().find(|&c| has_class(&doc, c, "deny")).unwrap();
    assert!(has_class(&doc, notif_deny, "selected-deny"));
    let clip_actions = doc.get(rows[3]).children.iter().copied().find(|&c| has_class(&doc, c, "perm-actions")).unwrap();
    let clip_allow = doc.get(clip_actions).children.iter().copied().find(|&c| has_class(&doc, c, "allow")).unwrap();
    assert!(has_class(&doc, clip_allow, "selected-allow"));
}

/// BUG-411: the popover names the host it applies to, and its shields row
/// reflects (and is the only UI for) the per-site filter state.
#[test]
fn popover_binds_domain_and_site_shields_row() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        popover_open: true,
        popover_domain: "example.org".to_owned(),
        site_shields_on: false,
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let span = doc.find_by_id(crate::ids::PERM_DOMAIN).expect("asset has #permDomain");
    assert_eq!(text_of(&doc, span), "example.org");
    let toggle = doc.find_by_id(crate::ids::SHIELD_SITE_TOGGLE).expect("asset has #shieldSiteToggle");
    assert!(!has_class(&doc, toggle, "on"));
    let state = doc.find_by_id(crate::ids::SHIELD_SITE_STATE).expect("asset has #shieldSiteState");
    assert_eq!(text_of(&doc, state), "Выкл");

    let model = ChromeModel { popover_open: true, site_shields_on: true, ..ChromeModel::default() };
    bind_model(&mut doc, &model);
    // Empty domain must not leave the design's `figma.com` demo text behind.
    assert_eq!(text_of(&doc, span), NO_DOMAIN_LABEL);
    assert!(has_class(&doc, toggle, "on"));
    assert_eq!(text_of(&doc, state), "Вкл");
}

/// UX-USER-ACTIVATION: `#popupBtn` shows the number of blocked popups and
/// hides again when there are none.
#[test]
fn blocked_popup_indicator_follows_the_count() {
    let mut doc = parse_asset();
    let btn = doc.find_by_id(crate::ids::POPUP_BTN).expect("asset has #popupBtn");
    let label = doc.find_by_id(crate::ids::POPUP_LABEL).expect("asset has #popupLabel");
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, btn, "show"));
    let model = ChromeModel {
        omnibox: OmniboxModel { popups_blocked: 3, ..Default::default() },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    assert!(has_class(&doc, btn, "show"));
    assert_eq!(text_of(&doc, label), "3");
    assert_eq!(doc.get(btn).get_attr("data-action"), Some("toggle-shield-popover"));
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, btn, "show"));
}

#[test]
fn ask_state_selects_neither_button() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let popover = doc.find_by_id(crate::ids::PERM_POPOVER).expect("asset has #permPopover");
    let rows: Vec<NodeId> =
        doc.get(popover).children.iter().copied().filter(|&c| has_class(&doc, c, "perm-row")).collect();
    let actions = doc.get(rows[0]).children.iter().copied().find(|&c| has_class(&doc, c, "perm-actions")).unwrap();
    for &btn in &doc.get(actions).children {
        assert!(!has_class(&doc, btn, "selected-allow"));
        assert!(!has_class(&doc, btn, "selected-deny"));
    }
}

#[test]
fn palette_binds_query_and_rebuilds_results_and_toggles_open() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        palette: ChromePaletteModel {
            open: true,
            query: "new t".to_owned(),
            results: vec![
                ChromePaletteResultModel {
                    label: "New Tab".to_owned(),
                    sub_label: "Ctrl+T".to_owned(),
                    selected: true,
                    index: 0,
                },
                ChromePaletteResultModel {
                    label: "New Window".to_owned(),
                    sub_label: "".to_owned(),
                    selected: false,
                    index: 1,
                },
            ],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let overlay = doc.find_by_id(crate::ids::CP_OVERLAY).expect("asset has #cpOverlay");
    assert!(has_class(&doc, overlay, "open"));
    let input = doc.find_by_id(crate::ids::CP_INPUT).expect("asset has #cpInput");
    assert_eq!(doc.get(input).get_attr("value"), Some("new t"));
    let list = doc.find_by_id(crate::ids::CP_LIST).expect("asset has #cpList");
    let rows: Vec<NodeId> =
        doc.get(list).children.iter().copied().filter(|&c| has_class(&doc, c, "cp-row")).collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(doc.get(rows[0]).get_attr("style"), Some("background:var(--surface-2)"));
    assert_eq!(doc.get(rows[1]).get_attr("style"), None);
    assert_eq!(doc.get(rows[0]).get_attr("data-cp-index"), Some("0"));
    assert_eq!(doc.get(rows[1]).get_attr("data-cp-index"), Some("1"));
}

#[test]
fn palette_closed_hides_open_class_and_empty_results_show_empty_state() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let overlay = doc.find_by_id(crate::ids::CP_OVERLAY).expect("asset has #cpOverlay");
    assert!(!has_class(&doc, overlay, "open"));
    let list = doc.find_by_id(crate::ids::CP_LIST).expect("asset has #cpList");
    let empty = doc.get(list).children.iter().copied().find(|&c| has_class(&doc, c, "cp-empty"));
    assert!(empty.is_some(), "no results must render the .cp-empty state");
}

#[test]
fn console_panel_opens_and_renders_lines() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        console: ChromeConsoleModel {
            open: true,
            total: 3,
            lines: vec![
                ChromeConsoleLineModel { level: 0, text: "hello".to_owned() },
                ChromeConsoleLineModel { level: 1, text: "careful".to_owned() },
                ChromeConsoleLineModel { level: 2, text: "boom".to_owned() },
            ],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let panel = doc.find_by_id(crate::ids::DEVTOOLS).expect("asset has #devtools");
    assert!(has_class(&doc, panel, "open"));
    let title = doc.find_by_id(crate::ids::DT_CONSOLE_TITLE).expect("title");
    assert_eq!(text_of(&doc, title), "Console (3)");
    let list = doc.find_by_id(crate::ids::DT_CONSOLE).expect("list");
    let lines = find_descendants_by_class(&doc, list, "console-line");
    assert_eq!(lines.len(), 3);
    assert!(has_class(&doc, lines[1], "warn"));
    assert!(has_class(&doc, lines[2], "error"));
    let span = doc.get(lines[0]).children[0];
    assert_eq!(text_of(&doc, span), "hello");
    bind_model(&mut doc, &ChromeModel::default());
    let panel = doc.find_by_id(crate::ids::DEVTOOLS).expect("panel");
    assert!(!has_class(&doc, panel, "open"));
    let list = doc.find_by_id(crate::ids::DT_CONSOLE).expect("list");
    assert!(find_descendants_by_class(&doc, list, "console-line").is_empty());
}

#[test]
fn network_panel_opens_and_renders_rows() {
    let mut doc = parse_asset();
    let row = |status: &str, class: u8, blocked: bool| ChromeNetworkRowModel {
        method: "GET".to_owned(),
        status: status.to_owned(),
        status_class: class,
        time: "12 ms".to_owned(),
        url: "https://example.test/a".to_owned(),
        blocked,
        failed: false,
    };
    let model = ChromeModel {
        network: ChromeNetworkModel {
            open: true,
            total: 2,
            rows: vec![row("200", 2, false), row("blocked", 4, true)],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let panel = doc.find_by_id(crate::ids::NETWORK_PANEL).expect("asset has #networkPanel");
    assert!(has_class(&doc, panel, "open"));
    let title = doc.find_by_id(crate::ids::DT_NETWORK_TITLE).expect("title");
    assert_eq!(text_of(&doc, title), "Network (2)");
    let list = doc.find_by_id(crate::ids::DT_NETWORK).expect("list");
    let rows = find_descendants_by_class(&doc, list, "net-row");
    assert_eq!(rows.len(), 2);
    assert!(has_class(&doc, rows[1], "blocked"));
    let badges = find_descendants_by_class(&doc, list, "status-badge");
    assert!(has_class(&doc, badges[0], "s2"));
    assert!(has_class(&doc, badges[1], "s4"));
    bind_model(&mut doc, &ChromeModel::default());
    let panel = doc.find_by_id(crate::ids::NETWORK_PANEL).expect("panel");
    assert!(!has_class(&doc, panel, "open"));
    let list = doc.find_by_id(crate::ids::DT_NETWORK).expect("list");
    assert!(find_descendants_by_class(&doc, list, "net-row").is_empty());
}

#[test]
fn read_later_panel_opens_and_renders_rows() {
    let mut doc = parse_asset();
    let row = |id: i64, read: bool| ChromeReadLaterRowModel {
        id,
        title: format!("Page {id}"),
        meta: "example.com  ·  2024-01-15".to_owned(),
        status: if read { "Read" } else { "Unread" }.to_owned(),
        read,
    };
    let model = ChromeModel {
        read_later: ChromeReadLaterModel { open: true, total: 2, rows: vec![row(7, false), row(9, true)] },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let panel = doc.find_by_id(crate::ids::READ_LATER_PANEL).expect("asset has #readLaterPanel");
    assert!(has_class(&doc, panel, "open"));
    let title = doc.find_by_id(crate::ids::RL_TITLE).expect("title");
    assert_eq!(text_of(&doc, title), "Read Later (2)");
    let list = doc.find_by_id(crate::ids::RL_LIST).expect("list");
    let rows = find_descendants_by_class(&doc, list, "rl-row");
    assert_eq!(rows.len(), 2);
    let badges = find_descendants_by_class(&doc, list, "rl-badge");
    assert!(!has_class(&doc, badges[0], "read"));
    assert!(has_class(&doc, badges[1], "read"));
    let del = find_descendants_by_class(&doc, list, "rl-delete");
    assert_eq!(doc.get(del[1]).get_attr("data-action"), Some("read-later-delete"));
    assert_eq!(doc.get(del[1]).get_attr("data-read-later-id"), Some("9"));
    let open = find_descendants_by_class(&doc, list, "rl-text");
    assert_eq!(doc.get(open[0]).get_attr("data-action"), Some("read-later-open"));
    assert_eq!(doc.get(open[0]).get_attr("data-read-later-id"), Some("7"));
    bind_model(&mut doc, &ChromeModel::default());
    let panel = doc.find_by_id(crate::ids::READ_LATER_PANEL).expect("panel");
    assert!(!has_class(&doc, panel, "open"));
    let list = doc.find_by_id(crate::ids::RL_LIST).expect("list");
    assert!(find_descendants_by_class(&doc, list, "rl-row").is_empty());
    let title = doc.find_by_id(crate::ids::RL_TITLE).expect("title");
    assert_eq!(text_of(&doc, title), "Read Later");
}

#[test]
fn privacy_panel_opens_and_renders_rows() {
    let mut doc = parse_asset();
    let row = |host: &str, state: &str| ChromePrivacyRowModel {
        host: host.to_owned(),
        sub: "blocked · ||x^".to_owned(),
        state: state.to_owned(),
    };
    let model = ChromeModel {
        privacy: ChromePrivacyModel {
            open: true,
            blocked: 1,
            allowed: 2,
            position: "3/9".to_owned(),
            rows: vec![row("ads.test", "blocked"), row("cdn.test", "allowed")],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let panel = doc.find_by_id(crate::ids::PRIVACY_PANEL).expect("asset has #privacyPanel");
    assert!(has_class(&doc, panel, "open"));
    let blocked = doc.find_by_id(crate::ids::PV_BLOCKED).expect("blocked");
    assert_eq!(text_of(&doc, blocked), "1 blocked");
    let allowed = doc.find_by_id(crate::ids::PV_ALLOWED).expect("allowed");
    assert_eq!(text_of(&doc, allowed), "2 allowed");
    let pos = doc.find_by_id(crate::ids::PV_POSITION).expect("position");
    assert_eq!(text_of(&doc, pos), "3/9");
    let list = doc.find_by_id(crate::ids::PV_LIST).expect("list");
    let rows = find_descendants_by_class(&doc, list, "pv-row");
    assert_eq!(rows.len(), 2);
    assert!(has_class(&doc, rows[0], "blocked"));
    assert!(has_class(&doc, rows[1], "allowed"));
    bind_model(&mut doc, &ChromeModel::default());
    let panel = doc.find_by_id(crate::ids::PRIVACY_PANEL).expect("panel");
    assert!(!has_class(&doc, panel, "open"));
    let list = doc.find_by_id(crate::ids::PV_LIST).expect("list");
    assert!(find_descendants_by_class(&doc, list, "pv-row").is_empty());
}

#[test]
fn workspace_bar_opens_and_renders_chips() {
    let mut doc = parse_asset();
    let ws = |id: i64, active: bool| ChromeWorkspaceModel {
        id,
        name: format!("WS {id}"),
        active,
        color: "#0066FF".to_owned(),
    };
    let model = ChromeModel {
        workspaces: vec![ws(4, true), ws(8, false)],
        workspace_bar_open: true,
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let bar = doc.find_by_id(crate::ids::WORKSPACE_BAR).expect("asset has #workspaceBar");
    assert!(has_class(&doc, bar, "open"));
    let list = doc.find_by_id(crate::ids::WSB_CHIPS).expect("chips");
    let chips = find_descendants_by_class(&doc, list, "wsb-chip");
    assert_eq!(chips.len(), 2);
    assert!(has_class(&doc, chips[0], "active"));
    assert!(!has_class(&doc, chips[1], "active"));
    let dels = find_descendants_by_class(&doc, list, "wsb-del");
    assert_eq!(doc.get(dels[1]).get_attr("data-action"), Some("delete-workspace"));
    assert_eq!(doc.get(dels[1]).get_attr("data-ws-id"), Some("8"));
    let names = find_descendants_by_class(&doc, list, "wsb-name");
    assert_eq!(doc.get(names[0]).get_attr("data-action"), Some("select-workspace"));
    assert_eq!(doc.get(names[0]).get_attr("data-ws-id"), Some("4"));
    assert_eq!(text_of(&doc, names[0]), "WS 4");
    bind_model(&mut doc, &ChromeModel::default());
    let bar = doc.find_by_id(crate::ids::WORKSPACE_BAR).expect("bar");
    assert!(!has_class(&doc, bar, "open"));
    let list = doc.find_by_id(crate::ids::WSB_CHIPS).expect("chips");
    assert!(find_descendants_by_class(&doc, list, "wsb-chip").is_empty());
}

#[test]
fn pip_card_opens_places_and_toggles_glyph() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        pip: ChromePipModel {
            open: true,
            x: 30.0,
            y: 50.0,
            playing: true,
            title: "Clip".to_owned(),
            poster: "https://e.test/p.png".to_owned(),
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let root = doc.find_by_id(crate::ids::PIP_CARD).expect("asset has #pipCard");
    assert!(has_class(&doc, root, "open") && has_class(&doc, root, "playing"));
    assert_eq!(doc.get(root).get_attr("style"), Some("left:30px;top:50px"));
    let title = doc.find_by_id(crate::ids::PIP_TITLE).expect("title");
    assert_eq!(text_of(&doc, title), "Clip");
    let poster = doc.find_by_id(crate::ids::PIP_POSTER).expect("poster");
    assert_eq!(doc.get(poster).get_attr("src"), Some("https://e.test/p.png"));
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, root, "open"));
}

#[test]
fn tab_ctx_menu_opens_places_and_flips_labels() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        tab_ctx: ChromeTabCtxModel { open: true, x: 40.0, y: 12.0, pinned: true, collapsed: true },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let root = doc.find_by_id(crate::ids::TAB_CTX_MENU).expect("asset has #tabCtxMenu");
    assert!(has_class(&doc, root, "open"));
    assert_eq!(doc.get(root).get_attr("style"), Some("left:40px;top:12px"));
    let pin = doc.find_by_id(crate::ids::TAB_CTX_PIN).expect("pin row");
    assert_eq!(text_of(&doc, pin), "Открепить");
    let grp = doc.find_by_id(crate::ids::TAB_CTX_TOGGLE_GROUP).expect("group row");
    assert_eq!(text_of(&doc, grp), "Развернуть группу");
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, root, "open"));
}

#[test]
fn page_ctx_menu_builds_rows() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        page_ctx: ChromePageCtxModel {
            open: true,
            x: 10.0,
            y: 20.0,
            suggestions: vec!["world".to_owned(), "would".to_owned()],
            spell_footer: true,
            extra_rows: vec![("ho:other".to_owned(), "Открыть в другом браузере".to_owned())],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let root = doc.find_by_id(crate::ids::PAGE_CTX_MENU).expect("asset has #pageCtxMenu");
    assert!(has_class(&doc, root, "open"));
    assert_eq!(doc.get(root).get_attr("style"), Some("left:10px;top:20px"));
    assert_eq!(find_descendants_by_class(&doc, root, "page-ctx-row").len(), 5);
    assert_eq!(find_descendants_by_class(&doc, root, "sep").len(), 2);
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, root, "open"));
    assert!(find_descendants_by_class(&doc, root, "page-ctx-row").is_empty());
}

#[test]
fn note_modal_opens_and_shows_fields() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        note: ChromeNoteModel {
            open: true,
            url: "https://a.test/p".to_owned(),
            selection: "quoted".to_owned(),
            comment: "mine".to_owned(),
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let overlay = doc.find_by_id(crate::ids::NOTE_OVERLAY).expect("asset has #noteOverlay");
    assert!(has_class(&doc, overlay, "open"));
    let url = doc.find_by_id(crate::ids::NOTE_URL).expect("url");
    assert_eq!(text_of(&doc, url), "https://a.test/p");
    let sel = doc.find_by_id(crate::ids::NOTE_SEL).expect("selection");
    assert_eq!(text_of(&doc, sel), "quoted");
    let cmt = doc.find_by_id(crate::ids::NOTE_COMMENT).expect("comment");
    assert_eq!(text_of(&doc, cmt), "mine");
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, overlay, "open"));
}

#[test]
fn focus_timer_shows_countdown_icon_and_done_banner() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        focus: ChromeFocusModel { active: true, running: true, finished: false, label: "24:59".to_owned() },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let pill = doc.find_by_id(crate::ids::FOCUS_TIMER).expect("asset has the focus pill");
    assert!(has_class(&doc, pill, "open"));
    let time = doc.find_by_id(crate::ids::FOCUS_TIME).expect("time");
    assert_eq!(text_of(&doc, time), "24:59");
    let icon = doc.find_by_id(crate::ids::FOCUS_TOGGLE_ICON).expect("icon");
    assert_eq!(doc.get(icon).get_attr("href"), Some("#i-pause"));
    let done = doc.find_by_id(crate::ids::FOCUS_DONE).expect("done");
    assert!(!has_class(&doc, done, "show"));
    let model = ChromeModel {
        focus: ChromeFocusModel { active: true, running: false, finished: true, label: "00:00".to_owned() },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    assert_eq!(doc.get(icon).get_attr("href"), Some("#i-play"));
    assert!(has_class(&doc, done, "show"));
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, pill, "open"));
    assert!(!has_class(&doc, done, "show"));
}

#[test]
fn inspector_panel_binds_tab_rows_and_network() {
    let mut doc = parse_asset();
    let row = |kind: u8, key: &str, value: &str| ChromeInspectorRowModel {
        kind,
        key: key.to_owned(),
        value: value.to_owned(),
    };
    let model = ChromeModel {
        inspector: ChromeInspectorModel {
            open: true,
            tab: 1,
            label: "div#a".to_owned(),
            node: "NodeId(3)".to_owned(),
            rows: vec![row(1, ".a", ""), row(0, "color", "red"), row(2, "", "")],
            position: "3/30".to_owned(),
            ..ChromeInspectorModel::default()
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let panel = doc.find_by_id(crate::ids::INSP_PANEL).expect("asset has #inspPanel");
    assert!(has_class(&doc, panel, "open"));
    let styles = doc.find_by_id(crate::ids::INSP_TAB_STYLES).expect("styles tab");
    assert!(has_class(&doc, styles, "active"));
    let elements = doc.find_by_id(crate::ids::INSP_TAB_ELEMENTS).expect("elements tab");
    assert!(!has_class(&doc, elements, "active"));
    let label = doc.find_by_id(crate::ids::INSP_LABEL).expect("label");
    assert_eq!(text_of(&doc, label), "div#a");
    let list = doc.find_by_id(crate::ids::INSP_ROWS).expect("rows");
    let rows = find_descendants_by_class(&doc, list, "insp-row");
    assert_eq!(rows.len(), 3);
    assert!(has_class(&doc, rows[0], "insp-sel"));
    assert!(has_class(&doc, rows[2], "insp-sep"));
    let keys = find_descendants_by_class(&doc, list, "insp-key");
    assert_eq!(text_of(&doc, keys[1]), "color:");
    // Network tab: request rows go to #inspNet, element rows are cleared.
    let net = ChromeNetworkRowModel {
        method: "GET".to_owned(),
        status: "200".to_owned(),
        status_class: 2,
        time: "5 ms".to_owned(),
        url: "https://a.test/".to_owned(),
        blocked: false,
        failed: false,
    };
    let model = ChromeModel {
        inspector: ChromeInspectorModel {
            open: true,
            tab: 3,
            net_rows: vec![net],
            ..ChromeInspectorModel::default()
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let list = doc.find_by_id(crate::ids::INSP_ROWS).expect("rows");
    assert!(find_descendants_by_class(&doc, list, "insp-row").is_empty());
    let netlist = doc.find_by_id(crate::ids::INSP_NET).expect("net");
    assert_eq!(find_descendants_by_class(&doc, netlist, "net-row").len(), 1);
    bind_model(&mut doc, &ChromeModel::default());
    let panel = doc.find_by_id(crate::ids::INSP_PANEL).expect("panel");
    assert!(!has_class(&doc, panel, "open"));
}

#[test]
fn profile_menus_open_and_mark_active_item() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        profile_menu_open: true,
        profile_slug: Some("work".to_owned()),
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    for id in [crate::ids::PROFILE_MENU, crate::ids::PROFILE_MENU_H] {
        let menu = doc.find_by_id(id).expect("asset has the profile menu");
        assert!(has_class(&doc, menu, "open"), "{id} must be open");
        let active: Vec<_> = find_descendants_by_class(&doc, menu, "pm-item")
            .into_iter()
            .filter(|&i| has_class(&doc, i, "active"))
            .map(|i| doc.get(i).get_attr("data-profile").unwrap_or("").to_owned())
            .collect();
        assert_eq!(active, ["work"], "{id}");
    }
    bind_model(&mut doc, &ChromeModel::default());
    let menu = doc.find_by_id(crate::ids::PROFILE_MENU).expect("menu");
    assert!(!has_class(&doc, menu, "open"));
}

#[test]
fn access_overlay_moves_active_pills_and_toggles() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        access: ChromeAccessModel {
            open: true,
            font_value: "1.5".into(),
            reduced_motion: true,
            forced_colors: false,
            cursor_value: "large".into(),
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let overlay = doc.find_by_id(crate::ids::ACCESS_OVERLAY).expect("asset has #accessOverlay");
    assert!(has_class(&doc, overlay, "open"));
    let active_of = |doc: &Document, id: &str| -> Vec<String> {
        let g = doc.find_by_id(id).expect("pill group");
        find_descendants_by_class(doc, g, "ac-pill")
            .into_iter()
            .filter(|&p| has_class(doc, p, "active"))
            .map(|p| doc.get(p).get_attr("data-value").unwrap_or("").to_owned())
            .collect()
    };
    assert_eq!(active_of(&doc, crate::ids::AC_FONT_PILLS), ["1.5"]);
    assert_eq!(active_of(&doc, crate::ids::AC_CURSOR_PILLS), ["large"]);
    let motion = doc.find_by_id(crate::ids::AC_MOTION_TOGGLE).expect("motion toggle");
    let colors = doc.find_by_id(crate::ids::AC_COLORS_TOGGLE).expect("colors toggle");
    assert!(has_class(&doc, motion, "on"));
    assert!(!has_class(&doc, colors, "on"));
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, overlay, "open"));
    assert!(!has_class(&doc, motion, "on"));
}

#[test]
fn shortcuts_overlay_rebuilds_rows_and_toggles_open() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        shortcuts: ChromeShortcutsModel {
            open: true,
            rows: vec![
                ChromeShortcutRowModel { index: 3, label: "A".into(), binding: "Ctrl+A".into(), rebinding: false },
                ChromeShortcutRowModel { index: 4, label: "B".into(), binding: "...".into(), rebinding: true },
            ],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let overlay = doc.find_by_id(crate::ids::SHORTCUTS_OVERLAY).expect("asset has #shortcutsOverlay");
    assert!(has_class(&doc, overlay, "open"));
    let list = doc.find_by_id(crate::ids::SC_LIST).expect("asset has #scList");
    let rows = find_descendants_by_class(&doc, list, "sc-row");
    assert_eq!(rows.len(), 2, "seed row must be replaced");
    assert_eq!(doc.get(rows[1]).get_attr("data-sc-index"), Some("4"));
    assert!(has_class(&doc, rows[1], "rebinding"));
    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, overlay, "open"));
    assert!(find_descendants_by_class(&doc, list, "sc-row").is_empty());
}

#[test]
fn cert_binds_title_rows_and_fingerprint_and_toggles_open() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        cert: ChromeCertModel {
            open: true,
            title: "Сертификат — example.com".to_owned(),
            rows: [
                "example.com".to_owned(),
                "Example Inc.".to_owned(),
                "example.com, *.example.com".to_owned(),
                "Example CA".to_owned(),
                "2026-01-01".to_owned(),
                "2027-01-01".to_owned(),
            ],
            fingerprint: "AA:BB:CC".to_owned(),
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let overlay = doc.find_by_id(crate::ids::CERT_OVERLAY).expect("asset has #certOverlay");
    assert!(has_class(&doc, overlay, "open"));
    let h3 = find_descendant_by_tag(&doc, overlay, "h3").expect("#certOverlay has an <h3> title");
    let title_text: String = doc
        .get(h3)
        .children
        .iter()
        .filter_map(|&c| match &doc.get(c).data {
            NodeData::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(title_text, "Сертификат — example.com");

    let rows = find_descendants_by_class(&doc, overlay, "cert-row");
    assert_eq!(rows.len(), 6, "asset must have exactly 6 static .cert-row elements");
    let first_v = doc.get(rows[0]).children.iter().copied().find(|&c| has_class(&doc, c, "v")).unwrap();
    let first_v_text: String = doc
        .get(first_v)
        .children
        .iter()
        .filter_map(|&c| match &doc.get(c).data {
            NodeData::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(first_v_text, "example.com");

    let fp = find_descendant_by_class(&doc, overlay, "cert-fp").expect("#certOverlay has .cert-fp");
    let fp_text: String = doc
        .get(fp)
        .children
        .iter()
        .filter_map(|&c| match &doc.get(c).data {
            NodeData::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(fp_text, "AA:BB:CC");
}

#[test]
fn cert_closed_hides_open_class() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let overlay = doc.find_by_id(crate::ids::CERT_OVERLAY).expect("asset has #certOverlay");
    assert!(!has_class(&doc, overlay, "open"));
}

#[test]
fn print_open_toggles_the_open_class() {
    let mut doc = parse_asset();
    let open = ChromePrintModel { open: true, ..ChromePrintModel::default() };
    bind_model(&mut doc, &ChromeModel { print: open, ..ChromeModel::default() });
    let overlay = doc.find_by_id(crate::ids::PRINT_OVERLAY).expect("asset has #printOverlay");
    assert!(has_class(&doc, overlay, "open"));

    bind_model(&mut doc, &ChromeModel { print: ChromePrintModel::default(), ..ChromeModel::default() });
    assert!(!has_class(&doc, overlay, "open"));
}

#[test]
fn print_landscape_moves_selected_option() {
    let mut doc = parse_asset();
    let select = doc.find_by_id(crate::ids::PRINT_ORIENTATION_SELECT).expect("asset has #printOrientationSelect");
    let options = doc.get(select).children.clone();
    assert_eq!(options.len(), 2, "asset must have exactly 2 orientation options");

    let portrait = ChromePrintModel { landscape: false, ..ChromePrintModel::default() };
    bind_model(&mut doc, &ChromeModel { print: portrait, ..ChromeModel::default() });
    assert!(doc.get(options[0]).get_attr("selected").is_some());
    assert!(doc.get(options[1]).get_attr("selected").is_none());

    let landscape = ChromePrintModel { landscape: true, ..ChromePrintModel::default() };
    bind_model(&mut doc, &ChromeModel { print: landscape, ..ChromeModel::default() });
    assert!(doc.get(options[0]).get_attr("selected").is_none());
    assert!(doc.get(options[1]).get_attr("selected").is_some());
}

#[test]
fn print_backgrounds_toggles_checked() {
    let mut doc = parse_asset();
    let check = doc.find_by_id(crate::ids::PRINT_BG_CHECK).expect("asset has #printBgCheck");

    let on = ChromePrintModel { backgrounds: true, ..ChromePrintModel::default() };
    bind_model(&mut doc, &ChromeModel { print: on, ..ChromeModel::default() });
    assert!(doc.get(check).get_attr("checked").is_some());

    let off = ChromePrintModel { backgrounds: false, ..ChromePrintModel::default() };
    bind_model(&mut doc, &ChromeModel { print: off, ..ChromeModel::default() });
    assert!(doc.get(check).get_attr("checked").is_none());
}

fn text_of(doc: &Document, id: NodeId) -> String {
    doc.get(id)
        .children
        .iter()
        .filter_map(|&c| match &doc.get(c).data {
            NodeData::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn default_content_view_is_page() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let page = doc.find_by_id(crate::ids::VIEW_PAGE).expect("asset has #view-page");
    assert!(has_class(&doc, page, "active"));
    let history = doc.find_by_id(crate::ids::VIEW_HISTORY).expect("asset has #view-history");
    assert!(!has_class(&doc, history, "active"));
}

#[test]
fn content_view_switches_active_view_exclusively() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel { content_view: ChromeContentView::Settings, ..ChromeModel::default() });
    let page = doc.find_by_id(crate::ids::VIEW_PAGE).expect("asset has #view-page");
    let history = doc.find_by_id(crate::ids::VIEW_HISTORY).expect("asset has #view-history");
    let bookmarks = doc.find_by_id(crate::ids::VIEW_BOOKMARKS).expect("asset has #view-bookmarks");
    let settings = doc.find_by_id(crate::ids::VIEW_SETTINGS).expect("asset has #view-settings");
    assert!(!has_class(&doc, page, "active"));
    assert!(!has_class(&doc, history, "active"));
    assert!(!has_class(&doc, bookmarks, "active"));
    assert!(has_class(&doc, settings, "active"));
}

/// CC-18: `body[data-demo]` and exactly one `#demoSwitch` button's
/// `.active` follow [`ChromeControlPanelModel::shape`].
#[test]
fn control_panel_shape_sets_data_demo_and_the_matching_switch_button() {
    let mut doc = parse_asset();
    bind_model(
        &mut doc,
        &ChromeModel {
            control_panel: ChromeControlPanelModel { shape: ControlPanelShape::Dock, ..Default::default() },
            ..ChromeModel::default()
        },
    );
    let body = doc.body().expect("asset has <body>");
    assert_eq!(doc.get(body).get_attr("data-demo"), Some("dock"));
    let bar = doc.find_by_id(crate::ids::DEMO_BAR).expect("asset has #demoBar");
    let mut active_variants: Vec<&str> = collect_by_attr(&doc, bar, "data-demo-variant")
        .into_iter()
        .filter(|&n| has_class(&doc, n, "active"))
        .map(|n| doc.get(n).get_attr("data-demo-variant").expect("just matched on it"))
        .collect();
    assert_eq!(active_variants.len(), 1, "exactly one shape button should be active");
    assert_eq!(active_variants.pop(), Some("dock"));
}

/// CC-18: `#demoBar.mini-open`/`#infoPanel.open` follow their flags
/// independently of the shape.
#[test]
fn control_panel_mini_and_info_toggle_their_own_class() {
    let mut doc = parse_asset();
    bind_model(
        &mut doc,
        &ChromeModel {
            control_panel: ChromeControlPanelModel { mini_open: true, info_open: true, ..Default::default() },
            ..ChromeModel::default()
        },
    );
    let bar = doc.find_by_id(crate::ids::DEMO_BAR).expect("asset has #demoBar");
    let info = doc.find_by_id(crate::ids::INFO_PANEL).expect("asset has #infoPanel");
    assert!(has_class(&doc, bar, "mini-open"));
    assert!(has_class(&doc, info, "open"));

    bind_model(&mut doc, &ChromeModel::default());
    assert!(!has_class(&doc, bar, "mini-open"));
    assert!(!has_class(&doc, info, "open"));
}

/// CC-18: the floating panel's own profile/layout buttons (distinct
/// nodes from the toolbar avatar popover) mirror
/// `profile_slug`/`layout_vertical`, the same as `body[data-profile]`/
/// `[data-layout]` do.
#[test]
fn control_panel_profile_and_layout_buttons_mirror_the_model() {
    let mut doc = parse_asset();
    bind_model(
        &mut doc,
        &ChromeModel {
            profile_slug: Some("anonymous".to_owned()),
            layout_vertical: false,
            ..ChromeModel::default()
        },
    );
    let bar = doc.find_by_id(crate::ids::DEMO_BAR).expect("asset has #demoBar");
    for node in collect_by_attr(&doc, bar, "data-profile") {
        let is_anon = doc.get(node).get_attr("data-profile") == Some("anonymous");
        assert_eq!(has_class(&doc, node, "active"), is_anon);
    }
    for node in collect_by_attr(&doc, bar, "data-layout") {
        let is_horizontal = doc.get(node).get_attr("data-layout") == Some("horizontal");
        assert_eq!(has_class(&doc, node, "active"), is_horizontal);
    }
}

#[test]
fn history_banner_and_rows_are_rebuilt_from_the_model() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        history: ChromeHistoryModel {
            banner: true,
            rows: vec![
                ChromeHistoryRow::Group("Сегодня".to_owned()),
                ChromeHistoryRow::Entry {
                    title: "Example".to_owned(),
                    url: "example.com".to_owned(),
                    time_label: "14:02".to_owned(),
                },
            ],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let banner = doc.find_by_id(crate::ids::HIST_BANNER).expect("asset has #histBanner");
    assert_eq!(doc.get(banner).get_attr("style"), Some(""));
    let wrap = find_by_class(&doc, "hist-wrap").expect("asset has .hist-wrap");
    let days: Vec<NodeId> = doc.get(wrap).children.iter().copied().filter(|&c| has_class(&doc, c, "hist-day")).collect();
    assert_eq!(days.len(), 1, "old demo groups must be gone, only the 1 model group remains");
    assert_eq!(text_of(&doc, days[0]), "Сегодня");
    let items: Vec<NodeId> = doc.get(wrap).children.iter().copied().filter(|&c| has_class(&doc, c, "hist-item")).collect();
    assert_eq!(items.len(), 1, "old demo entries must be gone, only the 1 model entry remains");
    let title = doc.get(items[0]).children.iter().copied().nth(1).expect("entry has a text wrapper");
    let title_el = doc.get(title).children.iter().copied().find(|&c| has_class(&doc, c, "hist-title")).unwrap();
    assert_eq!(text_of(&doc, title_el), "Example");
}

#[test]
fn history_banner_hidden_when_not_anonymous() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let banner = doc.find_by_id(crate::ids::HIST_BANNER).expect("asset has #histBanner");
    assert_eq!(doc.get(banner).get_attr("style"), Some("display:none"));
}

#[test]
fn bookmarks_folders_and_cards_are_rebuilt_from_the_model() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        bookmarks: ChromeBookmarksModel {
            folders: vec![
                ChromeBookmarkFolderModel {
                    label: "Все закладки".to_owned(),
                    active: false,
                    filter: None,
                },
                ChromeBookmarkFolderModel {
                    label: "Работа".to_owned(),
                    active: true,
                    filter: Some("Работа".to_owned()),
                },
            ],
            title: "Работа".to_owned(),
            cards: vec![ChromeBookmarkCardModel {
                fav_letter: "R".to_owned(),
                title: "Rust".to_owned(),
                url: "rust-lang.org".to_owned(),
            }],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let tree = find_by_class(&doc, "bm-tree").expect("asset has .bm-tree");
    let folders: Vec<NodeId> =
        doc.get(tree).children.iter().copied().filter(|&c| has_class(&doc, c, "bm-folder")).collect();
    assert_eq!(folders.len(), 2, "old demo folders must be gone, only the 2 model folders remain");
    assert!(!has_class(&doc, folders[0], "active"));
    assert!(has_class(&doc, folders[1], "active"));
    assert!(has_class(&doc, folders[1], "indent"), "every folder but the first gets .indent");
    let grid = find_by_class(&doc, "bm-grid").expect("asset has .bm-grid");
    let cards: Vec<NodeId> =
        doc.get(grid).children.iter().copied().filter(|&c| has_class(&doc, c, "bm-card")).collect();
    assert_eq!(cards.len(), 1, "old demo cards must be gone, only the 1 model card remains");
    let title_el = doc.get(cards[0]).children.iter().copied().find(|&c| has_class(&doc, c, "bm-title")).unwrap();
    assert_eq!(text_of(&doc, title_el), "Rust");
}

// ── BUG-422: per-entry actions on the two list views ────────────────────

#[test]
fn bug422_history_rows_carry_open_and_per_row_actions() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        history: ChromeHistoryModel {
            banner: false,
            rows: vec![ChromeHistoryRow::Entry {
                title: "Example".to_owned(),
                url: "https://example.com/a".to_owned(),
                time_label: "14:02".to_owned(),
            }],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let wrap = find_by_class(&doc, "hist-wrap").expect("asset has .hist-wrap");
    let item = doc
        .get(wrap)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "hist-item"))
        .expect("one rebuilt row");
    assert_eq!(doc.get(item).get_attr("data-action"), Some("open-history-entry"));
    assert_eq!(doc.get(item).get_attr("data-hist-url"), Some("https://example.com/a"));

    let actions = doc
        .get(item)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "hist-actions"))
        .expect("the rebuilt row carries the design's action strip");
    let buttons: Vec<&str> = doc
        .get(actions)
        .children
        .iter()
        .filter_map(|&c| doc.get(c).get_attr("data-action"))
        .collect();
    assert_eq!(
        buttons,
        vec!["bookmark-history-entry", "copy-history-entry", "delete-history-entry"]
    );
    // Every button carries its own copy of the row context — the click
    // resolves on the button itself, not on its `.hist-item` ancestor.
    for &b in &doc.get(actions).children.clone() {
        assert_eq!(doc.get(b).get_attr("data-hist-url"), Some("https://example.com/a"));
        assert_eq!(doc.get(b).get_attr("data-hist-title"), Some("Example"));
    }
}

#[test]
fn bug422_history_row_action_buttons_carry_the_sprite_icon() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        history: ChromeHistoryModel {
            banner: false,
            rows: vec![ChromeHistoryRow::Entry {
                title: "Example".to_owned(),
                url: "https://example.com/a".to_owned(),
                time_label: "14:02".to_owned(),
            }],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let wrap = find_by_class(&doc, "hist-wrap").expect("asset has .hist-wrap");
    let item = doc.get(wrap).children.iter().copied().find(|&c| has_class(&doc, c, "hist-item")).unwrap();
    let actions = doc.get(item).children.iter().copied().find(|&c| has_class(&doc, c, "hist-actions")).unwrap();
    let first = doc.get(actions).children[0];
    let svg = doc.get(first).children[0];
    match &doc.get(svg).data {
        NodeData::Element { name, .. } => {
            assert_eq!(name.local, "svg");
            assert_eq!(name.namespace, lumen_dom::Namespace::Svg);
        }
        other => panic!("expected an <svg> element, got {other:?}"),
    }
    let use_el = doc.get(svg).children[0];
    assert_eq!(doc.get(use_el).get_attr("href"), Some("#i-star"));
}

#[test]
fn bug422_bookmark_folders_and_cards_carry_actions() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        bookmarks: ChromeBookmarksModel {
            folders: vec![
                ChromeBookmarkFolderModel {
                    label: "Все закладки".to_owned(),
                    active: true,
                    filter: None,
                },
                ChromeBookmarkFolderModel {
                    label: "/Work".to_owned(),
                    active: false,
                    filter: Some("/Work".to_owned()),
                },
            ],
            title: "Все закладки".to_owned(),
            cards: vec![ChromeBookmarkCardModel {
                fav_letter: "R".to_owned(),
                title: "Rust".to_owned(),
                url: "https://rust-lang.org/".to_owned(),
            }],
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);

    let tree = find_by_class(&doc, "bm-tree").expect("asset has .bm-tree");
    let folders: Vec<NodeId> =
        doc.get(tree).children.iter().copied().filter(|&c| has_class(&doc, c, "bm-folder")).collect();
    assert_eq!(doc.get(folders[0]).get_attr("data-action"), Some("select-folder"));
    // The root row's filter is empty, not absent — an absent attribute is
    // how `dispatch_chrome_action` recognises a stale/unbound asset row.
    assert_eq!(doc.get(folders[0]).get_attr("data-bm-folder"), Some(""));
    assert_eq!(doc.get(folders[1]).get_attr("data-bm-folder"), Some("/Work"));

    let grid = find_by_class(&doc, "bm-grid").expect("asset has .bm-grid");
    let card = doc
        .get(grid)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "bm-card"))
        .expect("one rebuilt card");
    assert_eq!(doc.get(card).get_attr("data-action"), Some("open-bookmark"));
    assert_eq!(doc.get(card).get_attr("data-bm-url"), Some("https://rust-lang.org/"));
    let actions = doc
        .get(card)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "bm-actions"))
        .expect("the rebuilt card carries a delete button");
    let del = doc.get(actions).children[0];
    assert_eq!(doc.get(del).get_attr("data-action"), Some("delete-bookmark"));
    assert_eq!(doc.get(del).get_attr("data-bm-url"), Some("https://rust-lang.org/"));
}

#[test]
fn settings_section_toggles_nav_and_section_active_class() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        settings: ChromeSettingsModel { active_section: "appearance".to_owned(), ..ChromeSettingsModel::default() },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let nav = find_by_class(&doc, "set-nav").expect("asset has .set-nav");
    let active_items: Vec<NodeId> =
        doc.get(nav).children.iter().copied().filter(|&c| has_class(&doc, c, "active")).collect();
    assert_eq!(active_items.len(), 1);
    assert_eq!(doc.get(active_items[0]).get_attr("data-section"), Some("appearance"));
    let main = find_by_class(&doc, "set-main").expect("asset has .set-main");
    let active_sections: Vec<NodeId> =
        doc.get(main).children.iter().copied().filter(|&c| has_class(&doc, c, "active")).collect();
    assert_eq!(active_sections.len(), 1);
    assert_eq!(doc.get(active_sections[0]).get_attr("data-set"), Some("appearance"));
}

#[test]
fn settings_autofill_rows_follow_the_model() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
    let html = std::fs::read_to_string(&path).expect("chrome.html");
    let mut doc = lumen_html_parser::parse(&html);
    let model = ChromeModel {
        settings: ChromeSettingsModel {
            active_section: "autofill".to_owned(),
            autofill: vec![("https://a.test".into(), "email".into(), "a@b.c".into())],
            cards: vec![("1111".into(), "•••• 1111".into())],
            autofill_available: true,
            ..ChromeSettingsModel::default()
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let list = doc.find_by_id(crate::ids::AF_LIST).expect("#afList");
    let rows = doc.get(list).children.clone();
    assert_eq!(rows.len(), 2, "образец заменён строками модели: карта, затем значение");
    let card_del = find_descendants_by_class(&doc, rows[0], "upd-check")[0];
    assert_eq!(doc.get(card_del).get_attr("data-action"), Some("delete-saved-card"));
    assert_eq!(doc.get(card_del).get_attr("data-card-last4"), Some("1111"));
    let del = find_descendants_by_class(&doc, rows[1], "upd-check")[0];
    assert_eq!(doc.get(del).get_attr("data-action"), Some("delete-autofill-entry"));
    assert_eq!(doc.get(del).get_attr("data-af-origin"), Some("https://a.test"));
    assert_eq!(doc.get(del).get_attr("data-af-field"), Some("email"));
    assert_eq!(doc.get(del).get_attr("data-af-value"), Some("a@b.c"));
}

#[test]
fn settings_site_permissions_rows_follow_the_model() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
    let html = std::fs::read_to_string(&path).expect("chrome.html");
    let mut doc = lumen_html_parser::parse(&html);
    let model = ChromeModel {
        settings: ChromeSettingsModel {
            active_section: "permissions".to_owned(),
            site_permissions: vec![("https://a.test".into(), "Camera".into(), true)],
            ..ChromeSettingsModel::default()
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let list = doc.find_by_id(crate::ids::PERM_LIST).expect("#permList");
    let rows = doc.get(list).children.clone();
    assert_eq!(rows.len(), 1, "образец заменён строкой модели");
    let del = find_descendants_by_class(&doc, rows[0], "upd-check")[0];
    assert_eq!(doc.get(del).get_attr("data-action"), Some("revoke-site-permission"));
    assert_eq!(doc.get(del).get_attr("data-perm-origin"), Some("https://a.test"));
    assert_eq!(doc.get(del).get_attr("data-perm-kind"), Some("Camera"));
}

#[test]
fn settings_spelling_rows_follow_the_model() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
    let html = std::fs::read_to_string(&path).expect("chrome.html");
    let mut doc = lumen_html_parser::parse(&html);
    let model = ChromeModel {
        settings: ChromeSettingsModel {
            active_section: "spelling".to_owned(),
            spell_enabled: false,
            spell_languages: vec![("en_US".into(), true), ("ru_RU".into(), false)],
            ..ChromeSettingsModel::default()
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let toggle = doc.find_by_id(crate::ids::SPELL_TOGGLE).expect("#spellToggle");
    assert!(!has_class(&doc, toggle, "on"));
    let list = doc.find_by_id(crate::ids::SPELL_LANG_LIST).expect("#spellLangList");
    let rows = doc.get(list).children.clone();
    assert_eq!(rows.len(), 2, "статичная демо-строка заменена строками модели");
    let ru = find_descendants_by_class(&doc, rows[1], "toggle")[0];
    assert_eq!(doc.get(ru).get_attr("data-action"), Some("toggle-spell-language"));
    assert_eq!(doc.get(ru).get_attr("data-spell-lang"), Some("ru_RU"));
    assert!(!has_class(&doc, ru, "on"));
    let en = find_descendants_by_class(&doc, rows[0], "toggle")[0];
    assert!(has_class(&doc, en, "on"));
}

#[test]
fn settings_translate_rows_follow_the_model() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
    let html = std::fs::read_to_string(&path).expect("chrome.html");
    let mut doc = lumen_html_parser::parse(&html);
    let model = ChromeModel {
        settings: ChromeSettingsModel {
            active_section: "translate".to_owned(),
            translate_languages: vec![("ru".into(), "Русский".into(), false), ("de".into(), "Deutsch".into(), true)],
            translate_model: "phi3:mini".to_owned(),
            ..ChromeSettingsModel::default()
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let list = doc.find_by_id(crate::ids::TRANSLATE_LANG_LIST).expect("#translateLangList");
    let rows = doc.get(list).children.clone();
    assert_eq!(rows.len(), 2);
    let de = find_descendants_by_class(&doc, rows[1], "toggle")[0];
    assert_eq!(doc.get(de).get_attr("data-action"), Some("set-translate-language"));
    assert_eq!(doc.get(de).get_attr("data-lang"), Some("de"));
    assert!(has_class(&doc, de, "on"));
    let ru = find_descendants_by_class(&doc, rows[0], "toggle")[0];
    assert!(!has_class(&doc, ru, "on"));
}

#[test]
fn settings_site_data_button_carries_current_site() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
    let html = std::fs::read_to_string(&path).expect("chrome.html");
    let mut doc = lumen_html_parser::parse(&html);
    let model = ChromeModel {
        settings: ChromeSettingsModel {
            active_section: "permissions".to_owned(),
            current_site: Some("a.test".into()),
            ..ChromeSettingsModel::default()
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let btn = doc.find_by_id(crate::ids::SITE_DATA_BTN).expect("#siteDataBtn");
    assert_eq!(doc.get(btn).get_attr("data-action"), Some("clear-site-data"));
    assert_eq!(doc.get(btn).get_attr("data-site"), Some("a.test"));
}

#[test]
fn settings_passwords_rows_follow_the_model() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
    let html = std::fs::read_to_string(&path).expect("chrome.html");
    let mut doc = lumen_html_parser::parse(&html);
    let model = ChromeModel {
        settings: ChromeSettingsModel {
            active_section: "passwords".to_owned(),
            passwords: vec![("https://a.test".into(), "anna".into()), ("https://b.test".into(), String::new())],
            passwords_available: true,
            ..ChromeSettingsModel::default()
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let list = doc.find_by_id(crate::ids::PW_LIST).expect("#pwList");
    let rows = doc.get(list).children.clone();
    assert_eq!(rows.len(), 2, "статический образец заменён строками модели");
    let del = find_descendants_by_class(&doc, rows[0], "upd-check")[0];
    assert_eq!(doc.get(del).get_attr("data-action"), Some("delete-saved-login"));
    assert_eq!(doc.get(del).get_attr("data-login-origin"), Some("https://a.test"));
    assert_eq!(doc.get(del).get_attr("data-login-user"), Some("anna"));
}

#[test]
fn settings_privacy_toggles_reflect_ad_block_and_fingerprint_state() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        settings: ChromeSettingsModel {
            active_section: "privacy".to_owned(),
            ad_block_on: true,
            fingerprint_on: false,
            ..ChromeSettingsModel::default()
        },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let main = find_by_class(&doc, "set-main").expect("asset has .set-main");
    let privacy = doc
        .get(main)
        .children
        .iter()
        .copied()
        .find(|&c| doc.get(c).get_attr("data-set") == Some("privacy"))
        .expect("asset has the privacy section");
    let toggles = find_descendants_by_class(&doc, privacy, "toggle");
    assert!(toggles.len() >= 2, "privacy section must have at least the 2 mapped toggles");
    assert!(has_class(&doc, toggles[0], "on"), "ad_block_on must set the first toggle's .on class");
    assert!(!has_class(&doc, toggles[1], "on"), "fingerprint_on=false must clear the second toggle's .on class");
}

#[test]
fn right_sidebar_open_and_tab_are_bound() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        right_sidebar: ChromeRightSidebarModel { open: true, tab: ChromeSidebarTab::Web },
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let panel = doc.find_by_id(crate::ids::RIGHT_SIDEBAR).expect("asset has #rightSidebar");
    assert!(has_class(&doc, panel, "open"));
    let title = doc.find_by_id(crate::ids::RS_TITLE).expect("asset has #rsTitle");
    assert_eq!(text_of(&doc, title), "Web");
    let ai_body = doc.find_by_id(crate::ids::RS_BODY_AI).expect("asset has #rsBodyAi");
    assert_eq!(doc.get(ai_body).get_attr("style"), Some("display:none"));
    let web_body = doc.find_by_id(crate::ids::RS_BODY_WEB).expect("asset has #rsBodyWeb");
    assert_eq!(doc.get(web_body).get_attr("style"), Some(""));
}

#[test]
fn right_sidebar_closed_hides_open_class() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel::default());
    let panel = doc.find_by_id(crate::ids::RIGHT_SIDEBAR).expect("asset has #rightSidebar");
    assert!(!has_class(&doc, panel, "open"));
}
