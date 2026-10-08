//! BUG-1198: регистрация связей фрейма с предками ДО первой строки его
//! скрипта.
//!
//! Раньше `frames::spawn_frame` ставил их после `run_scripts_with_dom`, то есть
//! после инлайн-скриптов ребёнка: такой скрипт видел fallback
//! `window.top === window`, и `window.top.postMessage(…)` из srcdoc-фрейма
//! уходил самому ребёнку, а родитель ждал его вечно (WPT
//! `origin-from-messageevent-opaque`). По HTML LS §7.3.1 вложенный navigable
//! со своими предками существует раньше, чем его документ исполнит хоть один
//! скрипт.
//!
//! Две стороны, обе здесь:
//! - в контексте ребёнка — слоты `parent`/`top` (`window.parent`/`top`/
//!   `frameElement`/`name`);
//! - в контексте родителя — биндинг самого ребёнка (`contentWindow`), без
//!   `peer`: рантайма ребёнка как `PersistentJs` ещё нет. Без него сообщение,
//!   поставленное инлайн-скриптом ребёнка, родитель, разбирающий ящик на
//!   своём потоке, мог забрать раньше, чем `spawn_frame` зарегистрирует
//!   ребёнка, — и доставил бы его с `source === null`. `spawn_frame` после
//!   скриптов регистрирует биндинг ещё раз, уже с `peer`; повтор замещает
//!   запись на месте (`frame_bridge::upsert_binding`).

use crate::*;

/// Всё, что нужно для регистрации связей фрейма с предками, снятое
/// `frames::spawn_frame` до исполнения скриптов ребёнка.
pub(crate) struct FrameAncestry<'a> {
    /// Контекст родителя — адресат биндинга ребёнка (`contentWindow`).
    pub(crate) parent_js: Option<&'a Arc<dyn PersistentJs>>,
    /// nid host-элемента в дереве родителя.
    pub(crate) host_nid: u32,
    /// Снимок атрибута `name` хоста (BUG-921).
    pub(crate) name: Option<&'a str>,
    /// Адрес документа ребёнка.
    pub(crate) child_url: &'a str,
    /// Родитель и ребёнок взаимно доступны (same-origin, не opaque-sandbox).
    pub(crate) accessible: bool,
    /// У ребёнка непрозрачное происхождение (`sandbox` без
    /// `allow-same-origin`).
    pub(crate) opaque: bool,
    pub(crate) parent_doc: &'a Arc<Mutex<Document>>,
    pub(crate) parent_url: &'a str,
    /// BUG-979: хэндл родителя для синхронного чтения его глобалов; только
    /// same-origin.
    pub(crate) parent_peer: Option<Arc<dyn lumen_js::frame_peer_bridge::FramePeerBridge>>,
    /// Слот верха (документ, адрес, доступность) — только у фрейма глубины
    /// ≥ 1: у фрейма первого уровня `top` разрешается через слот родителя.
    pub(crate) top: Option<(&'a Arc<Mutex<Document>>, &'a str, bool)>,
}

impl FrameAncestry<'_> {
    /// Зарегистрировать обе стороны. Вызывается `run_scripts_with_dom` после
    /// `install_dom` рантайма ребёнка `rt` (шим с `_lumen_frame_install_*`
    /// уже стоит) и до первого парсерного скрипта; `doc` — документ ребёнка.
    #[cfg(feature = "v8")]
    pub(crate) fn register(&self, doc: &Arc<Mutex<Document>>, rt: &lumen_js::v8_runtime::V8JsRuntime) {
        if let Some(js) = self.parent_js {
            js.register_iframe_document(
                self.host_nid,
                Arc::clone(doc),
                self.child_url,
                self.name,
                self.accessible,
                self.opaque,
                None,
            );
        }
        rt.register_parent_document(
            self.host_nid,
            Arc::clone(self.parent_doc),
            self.parent_url.to_owned(),
            self.name.map(str::to_owned),
            self.accessible,
            self.parent_peer.clone(),
        );
        if let Some((top_doc, top_url, accessible_top)) = self.top {
            rt.register_top_document(Arc::clone(top_doc), top_url.to_owned(), accessible_top, None);
        }
    }
}
