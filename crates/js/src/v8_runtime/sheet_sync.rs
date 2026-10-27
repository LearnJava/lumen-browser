//! BUG-493 (живое окно): реестр листов `document.styleSheets`/`element.sheet`
//! сверяется с DOM на стороне JS, а CSSOM-правки доходят до каскада шелла.
//!
//! До этого реестр [`super::V8JsRuntime::stylesheet_nodes`] заполнял только
//! шелл — один раз, после каскада загрузки (`build_stylesheet_node_registry`).
//! Всё, что скрипт вставлял позже, оставалось без листа: `<style>` после
//! `appendChild` отвечал `.sheet === null`, и styled-components падал с
//! ошибкой #17 («CSSStyleSheet could not be found on HTMLStyleElement») —
//! twitch, quora, bbc, imdb. Здесь реестр досчитывается из самого DOM при
//! чтении, как это делает браузер: у подключённого `<style>` лист есть сразу.
//!
//! Вторая половина — [`patched_cascade`]: правки `insertRule`/`deleteRule`/
//! `.style` (журнал `CssomDeltaLog`, CSSOM-8 вариант C) накладываются не только
//! на лист синхронного флаша, но и на каскад, по которому шелл рисует.
//! Пустой `<style>`, в который библиотека пишет только через `insertRule`
//! (speedy-режим styled-components/emotion), в тексте каскада не встречается
//! вовсе — его правила вливаются листом целиком.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use lumen_css_parser::{CssomOp, Stylesheet, StylesheetNodeEntry, StylesheetRevision};
use lumen_dom::{Document, NodeData, NodeId};

use super::named_access::lock_document_bounded;
use super::dom_touched::DomTouched;
use super::style_flush::CssomDeltaLog;

/// Листы шелла до наложения CSSOM-правок и ревизии результатов наложения (последние
/// [`PRISTINE_KEPT`], старые первыми) — см. [`SheetSync::pristine`].
pub(crate) type PristineCascade = Arc<Mutex<Vec<(StylesheetRevision, Arc<Stylesheet>)>>>;

/// Сколько результатов наложения помнит [`SheetSync::pristine`]. Шелл и движок накладывают правки
/// независимо, и флаш может получить лист не последнего наложения, а одного из предыдущих
/// (BUG-1554: выключенный скриптом `<link>` оставался в каскаде флаша).
const PRISTINE_KEPT: usize = 8;

