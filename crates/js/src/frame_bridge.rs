//! BUG-480 срез 2 — мост `contentWindow`/`contentDocument` из JS родителя в
//! под-документ `<iframe>` (HTML LS §4.8.5 content navigable).
//!
//! Срез 1 (`load_frame_sub_documents`, shell) загружает каждый статический
//! `<iframe>` в отдельный `Document` с собственным V8-контекстом. Изолят у
//! каждого контекста свой, поэтому прямой передачи объектов между окнами нет
//! и не будет (см. commit-message среза 1); вместо этого этот модуль держит в
//! **родительском** изоляте реестр биндингов «хост-элемент → под-документ» и
//! семейство нативов `_lumen_f_*`, читающих под-документ через его
//! `Arc<Mutex<Document>>`. JS-фасады (Window/Document/Element) строятся шимом
//! поверх этих нативов и интернируются, поэтому `iframe.contentWindow ===`
//! `iframe.contentWindow`, а `contentDocument.defaultView === contentWindow`.
//!
//! Поток данных: shell после загрузки ребёнка вызывает
//! [`V8JsRuntime::register_frame_document`] (до диспатча trusted `load` на
//! хосте), биндинг ложится в реестр — геттеры из `iframe_element.rs` начинают
//! видеть фасады сразу, патчить враппер хоста не нужно.
//!
//! BUG-480 срез 3 — иерархия окон поверх того же реестра:
//! - у **ребёнка** слоты `parent`/`top` заполняются биндингами на документы
//!   предков (`register_parent_document`/`register_top_document`) — шим
//!   переопределяет `window.parent`/`window.top`/`window.frameElement`/
//!   `window.name` геттерами с честным fallback на прежнее поведение, пока
//!   слотов нет;
//! - в **каждом** контексте `window.length` становится живым счётчиком
//!   дочерних фреймов, а `register_frame_document` ставит на `window`
//!   индексный (`window[0]`…) и именованный (`window[имя]`) доступники;
//!   `window.frames === window` остаётся самоссылкой (спека), индексный
//!   доступ идёт через неё.
//!
//! Границы среза 3:
//! - фасады не пробрасывают вызов функций между изолятами: `parent.foo()`,
//!   где `foo` объявлена скриптом родителя, не работает (читать свойства
//!   документа родителя можно);
//! - `length`/индексные доступники фасадов остаются нулём/пустыми: счёт
//!   фреймов чужого изолята недоступен, динамический `length` есть только у
//!   настоящего `window` контекста;
//! - доступ только чтение; мутации из родителя и события — будущие срезы;
//! - cross-origin / opaque-sandbox (`sandbox` без `allow-same-origin`)
//!   биндинги регистрируются с `accessible: false`: `contentWindow` отдаёт
//!   фасад без `.document` (спека: WindowProxy доступен всегда),
//!   `contentDocument` — `null`;
//! - фреймы без загруженного под-документа (динамически созданные `<iframe>`,
//!   неудавшийся fetch) биндинга не имеют — оба геттера дают `null`;
//! - геометрия (`offsetWidth`, `getBoundingClientRect`) — честные нули:
//!   layout содержимого фрейма — отдельный срез.
//!
//! BUG-480 срез 4 — `postMessage` через границу изолятов (HTML LS §9.2.9):
//! - у каждого фасада окна есть `postMessage(message, targetOrigin)`; сообщение
//!   уходит в глобальный исходящий ящик ([`FRAME_OUTBOX`], ключ адресата —
//!   указатель его `Arc<Mutex<Document>>`, один и тот же инстанс у shell,
//!   реестра родителя и JS-контекста ребёнка — см. срез 1);
//! - получатель разбирает свой ящик в `_lumen_frame_pump_messages()`, которую
//!   shell вызывает на каждом тике рядом с pump_websockets и т.д. (и для
//!   страницы, и для хэндлов фреймов); доставка асинхронная, как task;
//! - `targetOrigin`: `'*'` — всегда; `'/'` (и опущенный аргумент) — только
//!   same-origin по уже вычисленному shell'ом флагу `accessible`; явная строка
//!   — точное совпадение с нормализованным origin URL биндинга;
//! - `event.source` — фасад окна отправителя в реестре получателя,
//!   `event.origin` — origin отправителя (для `about:srcdoc`/`about:blank`
//!   детей — origin родителя-получателя, как по спеке про наследование);
//! - сериализация данных — JSON-круготрип: примитивы, массивы и plain object'ы
//!   ходят честно; функции/символы бросают `DataCloneError` на отправке,
//!   вложенные функции/узлы DOM деградируют до `null`/`{}` (подмножество
//!   structured clone — отклонение задокументировано в баг-файле);
//! - срез 4 покрывает рёбра «предок ↔ непосредственный потомок»: постинг
//!   внком на `window.top` доставляется, но `event.source` у верха для внука
//!   — `null` (внук не лежит в прямых слотах top), sibling↔sibling — будущий
//!   срез.

#[cfg(feature = "v8-backend")]
use std::sync::{Arc, Mutex, OnceLock};

/// Псевдо-bid слота «окно родителя» в реестре ([`FrameDocSlots::parent`]).
///
/// Обычные биндинги адресуются индексом в `frames`; специальные значения
/// сверху диапазона `u32` позволяют всему семейству нативов `_lumen_f_*`
/// работать со документом предка без второго семейства функций.
#[cfg(feature = "v8-backend")]
pub(crate) const PARENT_BID: u32 = u32::MAX;

