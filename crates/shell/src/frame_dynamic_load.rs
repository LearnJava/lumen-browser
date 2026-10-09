//! FRAME-8: обнаружение и первичная загрузка фреймов, которых не видел
//! разовый проход [`frames::load_frame_sub_documents`] — вставленных
//! host-элементов и смены `src` уже вставленных. Выделено из `frames.rs`
//! отдельным файлом: тот уже превышал лимит `docs/conventions.md` (2000
//! строк) до этой задачи и расти дальше не должен.
//!
//! Диспетчеризация (когда и с какого потока звать [`scan_dynamic_frames`]/
//! [`run_new_frame_load`]) — `crates/shell/src/lumen/frame_dynamic.rs`, этот
//! файл — только чистые функции без доступа к `Lumen`.

use crate::*;
use crate::frames::{FrameHandle, FrameLoadEnv};

/// Что изменилось в дереве фреймов документа `doc` со времени последнего
/// скана — новые host-элементы без хэндла и хэндлы, чей `src` скрипт
/// поменял. HTML LS §4.8.5 «process the iframe attributes» запускается при
/// вставке узла и при каждом изменении `src`, а не один раз при разборе
/// документа — ровно то, чего [`frames::load_frame_sub_documents`] не делает
/// (единственный вызов, `page_pipeline.rs`, до первого события `load`).
pub(crate) struct DynamicFrameDelta {
    /// Host-элементы без хэндла в `frames` — требуют первичной загрузки.
    pub(crate) new: Vec<lumen_dom::IframeInfo>,
    /// (индекс в `frames`, новое значение `src`) — хэндл есть, но `src`
    /// сменился на непустое значение с прошлого скана.
    pub(crate) changed: Vec<(usize, String)>,
}