/// Ручки, которые нужны сверке реестра и записи CSSOM-правки. Все поля —
/// `Arc`, клон дешёвый; живёт в нативах `install_stylesheets`, во
/// `FlushHandles` и в самом рантайме (для вызовов шелла).
#[derive(Clone)]
pub(crate) struct SheetSync {
    pub(crate) doc: Arc<Mutex<Document>>,
    pub(crate) nodes: Arc<Mutex<Vec<StylesheetNodeEntry>>>,
    pub(crate) deltas: CssomDeltaLog,
    /// CSSOM-8 вариант C: «журнал вырос с последнего флаша».
    pub(crate) dirty: Arc<AtomicBool>,
    /// Поколение журнала правок: растёт при каждой записи и при каждом
    /// выбрасывании записей. Шелл сравнивает его со своим, чтобы понять, что
    /// каскад пора пересобрать.
    pub(crate) epoch: Arc<AtomicU64>,
    /// Флаг «страница изменилась» планировщика шелла (`V8JsRuntime::dom_dirty`).
    /// CSSOM-правка DOM не трогает, но экран после неё перерисовать надо —
    /// без флага `insertRule` из таймера ждал бы чужой мутации.
    pub(crate) relayout: Arc<AtomicBool>,
    /// Источник `DomTouched::epoch` — счётчика мутаций DOM.
    pub(crate) touched: Arc<Mutex<DomTouched>>,
    /// `DomTouched::epoch`, при котором реестр в последний раз сверялся с DOM.
    /// `None` — сверки не было или реестр заменил шелл.
    pub(crate) synced: Arc<Mutex<Option<u64>>>,
    /// Узлы реестра, лежащие в теневом дереве: их листы относятся к своему
    /// `ShadowRoot`, а не к документу — в `document.styleSheets` и в каскад
    /// документа они не попадают.
    pub(crate) shadow_owned: Arc<Mutex<HashSet<u32>>>,
    /// Последние результаты [`super::V8JsRuntime::patch_cascade`]: ревизия
    /// и лист шелла, на который накладывались правки. Шелл отдаёт результат
    /// обратно как лист синхронного флаша, и флаш по ревизии берёт исходный
    /// лист, чтобы не наложить те же правки второй раз. Помнится несколько
    /// наложений: шелл может наложить заново, пока флаш держит лист предыдущего.
    pub(crate) pristine: PristineCascade,
    /// `<style>` узла → `DomTouched::child_list_gen` на момент, когда его лист
    /// последний раз сверялся с текстом. Смена поколения при том же тексте
    /// (`style.innerHTML = <тот же текст>`) значит, что браузер создал бы
    /// новый лист: CSSOM-правки старого выбрасываются, лист разбирается заново.
    pub(crate) child_list_gen: Arc<Mutex<HashMap<u32, u64>>>,
    /// BUG-1439: листы узлов, вышедших из дерева, — старые первыми. `CSSStyleSheet`
    /// самостоятельный объект (CSSOM §6.2): после `removeChild(style)` скрипт,
    /// державший `style.sheet`, по-прежнему читает и правит его правила. Реестр
    /// узлов лист теряет, поэтому сверка откладывает его сюда, а
    /// `_lumen_stylesheet_take_detached` переселяет в реестр конструируемых.
    pub(crate) detached: DetachedSheets,
    /// BUG-1554: узлы, чей лист включили/выключили скриптом (`sheet.disabled`,
    /// `link.disabled`) или чей `<link>` догрузился уже после каскада шелла. Только
    /// им [`patched_cascade`] выносит вердикт «применяется ли лист»: остальные
    /// собрал сам шелл из того же DOM.
    pub(crate) state_touched: Arc<Mutex<HashSet<u32>>>,
    /// BUG-1554: листы из `state_touched`, которые реестр потерял (скрипт выключил лист или
    /// `<link disabled>`). Шелл мог уже вложить их текст в каскад, пересобрав его из DOM, пока
    /// лист был включён, — [`patched_cascade`] вырезает этот текст обратно.
    pub(crate) removed: DetachedSheets,
}

/// Листы узлов, вышедших из дерева, — см. [`SheetSync::detached`].
pub(crate) type DetachedSheets = Arc<Mutex<Vec<(u32, Arc<Stylesheet>)>>>;

/// Сколько листов вышедших из дерева узлов держит [`SheetSync::detached`]:
/// CSS-in-JS снимает и создаёт `<style>` непрерывно, а забрать лист успевает
/// только тот, у кого есть живая обёртка.
const DETACHED_SHEETS_CAP: usize = 256;

/// BUG-493: то, что нужно шеллу для каскада отрисовки, без обращения к
/// самому рантайму. Под движковым потоком (ADR-023, по умолчанию) рантайм
/// живёт на другом потоке, и `refresh_dynamic_css` на UI-потоке до него не
/// дотягивается — `js_ctx` там пуст. Все поля — `Arc` на те же данные, что у
/// рантайма, поэтому ручка работает с любого потока без очереди движка.
#[derive(Clone)]
pub struct CascadeSource {
    /// Слот рантайма: `None` до `install_dom`, дальше — ручки сверки.
    pub(crate) sync: Arc<Mutex<Option<SheetSync>>>,
    pub(crate) epoch: Arc<AtomicU64>,
    pub(crate) constructed: super::install::ConstructedStylesheets,
    pub(crate) adopted: super::install::AdoptedStylesheets,
}