/// Псевдо-bid слота «верхнее окно» в реестре ([`FrameDocSlots::top`]).
/// Заполняется только для фреймов глубины ≥ 2: у фрейма первого уровня
/// `parent === top`, и обе геттер-цепочки ведут через [`PARENT_BID`].
#[cfg(feature = "v8-backend")]
pub(crate) const TOP_BID: u32 = u32::MAX - 1;

/// Один зарегистрированный под-документ `<iframe>` в реестре рантайма.
///
/// Живёт в [`FrameDocSlots`] столько же, сколько контекст страницы:
/// биндинги никогда не удаляются по одному — замена страницы уносит весь
/// рантайм вместе с реестром (тот же lifecycle, что у [`crate::img_bitmap_store`]).
#[cfg(feature = "v8-backend")]
pub(crate) struct FrameDocBinding {
    /// `NodeId` элемента-хоста `<iframe>` в документе родителя.
    ///
    /// Для биндингов-предков ([`PARENT_BID`]) — nid хоста в документе родителя:
    /// по нему ребёнок строит `window.frameElement`. У топового слота хоста
    /// нет — поле заполнено условным значением и не читается.
    pub(crate) host_nid: u32,
    /// Под-документ. Отдельный `Arc` — его же держит shell (`FrameHandle.doc`)
    /// и JS-контекст самого ребёнка.
    pub(crate) doc: Arc<Mutex<lumen_dom::Document>>,
    /// Разрешённый адрес под-документа (`about:srcdoc`/`about:blank`/URL).
    pub(crate) url: String,
    /// Значение атрибута `name` хоста, если задан — ключ именованного доступа
    /// `window[name]` (срез 3).
    pub(crate) name: Option<String>,
    /// `false` — cross-origin или opaque sandbox: нативы чтения отдают пустые
    /// результаты, `.document` фасада окна — `null`.
    pub(crate) accessible: bool,
}

/// Реестр биндингов одного V8-изолята: дочерние фреймы + ссылки на предков.
///
/// `frames` — под-документы этого контекста (индекс = стабильный `bid` для
/// нативов `_lumen_f_*`, порядок регистрации = порядок документа).
/// `parent`/`top` — документы предков для контекста самого фрейма (срез 3):
/// заполняются shell-ом через `register_parent_document`/`register_top_document`.
#[cfg(feature = "v8-backend")]
#[derive(Default)]
pub(crate) struct FrameDocSlots {
    pub(crate) frames: Vec<FrameDocBinding>,
    pub(crate) parent: Option<FrameDocBinding>,
    pub(crate) top: Option<FrameDocBinding>,
    /// Срез 4: указатель `Arc` собственного документа этого контекста —
    /// ключ получателя в [`FRAME_OUTBOX`]. Заполняется `install_dom`
    /// (`Arc::as_ptr(&doc)`); у минимальных тестовых изолятов без
    /// `install_dom` остаётся `None`, и `_lumen_frame_take_messages`
    /// ничего не отдаёт.
    pub(crate) self_key: Option<usize>,
    /// Срез 4: нормализованный origin собственной страницы — им заменяется
    /// origin `about:`-биндинга при вычислении `event.origin` (наследование
    /// origin у srcdoc/about:blank детей).
    pub(crate) self_origin: String,
}

/// Общий `Arc` между `V8JsRuntime`, нативами этого модуля и вызовами
/// `register_*_document`.
#[cfg(feature = "v8-backend")]
pub(crate) type FrameDocRegistry = Arc<Mutex<FrameDocSlots>>;

// ── Срез 4: исходящий ящик кросс-фреймовых postMessage ───────────────────────

/// Отправитель сообщения в [`PendingFrameMessage`] — как получатель строит
/// `event.source` из СВОЕГО реестра.
#[cfg(feature = "v8-backend")]
#[derive(Clone, Copy)]
pub(crate) enum SourceKind {
    /// Отправитель — родитель получателя: source = фасад слота parent.
    Parent,
    /// Отправитель — потомок; ключ — указатель `Arc` документа отправителя,
    /// по которому получатель ищет свой слот `frames[j]`.
    ChildDoc(usize),
}

/// Одно сообщение в [`FRAME_OUTBOX`] до разбора получателем.
#[cfg(feature = "v8-backend")]
pub(crate) struct PendingFrameMessage {
    /// Клон `Arc<Mutex<Document>>` адресата. Держит документ живым и указатель
    /// стабильным до доставки — сообщение не попадёт чужому контексту, занявшему
    /// освобождённый адрес.
    pub(crate) target_doc: Arc<Mutex<lumen_dom::Document>>,
    /// Кто отправил ([`SourceKind`]).
    pub(crate) source: SourceKind,
    /// Данные сообщения, уже сериализованные отправителем (`JSON.stringify`).
    pub(crate) data_json: String,
}