/// Сверить живое дерево `doc` со списком уже загруженных `frames`.
///
/// `is_top` отличает СТРАНИЦУ (`FrameHandle::parent_doc == None`) от
/// документа конкретного фрейма — `frames` плоский список, и хэндл читается
/// только если его `parent_doc` указывает именно на `doc` (или отсутствует
/// при `is_top`), иначе `NodeId` из ДРУГОГО документа с тем же числовым
/// индексом дал бы ложное совпадение.
///
/// Смена `src` детектируется сравнением с [`FrameHandle::host_src`] — сырым
/// значением атрибута на момент последней загрузки этого хэндла — а не с
/// разрешённым [`FrameHandle::url`], который живёт уже в другой форме
/// (абсолютный адрес vs `about:blank`/`about:srcdoc`). Пустой live `src`
/// (атрибут снят) реакцию не запускает — спека этого случая не описывает
/// точной навигацией, а движок сегодня не умеет «обесфреймить» хэндл.
///
/// BUG-935 срез 87: `None` — документ занят (движковый поток держит его на
/// JS-ходе или флаше — секунды на ria.ru, где скрипты вставляют `<iframe>` с
/// рекламой), скан отложен. Раньше здесь стоял блокирующий `lock()`, и
/// `pump_raf_engine_thread` на UI-потоке стоял за чужим JS: кадры по 0,7–2,3 с,
/// почти целиком — этот скан. Вызывающий повторит скан на следующем проходе.
pub(crate) fn scan_dynamic_frames(
    doc: &Arc<Mutex<Document>>,
    frames: &[FrameHandle],
    is_top: bool,
) -> Option<DynamicFrameDelta> {
    let infos = match doc.try_lock() {
        Ok(d) => collect_iframes(&d),
        Err(std::sync::TryLockError::Poisoned(p)) => collect_iframes(&p.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    let mut delta = DynamicFrameDelta { new: Vec::new(), changed: Vec::new() };
    for info in infos {
        let existing = frames.iter().enumerate().find(|(_, h)| {
            h.host == info.node
                && match &h.parent_doc {
                    None => is_top,
                    Some(pd) => Arc::ptr_eq(pd, doc),
                }
        });
        match existing {
            None if info.loading_lazy => {}
            None => delta.new.push(info),
            Some((idx, h)) => {
                let live_src = info.src.clone().unwrap_or_default();
                if !live_src.is_empty() && live_src != h.host_src {
                    delta.changed.push((idx, live_src));
                }
            }
        }
    }
    Some(delta)
}

/// Всё, что [`run_new_frame_load`] нужно от нового host-элемента, снятое ДО
/// фоновой загрузки — доля `frames::FrameNavPrep` для случая, когда хэндла
/// ещё нет и заменять нечего.
pub(crate) struct FrameNewLoadPrep {
    host: NodeId,
    depth: usize,
    host_doc: Arc<Mutex<Document>>,
    host_base: ResourceBase,
    parent_js: Option<Arc<dyn PersistentJs>>,
    info: lumen_dom::IframeInfo,
}

/// Снять описание нового host-элемента — синхронная, быстрая часть; сеть и
/// парсинг остаются в [`run_new_frame_load`], которую вызывающая сторона
/// уносит на фоновый поток тем же приёмом, что и `frames::run_frame_navigation`.
pub(crate) fn prepare_new_frame_load(
    info: lumen_dom::IframeInfo,
    host_doc: &Arc<Mutex<Document>>,
    depth: usize,
    host_base: &ResourceBase,
    parent_js: Option<&Arc<dyn PersistentJs>>,
) -> FrameNewLoadPrep {
    FrameNewLoadPrep {
        host: info.node,
        depth,
        host_doc: Arc::clone(host_doc),
        host_base: host_base.clone(),
        parent_js: parent_js.cloned(),
        info,
    }
}

/// Host-узел, который [`prepare_new_frame_load`] снял — идентификатор для
/// антидубликатной брони `Lumen::pending_new_frames` на UI-потоке.
pub(crate) fn new_frame_load_key(prep: &FrameNewLoadPrep) -> (Arc<Mutex<Document>>, NodeId) {
    (Arc::clone(&prep.host_doc), prep.host)
}

/// Загрузить ОДИН новый под-документ — тот же `frames::spawn_frame`, что и
/// первичный проход `load_frame_sub_documents`, для host-элемента,
/// вставленного или получившего `src` уже ПОСЛЕ него.
pub(crate) fn run_new_frame_load(
    prep: &FrameNewLoadPrep,
    top_doc: &Arc<Mutex<Document>>,
    env: &FrameLoadEnv,
) -> Vec<FrameHandle> {
    frames::spawn_frame(
        &prep.info,
        None,
        &prep.host_doc,
        prep.depth,
        &prep.host_base,
        top_doc,
        env,
        prep.parent_js.as_ref(),
        None,
        None,
    )
}

/// Вклеить результат [`run_new_frame_load`] в `frames` на UI-потоке — в
/// отличие от `frames::apply_frame_navigation` это ДОБАВЛЕНИЕ, не замена:
/// хэндла с этим host-узлом раньше не было.
///
/// `false`, если `host` тем временем обзавёлся хэндлом сам (повторный скан
/// успел заметить и загрузить его другим путём, гонка с самим собой не
/// исключена при двух почти одновременных мутациях) — тогда `handles`
/// просто роняются вместе со своими JS-рантаймами.
pub(crate) fn apply_new_frame_load(
    frames: &mut Vec<FrameHandle>,
    host_doc: &Arc<Mutex<Document>>,
    host: NodeId,
    is_top: bool,
    handles: Vec<FrameHandle>,
) -> bool {
    if handles.is_empty()
        || frames.iter().any(|h| {
            h.host == host
                && match &h.parent_doc {
                    None => is_top,
                    Some(pd) => Arc::ptr_eq(pd, host_doc),
                }
        })
    {
        return false;
    }
    frames.extend(handles);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BUG-935 срез 87: скан не ждёт документ, занятый движковым потоком.
    #[test]
    fn scan_skips_a_document_held_by_another_thread() {
        let doc = Arc::new(Mutex::new(Document::new()));
        let held = doc.lock().unwrap();
        assert!(scan_dynamic_frames(&doc, &[], true).is_none(), "занятый документ — скан отложен");
        drop(held);
        let delta = scan_dynamic_frames(&doc, &[], true).expect("свободный документ сканируется");
        assert!(delta.new.is_empty() && delta.changed.is_empty());
    }
}