impl CascadeSource {
    /// См. [`super::V8JsRuntime::cssom_epoch`].
    pub fn cssom_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }

    /// См. [`super::V8JsRuntime::patch_cascade`].
    pub fn patch_cascade(&self, sheet: &Stylesheet) -> Option<Stylesheet> {
        let sync = self.sync.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
        sync.sync();
        let patched = {
            let nodes = sync.nodes.lock().unwrap_or_else(|e| e.into_inner());
            let deltas = sync.deltas.lock().unwrap_or_else(|e| e.into_inner());
            let touched = sync.state_touched.lock().unwrap_or_else(|e| e.into_inner());
            if deltas.is_empty() && touched.is_empty() {
                return None;
            }
            let shadow = sync.shadow_owned.lock().unwrap_or_else(|e| e.into_inner());
            let removed = sync.removed.lock().unwrap_or_else(|e| e.into_inner());
            patched_cascade(sheet, &nodes, &deltas, &shadow, &touched, &removed, false)?
        };
        {
            let mut kept = sync.pristine.lock().unwrap_or_else(|e| e.into_inner());
            kept.push((patched.revision(), Arc::new(sheet.clone())));
            let excess = kept.len().saturating_sub(PRISTINE_KEPT);
            kept.drain(..excess);
        }
        Some(patched)
    }

    /// См. [`super::V8JsRuntime::document_adopted_fingerprint`].
    pub fn document_adopted_fingerprint(&self) -> u64 {
        super::install::document_adopted_fingerprint(&self.constructed, &self.adopted)
    }

    /// См. [`super::V8JsRuntime::document_adopted_stylesheet`].
    pub fn document_adopted_stylesheet(&self) -> Option<Stylesheet> {
        super::install::document_adopted_stylesheet(&self.constructed, &self.adopted)
    }
}

/// Один владелец листа, найденный обходом DOM.
struct Owner {
    node: u32,
    /// Текст `<style>`; `None` — `<link rel=stylesheet>`.
    style_text: Option<String>,
    in_shadow: bool,
    /// BUG-1554: участвует ли лист в каскаде (`alternate`/`title`/`disabled` скрипта).
    enabled: bool,
}

impl SheetSync {
    /// Записать CSSOM-правку листа узла `node` в журнал.
    pub(crate) fn record(&self, node: u32, op: CssomOp) {
        self.deltas
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((node, op));
        self.dirty.store(true, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.relayout.store(true, Ordering::Relaxed);
    }

    /// BUG-1554: состояние владельца листа `nid` изменилось (вердикт скрипта, догрузка `<link>`):
    /// реестр пересверяется, а каскад шелла и флаш — перестраиваются.
    pub(crate) fn note_state_change(&self, nid: u32) {
        self.state_touched.lock().unwrap_or_else(|e| e.into_inner()).insert(nid);
        *self.synced.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.dirty.store(true, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.relayout.store(true, Ordering::Relaxed);
    }

    /// BUG-1554: тело `<link rel=stylesheet>` пришло уже после каскада шелла (его вставил скрипт
    /// или снял `disabled`): лист входит в реестр на место `nid` в порядке дерева. `false` —
    /// элемента нет в документе либо он не `<link>`-лист.
    pub(crate) fn register_loaded_link(&self, nid: u32, text: &str) -> bool {
        let media = {
            let Some(doc) = lock_document_bounded(&self.doc) else { return false };
            let id = NodeId::from_raw(nid);
            if !doc.contains_id(id) || !doc.is_connected(id) {
                return false;
            }
            let node = doc.get(id);
            if !node.element_name().is_some_and(|n| n.local == "link") || doc.link_has_no_sheet(id) {
                return false;
            }
            node.get_attr("media").map(str::trim).filter(|m| !m.is_empty()).map(str::to_owned)
        };
        // Лист, который каскад шелла уже нёс (`<link>` до его прохода), второй раз не вливается.
        if self.nodes.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|e| e.node == nid) {
            return true;
        }
        // `media` на `<link>` отсекает лист целиком; каскад шелла такие не вливал вовсе.
        let source = match media {
            Some(m) => format!("@media {m} {{
{text}
}}
"),
            None => text.to_owned(),
        };
        self.nodes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(StylesheetNodeEntry { node: nid, sheet: Arc::new(lumen_css_parser::parse(&source)), disabled: false });
        self.note_state_change(nid);
        self.sync();
        true
    }

