//! `#updateBar` self-update infobar + the settings-page "Обновления" block
//! (UPD-9) — bound from [`ChromeUpdateModel`].
//!
//! A child module of `model` so it reuses the tracked DOM primitives
//! (`set_attr`/`set_text`/`set_class_token`) instead of duplicating them —
//! every write here is therefore reported to `bind_model_tracked` exactly like
//! the rest of `model.rs`.

use lumen_dom::Document;

use super::{set_class_token, set_text};

/// Which buttons `#updateBar` shows — the shell's update stage reduced to what
/// the markup can express.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChromeUpdateAction {
    /// No action button: a download is running, or the bar only reports an
    /// error.
    #[default]
    None,
    /// `#updDownloadBtn` («Скачать») — a verified newer version is available.
    Download,
    /// `#updRestartBtn` («Перезапустить и обновить») — the archive is staged.
    Restart,
}

/// `#updateBar` + settings "Обновления" snapshot (UPD-9) — mirrors the shell's
/// `update_ui::UpdateUi`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeUpdateModel {
    /// `true` shows `#updateBar` (`.open`).
    pub bar_open: bool,
    /// `#updTitle` text, e.g. «Доступна версия 0.6.0».
    pub title: String,
    /// `#updMeta` text — current version, progress or the failure reason.
    pub meta: String,
    /// Which action button is visible.
    pub action: ChromeUpdateAction,
    /// `#updAutoToggle`'s `.on` class — `UpdateState::auto_check_updates`.
    pub auto_check: bool,
    /// `#updCheckStatus` text — the last manual/automatic check's result.
    pub check_status: String,
}

/// Binds [`ChromeUpdateModel`] into `#updateBar` and the settings-page
/// update rows. Every lookup is by id, so a missing node (an older asset in a
/// test fixture) is skipped rather than treated as an error.
pub(super) fn bind_update(doc: &mut Document, update: &ChromeUpdateModel) {
    if let Some(bar) = doc.find_by_id(crate::ids::UPDATE_BAR) {
        set_class_token(doc, bar, "open", update.bar_open);
    }
    if let Some(title) = doc.find_by_id(crate::ids::UPD_TITLE) {
        set_text(doc, title, &update.title);
    }
    if let Some(meta) = doc.find_by_id(crate::ids::UPD_META) {
        set_text(doc, meta, &update.meta);
    }
    if let Some(btn) = doc.find_by_id(crate::ids::UPD_DOWNLOAD_BTN) {
        set_class_token(doc, btn, "hidden", update.action != ChromeUpdateAction::Download);
    }
    if let Some(btn) = doc.find_by_id(crate::ids::UPD_RESTART_BTN) {
        set_class_token(doc, btn, "hidden", update.action != ChromeUpdateAction::Restart);
    }
    if let Some(toggle) = doc.find_by_id(crate::ids::UPD_AUTO_TOGGLE) {
        set_class_token(doc, toggle, "on", update.auto_check);
    }
    if let Some(status) = doc.find_by_id(crate::ids::UPD_CHECK_STATUS) {
        set_text(doc, status, &update.check_status);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{bind_model, bind_model_tracked, has_class, ChromeModel};
    use super::*;

    fn parse_asset() -> Document {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        crate::parse_document(&html).0
    }

    fn text_of(doc: &Document, id: &str) -> String {
        let node = doc.find_by_id(id).expect("asset has the id");
        doc.get(node)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                lumen_dom::NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn default_model_hides_the_bar() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let bar = doc.find_by_id(crate::ids::UPDATE_BAR).expect("asset has #updateBar");
        assert!(!has_class(&doc, bar, "open"));
    }

    #[test]
    fn available_update_shows_download_button_only() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            update: ChromeUpdateModel {
                bar_open: true,
                title: "Доступна версия 9.9.9".to_owned(),
                meta: "Установлена 0.5.0".to_owned(),
                action: ChromeUpdateAction::Download,
                auto_check: false,
                check_status: "Доступна версия 9.9.9".to_owned(),
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let bar = doc.find_by_id(crate::ids::UPDATE_BAR).expect("asset has #updateBar");
        assert!(has_class(&doc, bar, "open"));
        assert_eq!(text_of(&doc, crate::ids::UPD_TITLE), "Доступна версия 9.9.9");
        assert_eq!(text_of(&doc, crate::ids::UPD_META), "Установлена 0.5.0");
        let dl = doc.find_by_id(crate::ids::UPD_DOWNLOAD_BTN).expect("asset has #updDownloadBtn");
        let rs = doc.find_by_id(crate::ids::UPD_RESTART_BTN).expect("asset has #updRestartBtn");
        assert!(!has_class(&doc, dl, "hidden"));
        assert!(has_class(&doc, rs, "hidden"));
        let toggle = doc.find_by_id(crate::ids::UPD_AUTO_TOGGLE).expect("asset has #updAutoToggle");
        assert!(!has_class(&doc, toggle, "on"), "the asset's static `.on` must follow the model");
        assert_eq!(text_of(&doc, crate::ids::UPD_CHECK_STATUS), "Доступна версия 9.9.9");
    }

    #[test]
    fn staged_update_swaps_to_restart_button() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            update: ChromeUpdateModel {
                bar_open: true,
                action: ChromeUpdateAction::Restart,
                ..ChromeUpdateModel::default()
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let dl = doc.find_by_id(crate::ids::UPD_DOWNLOAD_BTN).expect("asset has #updDownloadBtn");
        let rs = doc.find_by_id(crate::ids::UPD_RESTART_BTN).expect("asset has #updRestartBtn");
        assert!(has_class(&doc, dl, "hidden"));
        assert!(!has_class(&doc, rs, "hidden"));
    }

    /// An unchanged rebind must report nothing — the bar is bound on every
    /// chrome relayout, and a spurious touch would cost incremental reuse.
    #[test]
    fn identical_rebind_reports_no_mutation() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            update: ChromeUpdateModel {
                bar_open: true,
                title: "t".to_owned(),
                meta: "m".to_owned(),
                action: ChromeUpdateAction::Download,
                auto_check: true,
                check_status: "s".to_owned(),
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let touched = bind_model_tracked(&mut doc, &model);
        assert!(touched.is_empty(), "rebind of an identical model touched {touched:?}");
    }
}