/// Глобальный ящик «кто-то вызвал `facade.postMessage(...)`».
///
/// Пишут нативы любого изолята, читает только `_lumen_frame_take_messages`
/// на JS-потоке получателя. Ёмкость ограничена: переполнение молча теряет
/// сообщение (отправка postMessage не имеет отчётности о доставке и по спеке),
/// чтобы зомби-страница не растила память бесконечно.
#[cfg(feature = "v8-backend")]
pub(crate) fn frame_outbox() -> &'static Mutex<Vec<PendingFrameMessage>> {
    static OUTBOX: OnceLock<Mutex<Vec<PendingFrameMessage>>> = OnceLock::new();
    OUTBOX.get_or_init(|| Mutex::new(Vec::new()))
}

/// Верхняя граница неотправленных сообщений во всём процессе.
#[cfg(feature = "v8-backend")]
const FRAME_OUTBOX_CAP: usize = 256;

/// Нормализованный origin URL биндинга для `event.origin`/валидации
/// `targetOrigin`. `about:*` наследует origin контекста-родителя — здесь это
/// выражено тем, что вызывающая сторона подставляет `self_origin` получателя.
#[cfg(feature = "v8-backend")]
fn binding_origin(url: &str, fallback: &str) -> String {
    if url.starts_with("about:") {
        return fallback.to_owned();
    }
    crate::file_input::origin_for_url(url)
}

/// Разрешить `bid` (индекс или псевдо-bid предка) в слот реестра.
#[cfg(feature = "v8-backend")]
fn resolve_slot(slots: &FrameDocSlots, bid: u32) -> Option<&FrameDocBinding> {
    match bid {
        PARENT_BID => slots.parent.as_ref(),
        TOP_BID => slots.top.as_ref(),
        i => slots.frames.get(i as usize),
    }
}

/// Захватить биндинг `bid` на чтение, если он существует и разрешён.
///
/// Все нативы чтения проходят через эту точку: несуществующий bid и
/// cross-origin/opaque bid неотличимы для вызывающего JS — оба дают «пусто».
#[cfg(feature = "v8-backend")]
fn with_accessible_doc<R>(
    registry: &FrameDocRegistry,
    bid: u32,
    f: impl FnOnce(&lumen_dom::Document) -> R,
    empty: R,
) -> R {
    let reg = registry.lock().unwrap_or_else(|e| e.into_inner());
    let Some(binding) = resolve_slot(&reg, bid) else {
        return empty;
    };
    if !binding.accessible {
        return empty;
    }
    let doc = binding.doc.lock().unwrap_or_else(|e| e.into_inner());
    f(&doc)
}

/// Первый элемент с тегом `tag` (ASCII case-insensitive) в document order.
/// Mirrors `v8_runtime::find_element_by_tag`.
#[cfg(feature = "v8-backend")]
fn find_element_by_tag(doc: &lumen_dom::Document, tag: &str) -> Option<lumen_dom::NodeId> {
    find_first_matching(doc, doc.root(), &|node| {
        node.element_name()
            .map(|n| n.local.eq_ignore_ascii_case(tag))
            .unwrap_or(false)
    })
}

/// Предзаказный обход поддерева с предикатом. Mirrors `v8_runtime::find_first_matching`.
#[cfg(feature = "v8-backend")]
fn find_first_matching(
    doc: &lumen_dom::Document,
    start: lumen_dom::NodeId,
    pred: &dyn Fn(&lumen_dom::Node) -> bool,
) -> Option<lumen_dom::NodeId> {
    let node = doc.get(start);
    if pred(node) {
        return Some(start);
    }
    for &child in &node.children.clone() {
        if let Some(found) = find_first_matching(doc, child, pred) {
            return Some(found);
        }
    }
    None
}