    /// Сверить реестр с DOM, если DOM менялся с прошлой сверки.
    ///
    /// Порядок и состав берутся из дерева: каждый подключённый `<style>`
    /// (включая теневые деревья) получает лист; лист существующего `<style>`
    /// переиспользуется, пока его текст совпадает с разобранным, иначе
    /// разбирается заново, а CSSOM-правки старого листа выбрасываются —
    /// браузер на смене текста тоже создаёт новый лист. `<link>` в реестр
    /// отсюда не добавляется: тело внешнего листа приходит только из сети, и
    /// до загрузки `link.sheet` честно `null`; уже известный `<link>`
    /// сохраняется, пока остаётся в дереве. Отключённый от дерева узел
    /// теряет лист (`.sheet === null`, как в CSSOM §6.1).
    ///
    /// Документ берётся с ограниченным ожиданием: если он занят дольше
    /// бюджета, сверка пропускается и реестр остаётся прежним.
    pub(crate) fn sync(&self) {
        let dom_epoch = self.touched.lock().unwrap_or_else(|e| e.into_inner()).epoch;
        if *self.synced.lock().unwrap_or_else(|e| e.into_inner()) == Some(dom_epoch) {
            return;
        }
        let Some(doc) = lock_document_bounded(&self.doc) else {
            return;
        };
        let mut owners = Vec::new();
        let preferred = doc.preferred_sheet_set();
        collect_owners(&doc, doc.root(), false, preferred.as_deref(), &mut owners);
        drop(doc);

        let mut dropped: Vec<u32> = Vec::new();
        let mut shadow = HashSet::new();
        let gens: HashMap<u32, u64> = {
            let touched = self.touched.lock().unwrap_or_else(|e| e.into_inner());
            owners
                .iter()
                .map(|o| (o.node, touched.child_list_gen.get(&NodeId::from_raw(o.node)).copied().unwrap_or(0)))
                .collect()
        };
        {
            let mut seen = self.child_list_gen.lock().unwrap_or_else(|e| e.into_inner());
            let mut nodes = self.nodes.lock().unwrap_or_else(|e| e.into_inner());
            let mut old: HashMap<u32, StylesheetNodeEntry> =
                nodes.drain(..).map(|e| (e.node, e)).collect();
            let mut out = Vec::with_capacity(owners.len());
            for owner in owners {
                let enabled = owner.enabled;
                let prev = old.remove(&owner.node);
                let generation = gens.get(&owner.node).copied().unwrap_or(0);
                let rewritten = seen.insert(owner.node, generation).is_some_and(|old| old != generation);
                let entry = match (owner.style_text, prev) {
                    (Some(text), Some(prev))
                        if !rewritten && prev.sheet.source().unwrap_or("") == text =>
                    {
                        prev
                    }
                    (Some(text), prev) => {
                        if prev.is_some() {
                            dropped.push(owner.node);
                        }
                        StylesheetNodeEntry {
                            node: owner.node,
                            sheet: Arc::new(lumen_css_parser::parse(&text)),
                            disabled: !enabled,
                        }
                    }
                    (None, Some(prev)) => prev,
                    (None, None) => continue,
                };
                let mut entry = entry;
                entry.disabled = !enabled;
                if owner.in_shadow {
                    shadow.insert(owner.node);
                }
                out.push(entry);
            }
            {
                let mut detached = self.detached.lock().unwrap_or_else(|e| e.into_inner());
                let touched = self.state_touched.lock().unwrap_or_else(|e| e.into_inner());
                let mut removed = self.removed.lock().unwrap_or_else(|e| e.into_inner());
                removed.retain(|(n, _)| !out.iter().any(|e| e.node == *n));
                for (node, entry) in old {
                    if touched.contains(&node) {
                        removed.retain(|(n, _)| *n != node);
                        removed.push((node, Arc::clone(&entry.sheet)));
                    }
                    dropped.push(node);
                    detached.retain(|(n, _)| *n != node);
                    detached.push((node, entry.sheet));
                }
                let excess = detached.len().saturating_sub(DETACHED_SHEETS_CAP);
                detached.drain(..excess);
            }
            seen.retain(|n, _| out.iter().any(|e| e.node == *n));
            *nodes = out;
        }
        *self.shadow_owned.lock().unwrap_or_else(|e| e.into_inner()) = shadow;
        if !dropped.is_empty() {
            let mut deltas = self.deltas.lock().unwrap_or_else(|e| e.into_inner());
            let before = deltas.len();
            deltas.retain(|(n, _)| !dropped.contains(n));
            if deltas.len() != before {
                self.dirty.store(true, Ordering::Relaxed);
                self.epoch.fetch_add(1, Ordering::Relaxed);
            }
        }
        *self.synced.lock().unwrap_or_else(|e| e.into_inner()) = Some(dom_epoch);
    }

