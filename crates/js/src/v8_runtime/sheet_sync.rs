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
use super::runtime::DomTouched;
use super::style_flush::CssomDeltaLog;

/// Лист шелла до наложения CSSOM-правок и ревизия результата наложения —
/// см. [`SheetSync::pristine`].
pub(crate) type PristineCascade = Arc<Mutex<Option<(StylesheetRevision, Arc<Stylesheet>)>>>;

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
    /// Последний результат [`super::V8JsRuntime::patch_cascade`]: его ревизия
    /// и лист шелла, на который накладывались правки. Шелл отдаёт результат
    /// обратно как лист синхронного флаша, и флаш по ревизии берёт исходный
    /// лист, чтобы не наложить те же правки второй раз.
    pub(crate) pristine: PristineCascade,
}

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
            if deltas.is_empty() {
                return None;
            }
            let shadow = sync.shadow_owned.lock().unwrap_or_else(|e| e.into_inner());
            patched_cascade(sheet, &nodes, &deltas, &shadow, false)?
        };
        *sync.pristine.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((patched.revision(), Arc::new(sheet.clone())));
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
        collect_owners(&doc, doc.root(), false, &mut owners);
        drop(doc);

        let mut dropped: Vec<u32> = Vec::new();
        let mut shadow = HashSet::new();
        {
            let mut nodes = self.nodes.lock().unwrap_or_else(|e| e.into_inner());
            let mut old: HashMap<u32, StylesheetNodeEntry> =
                nodes.drain(..).map(|e| (e.node, e)).collect();
            let mut out = Vec::with_capacity(owners.len());
            for owner in owners {
                let prev = old.remove(&owner.node);
                let entry = match (owner.style_text, prev) {
                    (Some(text), Some(prev)) if prev.sheet.source().unwrap_or("") == text => prev,
                    (Some(text), prev) => {
                        if prev.is_some() {
                            dropped.push(owner.node);
                        }
                        StylesheetNodeEntry {
                            node: owner.node,
                            sheet: Arc::new(lumen_css_parser::parse(&text)),
                            disabled: false,
                        }
                    }
                    (None, Some(prev)) => prev,
                    (None, None) => continue,
                };
                if owner.in_shadow {
                    shadow.insert(owner.node);
                }
                out.push(entry);
            }
            dropped.extend(old.into_keys());
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
        if !doc.contains_id(id) || !is_connected(&doc, id) {
            return false;
        }
        match doc.get(id).element_name() {
            Some(name) if name.local == "style" => style_text(&doc, id) == source,
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
    merge_unlocated_text: bool,
) -> Option<Stylesheet> {
    let mut bases: HashMap<u32, usize> = HashMap::new();
    let mut unlocated: Vec<&Stylesheet> = Vec::new();
    let mut cursor = 0usize;
    for entry in nodes {
        if shadow_owned.contains(&entry.node) {
            continue;
        }
        let needle = entry.sheet.source().unwrap_or("");
        match sheet.locate_embedded_source(needle, cursor) {
            Some((start, end)) => {
                cursor = end;
                if deltas.iter().any(|(n, _)| *n == entry.node) {
                    let (base, _count) = sheet.cssom_range_for_source_span(start, end);
                    bases.insert(entry.node, base);
                }
            }
            None => {
                let wanted = if needle.is_empty() {
                    !entry.sheet.cssom_rules().is_empty()
                } else {
                    merge_unlocated_text
                };
                if wanted {
                    unlocated.push(&entry.sheet);
                }
            }
        }
    }
    if bases.is_empty() && unlocated.is_empty() {
        return None;
    }
    let mut patched = sheet.clone();
    // По убыванию `base`: правка раньше по странице сдвигает индексы всех
    // листов после неё, а `base` посчитаны один раз по нетронутому листу —
    // см. `Stylesheet::cssom_range_for_source_span`.
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

/// Обход дерева в порядке документа с заходом в теневые деревья: теневой
/// корень хоста обходится сразу после самого хоста, до его детей.
fn collect_owners(doc: &Document, id: NodeId, in_shadow: bool, out: &mut Vec<Owner>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data {
        if name.local == "style" {
            out.push(Owner {
                node: id.raw(),
                style_text: Some(style_text(doc, id)),
                in_shadow,
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
            if is_sheet && !attr("href").is_empty() {
                out.push(Owner { node: id.raw(), style_text: None, in_shadow });
            }
            return;
        }
        if let Some(sr) = doc.shadow_root_of(id) {
            collect_owners(doc, sr, true, out);
        }
    }
    for &child in &node.children {
        collect_owners(doc, child, in_shadow, out);
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

/// Подключён ли узел к документу (DOM §4.2 «connected»), с переходом из
/// теневого корня к его хосту.
fn is_connected(doc: &Document, id: NodeId) -> bool {
    let root = doc.root();
    let mut cur = id;
    loop {
        if cur == root {
            return true;
        }
        match doc.get(cur).parent {
            Some(p) => cur = p,
            None => match doc.shadow_host_of(cur) {
                Some(host) => cur = host,
                None => return false,
            },
        }
    }
}