/// Конкатенация текстовых узлов поддерева. Mirrors `v8_runtime::collect_text_content`.
#[cfg(feature = "v8-backend")]
fn collect_text_content(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> String {
    if let lumen_dom::NodeData::Comment(s) = &doc.get(id).data {
        return s.clone();
    }
    fn inner(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
        let node = doc.get(id);
        if let lumen_dom::NodeData::Text(s) = &node.data {
            out.push_str(s);
        }
        for &child in &node.children.clone() {
            inner(doc, child, out);
        }
    }
    let mut out = String::new();
    inner(doc, id, &mut out);
    out
}

/// Зарегистрировать нативы `_lumen_f_*` + оценить JS-шим фасадов.
///
/// Вызывается из `install_dom` (список `install_v8!`) с клоном реестра
/// рантайма — тем же, куда пишет [`V8JsRuntime::register_frame_document`].
#[cfg(feature = "v8-backend")]
pub(crate) fn install_frame_bridge_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    registry: FrameDocRegistry,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3};
    use lumen_core::ext::JsRuntime as _;

    // bid → есть ли биндинг вообще (для contentWindow, который существует
    // даже при accessible=false).
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_binding",
            into_v8_fn1(move |host_nid: u32| -> Option<u32> {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                reg.frames
                    .iter()
                    .position(|b| b.host_nid == host_nid)
                    .map(|i| i as u32)
            }),
        )?;
    }
    // Срез 3: bid слотов предков текущего контекста (или null, если контекст
    // сам верхний). Геттеры window.parent/top/frameElement читают их лениво.
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_parent_binding",
            into_v8_fn0(move || -> Option<u32> {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .parent
                    .is_some()
                    .then_some(PARENT_BID)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_top_binding",
            into_v8_fn0(move || -> Option<u32> {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .top
                    .is_some()
                    .then_some(TOP_BID)
            }),
        )?;
    }
    // Срез 3: число дочерних фреймов этого контекста (window.length) и доступ
    // к биндингу по индексу регистрации (постановка индексных/именованных
    // доступников на window после каждого register_frame_document).
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_count",
            into_v8_fn0(move || -> u32 {
                reg.lock().unwrap_or_else(|e| e.into_inner()).frames.len() as u32
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_host_at",
            into_v8_fn1(move |idx: u32| -> Option<u32> {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .frames
                    .get(idx as usize)
                    .map(|b| b.host_nid)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_name_at",
            into_v8_fn1(move |idx: u32| -> Option<String> {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .frames
                    .get(idx as usize)?
                    .name
                    .clone()
                    .filter(|n| !n.is_empty())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_accessible",
            into_v8_fn1(move |bid: u32| -> bool {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                resolve_slot(&reg, bid).is_some_and(|b| b.accessible)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_host",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                resolve_slot(&reg, bid)
                    .filter(|b| b.accessible)
                    .map(|b| b.host_nid)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_url",
            into_v8_fn1(move |bid: u32| -> String {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                match resolve_slot(&reg, bid) {
                    Some(b) if b.accessible => b.url.clone(),
                    _ => String::new(),
                }
            }),
        )?;
    }

    // ── Срез 4: кросс-фреймовый postMessage ──────────────────────────────────
    // Валидация targetOrigin + постановка в глобальный ящик. Выполняется на
    // JS-потоке ОТПРАВИТЕЛЯ; получатель разбирает ящик у себя на тике
    // (_lumen_frame_pump_messages), поэтому никаких перекрёстных вызовов
    // между изолятами нет.
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_post_message",
            into_v8_fn3(move |bid: u32, data_json: String, target_origin: String| -> bool {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                let Some(binding) = resolve_slot(&reg, bid) else {
                    return false;
                };
                // HTML LS §9.2.9 шаг 3: '*' доставляет всегда, '/' — только
                // same-origin (у нас это уже вычисленный shell'ом accessible),
                // явная строка — совпадение с origin адресата.
                let matches = match target_origin.as_str() {
                    "*" => true,
                    "/" | "" => binding.accessible,
                    o => o.eq_ignore_ascii_case(&binding_origin(
                        &binding.url,
                        &reg.self_origin,
                    )),
                };
                if !matches {
                    return false;
                }
                // Кто отправитель для получателя: постинг в фасад потомка —
                // сам отправитель его родитель; постинг в фасад предка
                // (PARENT_BID/TOP_BID) — отправитель потомок со своим ключом.
                // bid фасада отправителя разрешит получатель у себя (take):
                // в момент постановки реестр получателя недоступен.
                let source = match bid {
                    PARENT_BID | TOP_BID => SourceKind::ChildDoc(reg.self_key.unwrap_or(0)),
                    _ => SourceKind::Parent,
                };
                let target_doc = Arc::clone(&binding.doc);
                let outbox = frame_outbox();
                let mut outbox = outbox.lock().unwrap_or_else(|e| e.into_inner());
                if outbox.len() >= FRAME_OUTBOX_CAP {
                    return false;
                }
                outbox.push(PendingFrameMessage {
                    target_doc,
                    source,
                    data_json,
                });
                true
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_take_messages",
            into_v8_fn0(move || -> String {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                let key = match reg.self_key {
                    Some(k) => k,
                    None => return String::new(),
                };
                let outbox = frame_outbox();
                let mut outbox = outbox.lock().unwrap_or_else(|e| e.into_inner());
                let mut taken = Vec::new();
                let mut rest = Vec::new();
                for msg in outbox.drain(..) {
                    if Arc::as_ptr(&msg.target_doc) as usize == key {
                        taken.push(msg);
                    } else {
                        rest.push(msg);
                    }
                }
                *outbox = rest;
                if taken.is_empty() {
                    return String::new();
                }
                // event.origin и bid фасада отправителя считаются ЗДЕСЬ:
                // только реестр получателя знает свои слоты (source) и чем
                // наследуется origin about:-детей (srcdoc/about:blank →
                // origin получателя-родителя).
                let items: Vec<serde_json::Value> = taken
                    .into_iter()
                    .map(|m| {
                        let (source_bid, origin) = match m.source {
                            SourceKind::Parent => (
                                reg.parent.as_ref().map(|_| PARENT_BID),
                                reg.parent
                                    .as_ref()
                                    .map(|b| binding_origin(&b.url, &reg.self_origin))
                                    .unwrap_or_default(),
                            ),
                            SourceKind::ChildDoc(doc_key) => {
                                match reg
                                    .frames
                                    .iter()
                                    .position(|b| Arc::as_ptr(&b.doc) as usize == doc_key)
                                {
                                    Some(j) => {
                                        let b = &reg.frames[j];
                                        (
                                            Some(j as u32),
                                            binding_origin(&b.url, &reg.self_origin),
                                        )
                                    }
                                    // Отправителя нет в прямых слотах получателя
                                    // (внук → top): source = null, origin пустой.
                                    None => (None, String::new()),
                                }
                            }
                        };
                        serde_json::json!({
                            "bid": source_bid,
                            "origin": origin,
                            "data": serde_json::from_str::<serde_json::Value>(&m.data_json)
                                .unwrap_or(serde_json::Value::Null),
                        })
                    })
                    .collect();
                serde_json::to_string(&items).unwrap_or_else(|_| String::new())
            }),
        )?;
    }

    // ── Document-level чтение ────────────────────────────────────────────────
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_body",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    find_element_by_tag(d, "body").map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_head",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    find_element_by_tag(d, "head").map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_document_element",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| d.document_element().map(|n| n.index() as u32), None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_title",
            into_v8_fn1(move |bid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    find_element_by_tag(d, "title")
                        .map(|nid| collect_text_content(d, nid))
                        .unwrap_or_default()
                }, String::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_by_id",
            into_v8_fn2(move |bid: u32, id: String| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    find_first_matching(d, d.root(), &|node| {
                        matches!(&node.data, lumen_dom::NodeData::Element { .. })
                            && node.get_attr("id") == Some(id.as_str())
                    })
                    .map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query",
            into_v8_fn2(move |bid: u32, sel: String| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all(d, &sel).into_iter().next().map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query_all",
            into_v8_fn2(move |bid: u32, sel: String| -> Vec<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all(d, &sel)
                        .into_iter()
                        .map(|n| n.index() as u32)
                        .collect()
                }, Vec::new())
            }),
        )?;
    }

    // ── Element-level чтение ─────────────────────────────────────────────────
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query_scoped",
            into_v8_fn3(move |bid: u32, nid: u32, sel: String| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all_scoped(d, lumen_dom::NodeId::from_index(nid as usize), &sel)
                        .into_iter()
                        .next()
                        .map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query_all_scoped",
            into_v8_fn3(move |bid: u32, nid: u32, sel: String| -> Vec<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all_scoped(d, lumen_dom::NodeId::from_index(nid as usize), &sel)
                        .into_iter()
                        .map(|n| n.index() as u32)
                        .collect()
                }, Vec::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_children",
            into_v8_fn2(move |bid: u32, nid: u32| -> Vec<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    d.get(lumen_dom::NodeId::from_index(nid as usize))
                        .children
                        .iter()
                        .filter(|&&c| matches!(&d.get(c).data, lumen_dom::NodeData::Element { .. }))
                        .map(|&c| c.index() as u32)
                        .collect()
                }, Vec::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_parent_element",
            into_v8_fn2(move |bid: u32, nid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    let id = lumen_dom::NodeId::from_index(nid as usize);
                    d.get(id).parent.and_then(|pid| {
                        matches!(&d.get(pid).data, lumen_dom::NodeData::Element { .. })
                            .then(|| pid.index() as u32)
                    })
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_attr",
            into_v8_fn3(move |bid: u32, nid: u32, name: String| -> Option<String> {
                with_accessible_doc(&reg, bid, |d| {
                    d.get(lumen_dom::NodeId::from_index(nid as usize))
                        .get_attr(&name)
                        .map(str::to_owned)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_has_attr",
            into_v8_fn3(move |bid: u32, nid: u32, name: String| -> bool {
                with_accessible_doc(&reg, bid, |d| {
                    d.get(lumen_dom::NodeId::from_index(nid as usize))
                        .get_attr(&name)
                        .is_some()
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_tag",
            into_v8_fn2(move |bid: u32, nid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    match &d.get(lumen_dom::NodeId::from_index(nid as usize)).data {
                        lumen_dom::NodeData::Element { name, .. } => name.local.clone(),
                        _ => String::new(),
                    }
                }, String::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_is_text",
            into_v8_fn2(move |bid: u32, nid: u32| -> bool {
                with_accessible_doc(&reg, bid, |d| {
                    matches!(
                        &d.get(lumen_dom::NodeId::from_index(nid as usize)).data,
                        lumen_dom::NodeData::Text(_)
                    )
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_is_comment",
            into_v8_fn2(move |bid: u32, nid: u32| -> bool {
                with_accessible_doc(&reg, bid, |d| {
                    matches!(
                        &d.get(lumen_dom::NodeId::from_index(nid as usize)).data,
                        lumen_dom::NodeData::Comment(_)
                    )
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_text",
            into_v8_fn2(move |bid: u32, nid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    collect_text_content(d, lumen_dom::NodeId::from_index(nid as usize))
                }, String::new())
            }),
        )?;
    }

    rt.eval(FRAME_BRIDGE_SHIM)?;
    Ok(())
}

/// JavaScript shim: фасады Window/Document/Element над нативами `_lumen_f_*`.
///
/// Точка входа для геттеров `iframe_element.rs` — две глобальные функции,
/// принимающие `__nid__` хоста; всё остальное спрятано в замыкании модуля.
///
/// Срез 3 добавляет: (1) переопределение `window.parent/top/frameElement/
/// length` геттерами, которые читают слоты предков реестра и пока тех нет
/// ведут себя как прежние константы из WEB_API_SHIM; (2) глобальный
/// `_lumen_frame_install_index(idx)`, который `register_frame_document`
/// вызывает после каждой регистрации — ставит на `window` индексный и
/// именованный доступники к окну фрейма. Исполняется строго после
/// WEB_API_SHIM (порядок в `install_dom`), поэтому `window` уже существует
/// в проде; в минимальных тестовых изолятах блок пропускается через typeof.
#[cfg(feature = "v8-backend")]
const FRAME_BRIDGE_SHIM: &str = r#"(function() {
  'use strict';

  // Псевдо-bid слотов предков (зеркало frame_bridge::PARENT_BID/TOP_BID).
  var PARENT_BID = 0xFFFFFFFF;
  var TOP_BID = 0xFFFFFFFE;

  // Интерны фасадов, ключ — bid (стабильный индекс биндинга). Живут столько
  // же, сколько контекст страницы: identity фасадов обязана быть постоянной.
  var wins = {};
  var docs = {};
  var elems = {};

  function bidOrNull(hostNid) {
    if (hostNid === null || hostNid === undefined) return null;
    var bid = _lumen_frame_binding(hostNid);
    return (bid === null || bid === undefined) ? null : bid;
  }

  function isAncestorBid(bid) { return bid === PARENT_BID || bid === TOP_BID; }

  // Разрешение top-окна текущего КОНТЕКСТА (не фасада): отдельный слот top,
  // иначе слот parent (фрейм 1-го уровня), иначе сам window.
  function topOfContext() {
    if (typeof window === 'undefined') return null;
    var t = _lumen_top_binding();
    if (t !== null && t !== undefined) return winFacade(t);
    var p = _lumen_parent_binding();
    if (p !== null && p !== undefined) return winFacade(p);
    return window;
  }

  function frameElem(bid, nid) {
    if (nid === null || nid === undefined || nid < 0) return null;
    var cache = elems[bid];
    if (!cache) { cache = {}; elems[bid] = cache; }
    var cached = cache[nid];
    if (cached) return cached;
    var el = {
      __nid__: nid,
      get nodeType() {
        if (_lumen_f_is_text(bid, nid)) return 3;
        if (_lumen_f_is_comment(bid, nid)) return 8;
        return 1;
      },
      get localName()  { return _lumen_f_tag(bid, nid); },
      get tagName()    { var t = _lumen_f_tag(bid, nid); return t ? t.toUpperCase() : t; },
      get nodeName()   { var t = _lumen_f_tag(bid, nid); return t ? t.toUpperCase() : t; },
      get id()         { var v = _lumen_f_attr(bid, nid, 'id'); return v !== null && v !== undefined ? v : ''; },
      get className()  { var v = _lumen_f_attr(bid, nid, 'class'); return v !== null && v !== undefined ? v : ''; },
      get textContent(){ return _lumen_f_text(bid, nid); },
      getAttribute: function(n) { return _lumen_f_attr(bid, nid, String(n)); },
      hasAttribute: function(n) { return _lumen_f_has_attr(bid, nid, String(n)); },
      get children() { return _lumen_f_children(bid, nid).map(function(c) { return frameElem(bid, c); }); },
      get childElementCount() { return _lumen_f_children(bid, nid).length; },
      get firstElementChild() { return frameElem(bid, _lumen_f_children(bid, nid)[0]); },
      get lastElementChild() {
        var ch = _lumen_f_children(bid, nid);
        return frameElem(bid, ch[ch.length - 1]);
      },
      get parentElement() { return frameElem(bid, _lumen_f_parent_element(bid, nid)); },
      querySelector: function(sel) { return frameElem(bid, _lumen_f_query_scoped(bid, nid, String(sel))); },
      querySelectorAll: function(sel) {
        return _lumen_f_query_all_scoped(bid, nid, String(sel)).map(function(c) { return frameElem(bid, c); });
      },
      // BUG-480 срез 2: содержимое фрейма не layout'ится — честные нули вместо
      // выдуманных размеров (layout фреймов — будущий срез).
      get offsetWidth()  { return 0; },
      get offsetHeight() { return 0; },
      get clientWidth()  { return 0; },
      get clientHeight() { return 0; },
      get scrollWidth()  { return 0; },
      get scrollHeight() { return 0; },
      getBoundingClientRect: function() {
        return { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0 };
      },
    };
    cache[nid] = el;
    return el;
  }

  function docFacade(bid) {
    var cached = docs[bid];
    if (cached) return cached;
    var d = {};
    function el(nid) { return frameElem(bid, nid); }
    Object.defineProperty(d, 'body',              { get: function() { return el(_lumen_f_body(bid)); }, configurable: true });
    Object.defineProperty(d, 'head',              { get: function() { return el(_lumen_f_head(bid)); }, configurable: true });
    Object.defineProperty(d, 'documentElement',   { get: function() { return el(_lumen_f_document_element(bid)); }, configurable: true });
    Object.defineProperty(d, 'title',             { get: function() { return _lumen_f_title(bid); }, configurable: true });
    Object.defineProperty(d, 'URL',               { get: function() { return _lumen_f_url(bid); }, configurable: true });
    Object.defineProperty(d, 'documentURI',       { get: function() { return _lumen_f_url(bid); }, configurable: true });
    // Ребёнок получил window load ещё в срезе 1 — readyState к моменту доступа
    // всегда «complete»; отдельного трекинга переходов срез 2 не ведёт.
    Object.defineProperty(d, 'readyState',        { get: function() { return 'complete'; }, configurable: true });
    Object.defineProperty(d, 'defaultView',       { get: function() { return winFacade(bid); }, configurable: true });
    d.getElementById = function(id) { return el(_lumen_f_by_id(bid, String(id))); };
    d.querySelector = function(sel) { return el(_lumen_f_query(bid, String(sel))); };
    d.querySelectorAll = function(sel) {
      return _lumen_f_query_all(bid, String(sel)).map(function(n) { return frameElem(bid, n); });
    };
    docs[bid] = d;
    return d;
  }

  function winFacade(bid) {
    var cached = wins[bid];
    if (cached) return cached;
    var w = {};
    var hostNid = _lumen_f_accessible(bid) ? _lumen_f_host(bid) : null;
    Object.defineProperty(w, 'document', {
      get: function() { return _lumen_f_accessible(bid) ? docFacade(bid) : null; },
      configurable: true,
    });
    w.window = w;
    w.self = w;
    w.frames = w;
    // parent/top зависят от того, ЧЕЙ фасад читают и откуда. Фасад предка
    // (PARENT_BID/TOP_BID в изоляте ребёнка) сам себе parent/top: контекст,
    // в котором он построен, — его потомок. Фасад дочернего фрейма, читаемый
    // из родителя, отсылает к настоящему окну читающего контекста (срез 2).
    Object.defineProperty(w, 'parent', {
      get: function() {
        if (isAncestorBid(bid)) return w;
        return typeof window !== 'undefined' ? window : null;
      },
      configurable: true,
    });
    Object.defineProperty(w, 'top', {
      get: function() {
        if (isAncestorBid(bid)) return w;
        return topOfContext();
      },
      configurable: true,
    });
    Object.defineProperty(w, 'closed', { get: function() { return false; }, configurable: true });
    // Счётчик фреймов чужого изолята недоступен — у фасадов честный ноль;
    // живой length есть только у настоящего window (ниже).
    w.length = 0;
    Object.defineProperty(w, 'frameElement', {
      get: function() {
        if (!_lumen_f_accessible(bid)) return null;
        // Хост фрейма лежит в документе родителя; для фасада ПРЕДКА
        // (читается из изолята ребёнка) элемент строится нативами бриджа
        // над документом родителя, для обычного фасада — обычным враппером
        // текущего документа (хост и читающий код живут в одном дереве).
        if (isAncestorBid(bid)) return frameElem(bid, _lumen_f_host(bid));
        return (hostNid !== null && typeof _lumen_make_element === 'function')
          ? _lumen_make_element(hostNid)
          : null;
      },
      configurable: true,
    });
    Object.defineProperty(w, 'name', {
      get: function() {
        if (hostNid === null) return '';
        // Аналогично frameElement: атрибут хоста предка читаем через бридж.
        var a = isAncestorBid(bid)
          ? _lumen_f_attr(bid, hostNid, 'name')
          : _lumen_get_attr(hostNid, 'name');
        return (a === null || a === undefined) ? '' : a;
      },
      configurable: true,
    });
    Object.defineProperty(w, 'location', {
      get: function() {
        var href = _lumen_f_url(bid);
        return { href: href, toString: function() { return href; } };
      },
      configurable: true,
    });
    w.close = function() {};
    // Срез 4: postMessage на WindowProxy. Данные уходят JSON-круготрипом;
    // функции/символы клонировать нельзя — DataCloneError (TypeError там,
    // где DOMException ещё не установлен). Опущенный targetOrigin = '/'
    // (спечный дефолт), т.е. доставка только same-origin.
    w.postMessage = function(message, targetOrigin) {
      if (typeof message === 'function' || typeof message === 'symbol') {
        throw (typeof DOMException === 'function')
          ? new DOMException('object could not be cloned', 'DataCloneError')
          : new TypeError('object could not be cloned');
      }
      var json = (message === undefined) ? 'null' : JSON.stringify(message);
      if (typeof json !== 'string') {
        throw (typeof DOMException === 'function')
          ? new DOMException('object could not be cloned', 'DataCloneError')
          : new TypeError('object could not be cloned');
      }
      var to = (targetOrigin === undefined || targetOrigin === null) ? '/' : String(targetOrigin);
      _lumen_f_post_message(bid, json, to);
    };
    wins[bid] = w;
    return w;
  }

  globalThis._lumen_frame_content_document = function(hostNid) {
    var bid = bidOrNull(hostNid);
    if (bid === null || !_lumen_f_accessible(bid)) return null;
    return docFacade(bid);
  };
  globalThis._lumen_frame_content_window = function(hostNid) {
    var bid = bidOrNull(hostNid);
    if (bid === null) return null;
    return winFacade(bid);
  };

  // ── Срез 3: иерархия окон настоящего window этого контекста ──────────────
  // Установщики вызываются ЛЕНИВО из register_parent_document/
  // register_frame_document: пока фреймов нет, глобальные свойства контекста
  // остаются ровно теми, что поставил WEB_API_SHIM (`window.parent = window`,
  // `length = 0`). Это критично: инсталлтайм-акцессор на parent/top/length
  // ломает топ-левел `var parent = …` страниц и тестов (V8 не подменяет
  // существующий акцессор var-объявлением). Первый же вызов регистрации
  // меняет свойства; попытка поверх НЕконфигурируемого var-биндинга
  // пользователя молча пропускается try/catch — деградация до прежнего
  // значения.

  var __hierarchyInstalled = false;
  var __lengthInstalled = false;

  function installLengthAccessor() {
    if (__lengthInstalled) return;
    __lengthInstalled = true;
    try {
      Object.defineProperty(window, 'length', {
        get: function() { return _lumen_frame_count(); },
        configurable: true,
      });
    } catch (e) {}
  }

  function installHierarchyAccessors() {
    if (__hierarchyInstalled) return;
    __hierarchyInstalled = true;
    var __prevName = window.name;
    try {
      Object.defineProperty(window, 'parent', {
        get: function() {
          var p = _lumen_parent_binding();
          return (p !== null && p !== undefined) ? winFacade(p) : window;
        },
        configurable: true,
      });
    } catch (e) {}
    try {
      Object.defineProperty(window, 'top', {
        get: function() { return topOfContext(); },
        configurable: true,
      });
    } catch (e) {}
    try {
      Object.defineProperty(window, 'frameElement', {
        get: function() {
          var p = _lumen_parent_binding();
          return (p !== null && p !== undefined && _lumen_f_accessible(p))
            ? winFacade(p).frameElement
            : null;
        },
        configurable: true,
      });
    } catch (e) {}
    // window.name фрейма — атрибут name хоста (HTML LS §7.2.3); явное
    // присваивание перекрывает атрибут до замены документа.
    var __customName = null;
    try {
      Object.defineProperty(window, 'name', {
        get: function() {
          if (__customName !== null) return __customName;
          var p = _lumen_parent_binding();
          if (p === null || p === undefined || !_lumen_f_accessible(p)) {
            return __prevName === undefined ? '' : __prevName;
          }
          var host = _lumen_f_host(p);
          if (host === null || host === undefined) return '';
          var a = _lumen_f_attr(p, host, 'name');
          return (a === null || a === undefined) ? '' : a;
        },
        set: function(v) { __customName = String(v == null ? '' : v); },
        configurable: true,
      });
    } catch (e) {}
  }

  // Родитель зарегистрирован: включить parent/top/frameElement/name.
  globalThis._lumen_frame_install_hierarchy = function() {
    if (typeof window === 'undefined') return;
    installHierarchyAccessors();
  };

  // Фрейм зарегистрирован: индексный (`window[idx]`) + именованный
  // (`window[имя]`) доступники окна фрейма; живой length. Вызывается из
  // V8JsRuntime::register_frame_document с индексом биндинга; порядок
  // регистрации = порядок документа (спечный tree order). Именованный
  // доступ покрывает ТОЛЬКО iframe (embed/form/img/object — не бриджевая
  // территория).
  globalThis._lumen_frame_install_index = function(idx) {
    if (typeof window === 'undefined') return;
    installLengthAccessor();
    try {
      var host = _lumen_frame_host_at(idx);
      if (host === null || host === undefined) return;
      var mk = function(h) {
        return function() { return _lumen_frame_content_window(h); };
      };
      Object.defineProperty(window, String(idx), { get: mk(host), configurable: true });
      var nm = _lumen_frame_name_at(idx);
      if (nm) {
        Object.defineProperty(window, nm, { get: mk(host), configurable: true });
      }
    } catch (e) {}
  };

  // ── Срез 4: разбор ящика кросс-фреймовых postMessage ──────────────────────
  // Shell вызывает на каждом тике (pump_frame_messages) и у страницы, и у
  // каждого фрейма. Натив отдаёт JSON-массив сообщений, адресованных ЭТОМУ
  // контексту; каждое разворачивается в MessageEvent и доставляется через
  // хук из WEB_API_SHIM (window.onmessage + addEventListener('message')).
  globalThis._lumen_frame_pump_messages = function() {
    if (typeof window === 'undefined') return;
    if (typeof _lumen_deliver_frame_message !== 'function') return;
    var raw = _lumen_frame_take_messages();
    if (!raw) return;
    var msgs;
    try { msgs = JSON.parse(raw); } catch (e) { return; }
    if (!msgs || !msgs.length) return;
    for (var i = 0; i < msgs.length; i++) {
      var m = msgs[i];
      var source = null;
      if (m.bid !== null && m.bid !== undefined) {
        source = winFacade(m.bid);
      }
      _lumen_deliver_frame_message(m.data, m.origin, source);
    }
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
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
            });
            if let Some(top) = top_html {
                reg.top = Some(FrameDocBinding {
                    host_nid: 0,
                    doc: Arc::new(Mutex::new(lumen_html_parser::parse(top))),
                    url: "https://top.example/".to_owned(),
                    name: None,
                    accessible,
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
                     && window.name === ''"
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
    /// получает мини-хук приёма, складывающий доставки в `__msgs`.
    fn with_parent_child_pair(
        parent_accessible_to_child: bool,
        child_accessible_to_parent: bool,
    ) -> (V8JsRuntime, V8JsRuntime) {
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
            });
        }
        rt_child
            .eval("typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()")
            .unwrap();

        for rt in [&rt_parent, &rt_child] {
            rt.eval(
                "globalThis.__msgs = []; \
                 globalThis._lumen_deliver_frame_message = function(d, o, s) { \
                     __msgs.push({ d: d, o: o, s: s }); \
                 };",
            )
            .unwrap();
        }
        (rt_parent, rt_child)
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
}