    /// Индекс листа элемента `nid` в реестре для геттера `.sheet`; `-1`, если
    /// листа у элемента нет.
    ///
    /// Быстрый путь без обхода всего дерева: библиотеки CSS-in-JS читают
    /// `tag.sheet` на каждой вставке правила, вперемешку с мутациями DOM от
    /// фреймворка, и полная сверка на каждое чтение стоила бы O(узлов). Если
    /// элемент уже в реестре, подключён и (для `<style>`) его текст совпадает
    /// с разобранным, ответ берётся сразу; иначе — полная [`Self::sync`].
    pub(crate) fn index_for_owner(&self, nid: u32) -> i32 {
        if self.entry_is_current(nid) {
            return self.index_of(nid);
        }
        self.sync();
        self.index_of(nid)
    }

    /// Индекс записи узла `nid` в реестре без сверки с DOM; `-1`, если нет.
    pub(crate) fn index_of(&self, nid: u32) -> i32 {
        self.nodes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .position(|e| e.node == nid)
            .map_or(-1, |i| i as i32)
    }

    fn entry_is_current(&self, nid: u32) -> bool {
        let source = {
            let nodes = self.nodes.lock().unwrap_or_else(|e| e.into_inner());
            match nodes.iter().find(|e| e.node == nid) {
                Some(e) => e.sheet.source().map(str::to_owned).unwrap_or_default(),
                None => return false,
            }
        };
        let Some(doc) = lock_document_bounded(&self.doc) else {
            // Документ занят — отвечаем по реестру, как и без сверки вообще.
            return true;
        };
        let id = NodeId::from_raw(nid);
        if !doc.contains_id(id) || !doc.is_connected(id) {
            return false;
        }
        match doc.get(id).element_name() {
            Some(name) if name.local == "style" => {
                let unchanged_text = style_text(&doc, id) == source;
                drop(doc);
                let generation = self
                    .touched
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .child_list_gen
                    .get(&id)
                    .copied()
                    .unwrap_or(0);
                let seen = self.child_list_gen.lock().unwrap_or_else(|e| e.into_inner());
                unchanged_text && seen.get(&nid).is_none_or(|&old| old == generation)
            }
            Some(name) if name.local == "link" => !doc.link_has_no_sheet(id),
            Some(_) => true,
            None => false,
        }
    }
}

/// Наложить CSSOM-правки и листы, которых нет в тексте каскада, на копию
/// `sheet`. `None` — накладывать нечего, `sheet` годится как есть.
///
/// Лист каждого узла реестра ищется в исходном тексте каскада
/// ([`Stylesheet::locate_embedded_source`]). Найденный получает свои правки
/// из журнала со сдвигом индексов на `base` (CSSOM-8 вариант C). Не найденный
/// вливается в конец целиком — его запись в реестре уже содержит все правки:
///
/// - пустой `<style>`, наполненный одним `insertRule`, — всегда (его текста
///   в каскаде нет по определению, а правила должны рисоваться);
/// - `<style>` с текстом — только при `merge_unlocated_text`: для листа
///   синхронного флаша, который шелл прислал до вставки этого `<style>`.
///   Каскад шелла собран из того же DOM и такой текст уже содержит, а
///   отсутствие текста там значит, что блок отсёк CSP, — вливать его нельзя.
///
/// Листы теневых деревьев пропускаются: они не относятся к каскаду документа.
pub(crate) fn patched_cascade(
    sheet: &Stylesheet,
    nodes: &[StylesheetNodeEntry],
    deltas: &[(u32, CssomOp)],
    shadow_owned: &HashSet<u32>,
    state_touched: &HashSet<u32>,
    removed: &[(u32, Arc<Stylesheet>)],
    merge_unlocated_text: bool,
) -> Option<Stylesheet> {
    let mut bases: HashMap<u32, usize> = HashMap::new();
    // CSSOM-10: листы с правками дерева заменяют свой кусок текста каскада.
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    let mut unlocated: Vec<&Stylesheet> = Vec::new();
    // BUG-1554: тексты листов, которые скрипт убрал, а шелл успел вложить в каскад.
    for (_, gone) in removed {
        let needle = gone.source().unwrap_or("");
        if !needle.is_empty()
            && let Some((start, end)) = sheet.locate_embedded_source(needle, 0)
        {
            replacements.push((start, end, String::new()));
        }
    }
    let mut cursor = 0usize;
    for entry in nodes {
        if shadow_owned.contains(&entry.node) {
            continue;
        }
        let needle = entry.sheet.source().unwrap_or("");
        let tree_edited = deltas
            .iter()
            .any(|(n, op)| *n == entry.node && matches!(op, CssomOp::TreeEdit));
        let touched = state_touched.contains(&entry.node);
        match sheet.locate_embedded_source(needle, cursor) {
            Some((start, end)) => {
                cursor = end;
                if entry.disabled && touched {
                    // BUG-1554: скрипт выключил лист, который шелл уже вложил в каскад.
                    replacements.push((start, end, String::new()));
                } else if tree_edited {
                    replacements.push((start, end, entry.sheet.cssom_source_text()));
                } else if deltas.iter().any(|(n, _)| *n == entry.node) {
                    let (base, _count) = sheet.cssom_range_for_source_span(start, end);
                    bases.insert(entry.node, base);
                }
            }
            None if entry.disabled => continue,
            None => {
                let wanted = if touched {
                    // BUG-1554: лист включили скриптом или `<link>` догрузился позже каскада шелла.
                    true
                } else if needle.is_empty() {
                    // `@layer`/`@scope`/… не входят в `cssom_rules()` — у правленого листа
                    // смотрим на дерево.
                    !entry.sheet.cssom_rules().is_empty() || (tree_edited && !entry.sheet.cssom_tree().rules.is_empty())
                } else {
                    merge_unlocated_text
                };
                if !wanted {
                    continue;
                }
                // Порядок слоёв зависит от места объявления: пустой `<style>`,
                // в который вставили `@layer`, встаёт на своё место в тексте.
                if needle.is_empty() && tree_edited && sheet.source().is_some() && declares_layers(&entry.sheet) {
                    replacements.push((cursor, cursor, entry.sheet.cssom_source_text()));
                } else {
                    unlocated.push(&entry.sheet);
                }
            }
        }
    }
    if bases.is_empty() && unlocated.is_empty() && replacements.is_empty() {
        return None;
    }
    let mut patched = if replacements.is_empty() {
        sheet.clone()
    } else {
        let mut text = sheet.source().unwrap_or("").to_owned();
        // С конца к началу: ранние смещения не сдвигают поздние.
        // Равные `start` (вставки пустых листов) — тоже в обратном порядке.
        let mut indexed: Vec<_> = replacements.into_iter().enumerate().collect();
        indexed.sort_by_key(|(i, (start, _, _))| std::cmp::Reverse((*start, *i)));
        for (_, (start, end, with)) in indexed {
            text.replace_range(start..end, &with);
        }
        let mut rebuilt = lumen_css_parser::parse(&text);
        rebuilt.carry_color_profile_data(sheet);
        rebuilt
    };
    // По убыванию `base`: правка раньше по странице сдвигает индексы всех
    // листов после неё, а `base` посчитаны один раз по нетронутому листу —
    // см. `Stylesheet::cssom_range_for_source_span`. После подмены текста
    // индексы сдвинуты, поэтому старый путь работает только без подмен.
    let mut order: Vec<u32> = bases.keys().copied().collect();
    order.sort_unstable_by_key(|n| std::cmp::Reverse(bases[n]));
    for node in order {
        let ops: Vec<CssomOp> = deltas
            .iter()
            .filter(|(n, _)| *n == node)
            .map(|(_, op)| op.clone())
            .collect();
        // Best-effort: не применившаяся правка (индекс уже вне диапазона)
        // не мешает остальным — канала ошибки к вызову из JS здесь нет.
        let _ = patched.replay_cssom_ops(bases[&node], &ops);
    }
    for extra in unlocated {
        patched.merge_from(extra.clone());
    }
    Some(patched)
}

/// В листе есть `@layer` (блок или список имён) на верхнем уровне.
fn declares_layers(sheet: &Stylesheet) -> bool {
    use lumen_css_parser::cssom_tree::RuleKind;
    sheet
        .cssom_tree()
        .rules
        .iter()
        .any(|n| matches!(n.kind, RuleKind::Layer | RuleKind::LayerStatement))
}

/// Обход дерева в порядке документа с заходом в теневые деревья: теневой
/// корень хоста обходится сразу после самого хоста, до его детей.
fn collect_owners(doc: &Document, id: NodeId, in_shadow: bool, preferred: Option<&str>, out: &mut Vec<Owner>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data {
        if name.local == "style" {
            out.push(Owner {
                node: id.raw(),
                style_text: Some(style_text(doc, id)),
                in_shadow,
                enabled: doc.sheet_owner_enabled(id, preferred),
            });
            return;
        }
        if name.local == "link" {
            let attr = |n: &str| {
                attrs
                    .iter()
                    .find(|a| a.name.local == n)
                    .map_or("", |a| a.value.as_str())
            };
            let is_sheet = attr("rel")
                .split_ascii_whitespace()
                .any(|r| r.eq_ignore_ascii_case("stylesheet"));
            // `<link disabled>` — не лист вовсе (HTML LS §4.6.7).
            if is_sheet && !attr("href").is_empty() && !doc.link_has_no_sheet(id) {
                out.push(Owner { node: id.raw(), style_text: None, in_shadow, enabled: doc.sheet_owner_enabled(id, preferred) });
            }
            return;
        }
        if let Some(sr) = doc.shadow_root_of(id) {
            collect_owners(doc, sr, true, preferred, out);
        }
    }
    for &child in &node.children {
        collect_owners(doc, child, in_shadow, preferred, out);
    }
}

/// Текст прямых текстовых детей `<style>` — то же, что разбирает шелл
/// (`crates/shell/src/stylesheets.rs::style_element_text`).
fn style_text(doc: &Document, id: NodeId) -> String {
    let mut out = String::new();
    for &child in &doc.get(id).children {
        if let NodeData::Text(s) = &doc.get(child).data {
            out.push_str(s);
        }
    }
    out
}

#[cfg(test)]
mod tree_edit_tests {
    use super::*;

    fn entry(node: u32, sheet: Stylesheet) -> StylesheetNodeEntry {
        StylesheetNodeEntry { node, sheet: Arc::new(sheet), disabled: false }
    }

    /// Правка дерева листа подменяет его кусок текста каскада; соседний лист не трогается.
    #[test]
    fn tree_edit_replaces_the_located_sheet_text_only() {
        let first = "a { color: red; }";
        let second = "p { color: blue; }";
        let cascade = lumen_css_parser::parse(&format!("{first}\n{second}"));
        let mut edited = lumen_css_parser::parse(first);
        edited.cssom_insert_rule(None, "@media print { a { color: green; } }", 1).unwrap();
        let nodes = [entry(1, edited), entry(2, lumen_css_parser::parse(second))];
        let deltas = [(1u32, CssomOp::TreeEdit)];

        let patched = patched_cascade(&cascade, &nodes, &deltas, &HashSet::new(), &HashSet::new(), &[], false).unwrap();
        let text = patched.source().unwrap();
        assert!(text.contains("@media print"), "{text}");
        assert!(text.contains(second), "{text}");
        assert_eq!(patched.media_rules.len(), 1);
    }

    /// `@layer`, вставленный в пустой `<style>`, встаёт на место этого `<style>` в тексте
    /// (порядок слоёв зависит от места объявления), а не в конец каскада.
    #[test]
    fn layer_inserted_into_an_empty_style_lands_at_its_document_position() {
        let cascade = lumen_css_parser::parse("@layer second { p { color: red; } }");
        let mut empty = lumen_css_parser::parse("");
        empty.cssom_insert_rule(None, "@layer first {}", 0).unwrap();
        assert_eq!(empty.source(), Some(""), "source() must stay the DOM text");
        let nodes = [entry(1, empty), entry(2, lumen_css_parser::parse("@layer second { p { color: red; } }"))];
        let deltas = [(1u32, CssomOp::TreeEdit)];

        let patched = patched_cascade(&cascade, &nodes, &deltas, &HashSet::new(), &HashSet::new(), &[], false).unwrap();
        let text = patched.source().unwrap();
        assert!(text.contains("first") && text.contains("second"), "{text:?}");
        assert!(text.find("first").unwrap() < text.find("second").unwrap(), "{text:?}");
    }
}
