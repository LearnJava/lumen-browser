//! Состояния `scroll-state()` контейнера по раскладке (CSS Conditional 5
//! §«scroll-state container queries»).
//!
//! Считается один раз за проход контейнерных запросов
//! (`apply_container_styles`): `scrollable` — из геометрии и текущего `scroll_x`/
//! `scroll_y` бокса, логические стороны — из `writing-mode`/`direction`.
//! `scrolled`, `stuck`, `snapped` пока всегда «нет» (нужны направление
//! пользовательской прокрутки, липкий сдвиг и снап-позиция на момент прохода).
//!
//! Свежее дерево раскладки начинается с `scroll_x == scroll_y == 0`, а прежние
//! смещения возвращает `restore_scroll_positions` уже после прохода запросов.
//! Чтобы запрос видел текущую прокрутку, вызывающий ставит [`install_scroll_hints`]
//! на время раскладки: смещения из подсказок заменяют `scroll_x/y` бокса.

use crate::box_tree::{BoxRole, LayoutBox, PseudoKind};
use crate::style::{Direction, Edge, EdgeFlags, Overflow, ScrollStateInfo, WritingMode};
use crate::{padding_box, scrollable_extent_x, scrollable_extent_y};
use lumen_core::geom::Size;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Текущая прокрутка для запросов `scroll-state()` на время раскладки.
#[derive(Debug, Clone, Default)]
pub struct ScrollHints {
    /// `NodeId::raw()` → `[scroll_x, scroll_y, ..]` прокручиваемых контейнеров.
    pub offsets: HashMap<u32, [f32; 4]>,
    /// Прокрутка страницы (вьюпорта): её контейнер — корневой элемент.
    pub page: (f32, f32),
    /// `NodeId::raw()` → стороны, к которым контейнер ещё можно прокрутить —
    /// по ним `::scroll-button():enabled`/`:disabled` отвечает на свежей
    /// раскладке (CSS Overflow L5 §3.2). Считается по ПРЕЖНЕМУ дереву до
    /// пересборки: геометрии нового ещё нет.
    pub edges: HashMap<u32, EdgeFlags>,
    /// CSS Overflow L5 §3.1.7 — `NodeId::raw()` цели → её отношение к
    /// активному `::scroll-marker` её группы ([`MARKER_BEFORE`]/
    /// [`MARKER_CURRENT`]/[`MARKER_AFTER`]). Считается по ПРЕЖНЕМУ дереву
    /// ([`scroll_marker_relations`]): позиции целей и смещения там финальные.
    pub markers: HashMap<u32, u8>,
}

/// `::scroll-marker` цели стоит в flat tree order ДО активного маркера её
/// scroll marker group (`:target-before`).
pub const MARKER_BEFORE: u8 = 1;
/// `::scroll-marker` — активный маркер группы (`:target-current`).
pub const MARKER_CURRENT: u8 = 2;
/// `::scroll-marker` цели стоит ПОСЛЕ активного маркера (`:target-after`).
pub const MARKER_AFTER: u8 = 3;

thread_local! {
    /// Подсказки текущей раскладки этого потока (см. [`install_scroll_hints`]).
    static HINTS: RefCell<Option<Rc<ScrollHints>>> = const { RefCell::new(None) };
}

/// Снимает подсказки прокрутки при выходе из области видимости.
#[must_use = "подсказки действуют, пока жив возвращённый охранник"]
pub struct ScrollHintsGuard(Option<Rc<ScrollHints>>);

impl Drop for ScrollHintsGuard {
    fn drop(&mut self) {
        HINTS.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// Текущая прокрутка для `@container scroll-state(…)` во время раскладки этого потока.
/// Без подсказок контейнер считается непрокрученным (свежее дерево). Прежние подсказки
/// возвращаются при сбросе охранника.
pub fn install_scroll_hints(hints: ScrollHints) -> ScrollHintsGuard {
    let prev = HINTS.with(|h| h.borrow_mut().replace(Rc::new(hints)));
    ScrollHintsGuard(prev)
}

/// `(NodeId::raw(), физическое направление)` → можно ли ещё прокрутить
/// контейнер в эту сторону — таблица [`BUTTON_ENABLED`].
type ButtonEnabledMap = HashMap<(u32, u8), bool>;

thread_local! {
    /// Активность `::scroll-button()` на время сборки псевдо-боксов одного
    /// ребёнка. Читается матчером (`style::pseudo`) для
    /// `:enabled`/`:disabled`; без записи кнопка считается активной.
    static BUTTON_ENABLED: RefCell<Option<Rc<ButtonEnabledMap>>> = const { RefCell::new(None) };
}

/// Снимает таблицу активности кнопок при выходе из области видимости.
#[must_use = "таблица действует, пока жив возвращённый охранник"]
pub struct ButtonStatesGuard(Option<Rc<ButtonEnabledMap>>);

impl Drop for ButtonStatesGuard {
    fn drop(&mut self) {
        BUTTON_ENABLED.with(|b| *b.borrow_mut() = self.0.take());
    }
}

/// Коды физических направлений `::scroll-button()` — порядок общий для
/// [`install_scroll_button_states`] и [`scroll_button_enabled`].
pub(crate) const DIR_UP: u8 = 0;
pub(crate) const DIR_DOWN: u8 = 1;
pub(crate) const DIR_LEFT: u8 = 2;
pub(crate) const DIR_RIGHT: u8 = 3;

/// Код физического направления кнопки (`"up"`/`"down"`/`"left"`/`"right"`).
pub(crate) fn scroll_dir_code(dir: &str) -> Option<u8> {
    Some(match dir {
        "up" => DIR_UP,
        "down" => DIR_DOWN,
        "left" => DIR_LEFT,
        "right" => DIR_RIGHT,
        _ => return None,
    })
}

/// Ставит таблицу «можно ли прокрутить в сторону кнопки» на время вычисления
/// стилей её псевдо-боксов. Прежняя таблица возвращается при сбросе охранника.
pub fn install_scroll_button_states(
    states: HashMap<(u32, u8), bool>,
) -> ButtonStatesGuard {
    let prev = BUTTON_ENABLED.with(|b| b.borrow_mut().replace(Rc::new(states)));
    ButtonStatesGuard(prev)
}

/// Активна ли `::scroll-button()` направления `dir` у узла `node` — `None`,
/// когда таблица не установлена или направления в ней нет (матчер тогда
/// считает кнопку активной: без данных о прокрутке `:disabled` не доказан).
pub(crate) fn scroll_button_enabled(node: lumen_dom::NodeId, dir: &str) -> Option<bool> {
    let code = scroll_dir_code(dir)?;
    BUTTON_ENABLED.with(|b| {
        b.borrow().as_ref().and_then(|m| m.get(&(node.raw(), code)).copied())
    })
}

/// Роль бокса относительно вьюпорта (CSS Overflow 3 §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewportRole {
    /// Обычный бокс.
    None,
    /// Корневой элемент: его прокручиваемая область — вьюпорт.
    Root,
    /// `body`, чей `overflow` ушёл во вьюпорт: контейнером прокрутки не остаётся.
    Propagated,
}

/// Таблица стилей спрашивает `scroll-state(…)`: после прокрутки контейнера запросы надо
/// пересчитать, то есть заново пройти раскладку (стили потомков строятся один раз).
pub fn sheet_has_scroll_state_queries(sheet: &lumen_css_parser::Stylesheet) -> bool {
    sheet.container_rules.iter().any(|r| r.preludes().any(|(_, c)| c.to_ascii_lowercase().contains("scroll-state(")))
}

/// Корневой элемент (`html`) — контейнер `scroll-state`: прокрутка страницы меняет ответы.
pub fn root_is_scroll_state_container(root: &LayoutBox) -> bool {
    root.children
        .iter()
        .find(|c| !matches!(c.kind, crate::box_tree::BoxKind::Skip))
        .is_some_and(|html| html.style.container_scroll_state)
}

/// Ненулевые смещения прокрутки дерева (`NodeId::raw()` → `[scroll_x, scroll_y, 0, 0]`) —
/// то, что [`install_scroll_hints`] и `restore_scroll_positions` возвращают в свежее дерево.
pub fn scroll_offsets(root: &LayoutBox) -> HashMap<u32, [f32; 4]> {
    let mut out = HashMap::new();
    let mut stack = vec![root];
    while let Some(b) = stack.pop() {
        if b.scroll_x != 0.0 || b.scroll_y != 0.0 {
            // Первый бокс узла в порядке обхода — главный.
            out.entry(b.node.raw()).or_insert([b.scroll_x, b.scroll_y, 0.0, 0.0]);
        }
        stack.extend(b.children.iter().rev());
    }
    out
}

/// Смещение контейнера: из подсказок, иначе из самого бокса.
fn current_offset(b: &LayoutBox, role: ViewportRole) -> (f32, f32) {
    HINTS
        .with(|h| {
            h.borrow().as_ref().and_then(|h| match role {
                ViewportRole::Root => Some(h.page),
                _ => h.offsets.get(&b.node.raw()).map(|s| (s[0], s[1])),
            })
        })
        .unwrap_or((b.scroll_x, b.scroll_y))
}

/// Допуск сравнения смещений прокрутки (CSS px).
const EPS: f32 = 0.5;

/// Физические стороны `block-start` и `inline-start` (CSS Writing Modes 4 §6.2).
fn logical_starts(mode: WritingMode, dir: Direction) -> (Edge, Edge) {
    let rtl = dir == Direction::Rtl;
    match mode {
        WritingMode::HorizontalTb => (Edge::Top, if rtl { Edge::Right } else { Edge::Left }),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => (Edge::Right, if rtl { Edge::Bottom } else { Edge::Top }),
        WritingMode::VerticalLr => (Edge::Left, if rtl { Edge::Bottom } else { Edge::Top }),
        WritingMode::SidewaysLr => (Edge::Left, if rtl { Edge::Top } else { Edge::Bottom }),
    }
}

/// Стороны, к которым бокс ещё можно прокрутить, по физическим краям
/// `[top, right, bottom, left]`. `user_scrollable` решает, считается ли ось
/// прокручиваемой вообще (для `scroll-state()` — только `scroll`/`auto`,
/// для `::scroll-button()` — ещё и `hidden`: кнопки листают и её,
/// CSS Overflow L5 §3.2, WPT `scroll-buttons-disabled-snapping`).
fn scrollable_edges(
    b: &LayoutBox,
    role: ViewportRole,
    viewport: Size,
    user_scrollable: impl Fn(Overflow) -> bool,
) -> EdgeFlags {
    let s = &b.style;
    let (scroll_x, scroll_y) = current_offset(b, role);
    // Размер области прокрутки: padding box контейнера или вьюпорт.
    let (view_w, view_h) = match role {
        ViewportRole::Root => (viewport.width, viewport.height),
        _ => {
            let pb = padding_box(b);
            (pb.width, pb.height)
        }
    };
    let (mut top, mut right, mut bottom, mut left) = (false, false, false, false);
    if user_scrollable(s.overflow_x) {
        let (min_x, max_x) = scrollable_extent_x(b);
        let x = scroll_x.clamp(min_x, (max_x - view_w).max(min_x));
        left = x > min_x + EPS;
        right = x < max_x - view_w - EPS;
    }
    if user_scrollable(s.overflow_y) {
        let (min_y, max_y) = scrollable_extent_y(b);
        let y = scroll_y.clamp(min_y, (max_y - view_h).max(min_y));
        top = y > min_y + EPS;
        bottom = y < max_y - view_h - EPS;
    }
    EdgeFlags([top, right, bottom, left])
}

/// Состояния `scroll-state()` для бокса с `container-type: scroll-state`.
/// `role` — отношение бокса ко вьюпорту: у корневого элемента прокручивается вьюпорт
/// (смещение страницы, размер вьюпорта), а `body`, чей `overflow` ушёл во вьюпорт
/// (CSS Overflow 3 §3.5), контейнером прокрутки не остаётся.
pub(crate) fn scroll_state_info(b: &LayoutBox, role: ViewportRole, viewport: Size) -> ScrollStateInfo {
    let s = &b.style;
    let (block_start, inline_start) = logical_starts(s.writing_mode, s.direction);
    if role == ViewportRole::Propagated {
        return ScrollStateInfo { block_start, inline_start, ..ScrollStateInfo::default() };
    }
    // Пользователь прокручивает только `scroll`/`auto`; `hidden`/`clip`/`visible` — нет.
    // Вьюпорт прокручивается, пока его `overflow` не `hidden`/`clip`.
    let user_scrollable = |o: Overflow| match role {
        ViewportRole::Root => !matches!(o, Overflow::Hidden | Overflow::Clip),
        _ => matches!(o, Overflow::Scroll | Overflow::Auto),
    };
    ScrollStateInfo {
        scrollable: scrollable_edges(b, role, viewport, user_scrollable),
        block_start,
        inline_start,
        ..ScrollStateInfo::default()
    }
}

/// `overflow`, при котором `::scroll-button()` может сдвинуть бокс: кнопки
/// работают и на `hidden` (программная прокрутка — WPT `scroll-buttons-*`
/// гоняют `scrollLeft`/`scrollTop` именно на нём); `clip` и `visible`
/// прокрутки не имеют вовсе.
fn button_scrollable(o: Overflow) -> bool {
    matches!(o, Overflow::Scroll | Overflow::Auto | Overflow::Hidden)
}

/// Стороны, к которым каждый потенциальный scroll-контейнер ещё можно
/// прокрутить — `NodeId::raw()` → [`EdgeFlags`], для `ScrollHints::edges`.
/// Считается по уже свёрстанному дереву (геометрия готова); зовётся шеллом
/// ДО пересборки, чтобы новая раскладка увидела состояние прокрутки в
/// [`install_scroll_hints`]. Боксы без `overflow`-клиппинга и не-элементы
/// не записываются — у них кнопки всё равно `:disabled`.
pub fn scroll_state_edges(root: &LayoutBox, viewport: Size) -> HashMap<u32, EdgeFlags> {
    fn first_box(b: &LayoutBox) -> Option<&LayoutBox> {
        b.children.iter().find(|c| !matches!(c.kind, crate::box_tree::BoxKind::Skip))
    }
    let mut out = HashMap::new();
    // Роли вьюпорта — как у `viewport_propagating_nodes`: `html` всегда корневой
    // контеймент, `body` — только пока `overflow` корня `visible` (Overflow 3 §3.5).
    let (html_node, body_node) = match first_box(root) {
        Some(html) => {
            let body = if matches!(html.style.overflow_x, Overflow::Visible)
                && matches!(html.style.overflow_y, Overflow::Visible)
            {
                first_box(html).map(|b| b.node)
            } else {
                None
            };
            (html.node, body)
        }
        None => return out,
    };
    let mut stack: Vec<(&LayoutBox, ViewportRole)> = vec![(root, ViewportRole::None)];
    while let Some((b, role)) = stack.pop() {
        let role = if b.node == html_node {
            ViewportRole::Root
        } else if Some(b.node) == body_node {
            ViewportRole::Propagated
        } else {
            role
        };
        let s = &b.style;
        let clips = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto | Overflow::Hidden | Overflow::Clip)
            || matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto | Overflow::Hidden | Overflow::Clip);
        if b.origin.role == crate::box_tree::BoxRole::Element && (clips || role == ViewportRole::Root) {
            let pred = |o: Overflow| match role {
                ViewportRole::Root => !matches!(o, Overflow::Hidden | Overflow::Clip),
                ViewportRole::Propagated => false,
                ViewportRole::None => button_scrollable(o),
            };
            out.insert(b.node.raw(), scrollable_edges(b, role, viewport, pred));
        }
        stack.extend(b.children.iter().map(|c| (c, ViewportRole::None)));
    }
    out
}

/// Отношение `::scroll-marker` узла `node` к активному маркеру его группы —
/// [`MARKER_BEFORE`]/[`MARKER_CURRENT`]/[`MARKER_AFTER`], `None` без данных
/// (матчер тогда не матчит `:target-*`).
pub(crate) fn scroll_marker_relation(node: lumen_dom::NodeId) -> Option<u8> {
    HINTS.with(|h| {
        h.borrow().as_ref().and_then(|h| h.markers.get(&node.raw()).copied())
    })
}

/// Подсказанные [`scroll_state_edges`] для узла на время текущей раскладки —
/// `None`, когда подсказок нет (первая раскладка, движок без keeping-scroll).
pub(crate) fn hinted_scroll_edges(node: lumen_dom::NodeId) -> Option<EdgeFlags> {
    HINTS.with(|h| {
        h.borrow().as_ref().and_then(|h| h.edges.get(&node.raw()).copied())
    })
}

/// CSS Overflow L5 §3.1.7/§3.1.8 — отношение каждого `::scroll-marker` к
/// активному маркеру его группы по УЖЕ свёрстанному дереву `root` (геометрия
/// и смещения финальные — зовётся шеллом до пересборки, результат уходит в
/// [`ScrollHints::markers`]).
///
/// `explicit` — `NodeId::raw()` scroll-контейнера → `NodeId::raw()` цели,
/// которую последняя активация маркера сделала current scroll target
/// (шелл помнит её между раскладками); она выигрывает у позиционного выбора.
/// Без неё активный маркер — последняя цель, чья scroll-into-view позиция по
/// главной прокручиваемой оси не дальше текущего смещения (спека выбирает по
/// «eventual scroll position» с redistribution краёв — здесь берётся
/// основная часть алгоритма без неё), при равенстве/пустом выборе — первая.
pub fn scroll_marker_relations(
    root: &LayoutBox,
    explicit: &HashMap<u32, u32>,
    scroll_offsets: &HashMap<u32, [f32; 4]>,
) -> HashMap<u32, u8> {
    // Все `::scroll-marker-group` боксы дерева (каждый — одна группа).
    let mut groups: Vec<&LayoutBox> = Vec::new();
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        if b.origin.role == BoxRole::Pseudo(PseudoKind::ScrollMarkerGroup) {
            groups.push(b);
        }
        stack.extend(b.children.iter());
    }
    let mut out = HashMap::new();
    for g in groups {
        let Some(container) = g.origin.node else { continue };
        // Цели группы в flat tree order: прямой обход поддерева группы,
        // первый `::scroll-marker` бокс на ветку (его inline-run несёт тот же
        // `PseudoKind`, но не спускаемся внутрь найденного).
        let mut targets: Vec<lumen_dom::NodeId> = Vec::new();
        // Стек: `pop` снимает конец — детей кладём в обратном порядке, чтобы
        // обход шёл в flat tree order, как `collect_markers` собирал маркеры.
        let mut tstack: Vec<&LayoutBox> = g.children.iter().rev().collect();
        while let Some(b) = tstack.pop() {
            if b.origin.role == BoxRole::Pseudo(PseudoKind::ScrollMarker) {
                if let Some(n) = b.origin.node {
                    targets.push(n);
                }
                continue;
            }
            tstack.extend(b.children.iter().rev());
        }
        if targets.is_empty() {
            continue;
        }
        let active_raw = explicit
            .get(&container.raw())
            .copied()
            .filter(|raw| targets.iter().any(|t| t.raw() == *raw));
        let active_idx = match active_raw {
            Some(raw) => targets.iter().position(|t| t.raw() == raw).unwrap_or(0),
            None => marker_active_by_position(root, container, &targets, scroll_offsets),
        };
        for (i, t) in targets.iter().enumerate() {
            out.insert(
                t.raw(),
                if i < active_idx {
                    MARKER_BEFORE
                } else if i == active_idx {
                    MARKER_CURRENT
                } else {
                    MARKER_AFTER
                },
            );
        }
    }
    out
}

/// Позиционный выбор активного маркера — пример алгоритма §3.1.8: ось —
/// block-ось writing-mode контейнера, если по ней есть диапазон прокрутки,
/// иначе inline-ось; позиции целей — scroll-into-view (start-align) с
/// redistribution краёв; активна наибольшая позиция, не дальше текущего
/// («eventual») смещения или не дальше полупорта ВПЕРЕДИ неё при условии,
/// что предыдущая цель дальше полупорта ПОЗАДИ; без кандидатов — первая.
fn marker_active_by_position(
    root: &LayoutBox,
    container: lumen_dom::NodeId,
    targets: &[lumen_dom::NodeId],
    scroll_offsets: &HashMap<u32, [f32; 4]>,
) -> usize {
    let Some(cbox) = find_element_box(root, container) else { return 0 };
    let clip = padding_box(cbox);
    // Смещения могут быть новее дерева (JS-флаш считает отношения по
    // basis-дереву предыдущего флаша, а `scroll_states` уже видит
    // same-tick `scrollLeft=`) — тогда карта побеждает запечённое в боксе.
    let (scroll_x, scroll_y) = scroll_offsets
        .get(&container.raw())
        .map(|s| (s[0], s[1]))
        .unwrap_or((cbox.scroll_x, cbox.scroll_y));
    // Главная ось — block-направление writing-mode: у `horizontal-tb` это Y,
    // у вертикальных/sideways — X.
    let primary_y = matches!(cbox.style.writing_mode, WritingMode::HorizontalTb);
    let axes = if primary_y { [true, false] } else { [false, true] };
    // `(offset, origin, scrollport, scroll_range)` выбранной оси.
    let mut axis = axes[0];
    let mut geom = (scroll_y, clip.y, clip.height, 0.0f32);
    for &a in &axes {
        let (extent_min, extent_max, port, off, orig) = if a {
            let (min_y, max_y) = scrollable_extent_y(cbox);
            (min_y, max_y, clip.height, scroll_y, clip.y)
        } else {
            let (min_x, max_x) = scrollable_extent_x(cbox);
            (min_x, max_x, clip.width, scroll_x, clip.x)
        };
        let range = extent_max - extent_min - port;
        if range > 0.5 {
            axis = a;
            geom = (off, orig, port, range);
            break;
        }
    }
    let (offset, origin, scrollport, scroll_range) = geom;
    // Scroll-into-view позиции целей вдоль оси (start-align: смещение, при
    // котором начало цели совпадает с началом scrollport). Цели без бокса
    // пропускаются — активный выбор идёт по остальным.
    let mut positions: Vec<Option<f32>> = Vec::with_capacity(targets.len());
    for &t in targets {
        positions.push(
            find_element_box(root, t)
                .map(|tb| if axis { tb.rect.y - origin } else { tb.rect.x - origin }),
        );
    }
    // §3.1.8 redistribution: недостижимые края диапазона сжимаются в
    // `distribute range = min(scrollport/8, scroll range/2)` у обоих концов.
    let dr = (scrollport / 8.0).min(scroll_range / 2.0);
    if dr > 0.0 {
        let mut before_min = f32::INFINITY;
        let mut after_max = f32::NEG_INFINITY;
        for p in positions.iter().flatten() {
            if *p < dr {
                before_min = before_min.min(*p);
            } else if *p > scroll_range - dr {
                after_max = after_max.max(*p);
            }
        }
        for p in positions.iter_mut().flatten() {
            if *p < dr && dr > before_min {
                *p = (*p - before_min) / (dr - before_min) * dr;
            } else if *p > scroll_range - dr && after_max > scroll_range - dr {
                *p = (*p - (scroll_range - dr)) / (after_max - (scroll_range - dr)) * dr
                    + (scroll_range - dr);
            }
        }
    }
    // Выбор — Chromium `ScrollMarkerChooser::ChooseInternal`: кандидаты по
    // возрастанию позиции; «before» — позиция <= текущего смещения (побеждает
    // наибольшая), «after» — позиция < смещения + полупорта (побеждает
    // наименьшая); after выигрывает только при строго меньшем расстоянии.
    // Первый кандидат всегда становится before-победителем — когда все цели
    // впереди, выбирается ближайшая к текущей позиции.
    let mut order: Vec<(f32, usize)> = positions
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.map(|p| (p, i)))
        .collect();
    order.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let half = scrollport / 2.0;
    let intended = offset.abs();
    let mut before: Option<(f32, usize)> = None;
    let mut after: Option<(f32, usize)> = None;
    for &(pos, idx) in &order {
        let pos = pos.abs();
        match before {
            None => before = Some((pos, idx)),
            Some((bp, _)) if pos <= intended && pos > bp => before = Some((pos, idx)),
            _ => {}
        }
        if pos > intended && pos < intended + half && after.is_none_or(|(ap, _)| pos < ap) {
            after = Some((pos, idx));
        }
    }
    match (before, after) {
        (Some((bp, bi)), Some((ap, ai))) => {
            if ap - intended < intended - bp { ai } else { bi }
        }
        (Some((_, bi)), None) => bi,
        (None, Some((_, ai))) => ai,
        (None, None) => 0,
    }
}

/// Цели `::scroll-marker` группы контейнера `container` в flat tree order —
/// `origin.node` каждого `::scroll-marker` бокса, найденного внутри его
/// `::scroll-marker-group` (обход как в [`scroll_marker_relations`]). Пусто,
/// когда группы или маркеров нет. Зовётся шеллом для arrow-навигации
/// (§3.1.9: стрелки на сфокусированном маркере ходят по целям группы).
pub fn scroll_marker_group_order(root: &LayoutBox, container: lumen_dom::NodeId) -> Vec<lumen_dom::NodeId> {
    let mut out = Vec::new();
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        let is_group = b.origin.role == BoxRole::Pseudo(PseudoKind::ScrollMarkerGroup)
            && b.origin.node == Some(container);
        if is_group {
            let mut tstack: Vec<&LayoutBox> = b.children.iter().rev().collect();
            while let Some(m) = tstack.pop() {
                if m.origin.role == BoxRole::Pseudo(PseudoKind::ScrollMarker) {
                    if let Some(n) = m.origin.node {
                        out.push(n);
                    }
                    continue;
                }
                tstack.extend(m.children.iter().rev());
            }
            return out;
        }
        stack.extend(b.children.iter());
    }
    out
}

/// Активная цель группы `::scroll-marker` контейнера `container` — явная
/// current-target (как в [`scroll_marker_relations`]) или позиционный выбор
/// ([`marker_active_by_position`]). `None`, когда группы или маркеров нет.
/// Зовётся шеллом при скролле: фокус на маркере следует за новой активной
/// целью (§3.1.8 scroll tracking + §3.3 focus).
pub fn scroll_marker_active_target(
    root: &LayoutBox,
    container: lumen_dom::NodeId,
    explicit: &HashMap<u32, u32>,
    scroll_offsets: &HashMap<u32, [f32; 4]>,
) -> Option<lumen_dom::NodeId> {
    let targets = scroll_marker_group_order(root, container);
    if targets.is_empty() {
        return None;
    }
    let active_raw = explicit
        .get(&container.raw())
        .copied()
        .filter(|raw| targets.iter().any(|t| t.raw() == *raw));
    let idx = match active_raw {
        Some(raw) => targets.iter().position(|t| t.raw() == raw)?,
        None => marker_active_by_position(root, container, &targets, scroll_offsets),
    };
    Some(targets[idx])
}

/// CSS Overflow L5 §3.1.9 — сдвиг по группе `::scroll-marker` для стрелки,
/// указывающей в физическую сторону `edge`: `1` — по ходу чтения группы
/// (block-end/inline-end), `-1` — против хода. Порядок группы — flat tree
/// order ([`scroll_marker_group_order`]), совпадающий с порядком чтения.
pub fn scroll_marker_arrow_step(mode: WritingMode, dir: Direction, edge: Edge) -> i8 {
    let (block_start, inline_start) = logical_starts(mode, dir);
    let opposite = |e: Edge| match e {
        Edge::Top => Edge::Bottom,
        Edge::Right => Edge::Left,
        Edge::Bottom => Edge::Top,
        Edge::Left => Edge::Right,
    };
    if edge == opposite(block_start) || edge == opposite(inline_start) {
        1
    } else {
        -1
    }
}

/// Первый element-бокс узла `node` в pre-order (как `find_box_by_node`, но
/// только `BoxRole::Element` — псевдо-боксы делят `node` с родителем и не
/// должны перехватывать поиск).
fn find_element_box(root: &LayoutBox, node: lumen_dom::NodeId) -> Option<&LayoutBox> {
    if root.origin.role == BoxRole::Element && root.node == node {
        return Some(root);
    }
    root.children.iter().find_map(|c| find_element_box(c, node))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_starts_follow_writing_mode_and_direction() {
        use Direction::{Ltr, Rtl};
        assert_eq!(logical_starts(WritingMode::HorizontalTb, Ltr), (Edge::Top, Edge::Left));
        assert_eq!(logical_starts(WritingMode::HorizontalTb, Rtl), (Edge::Top, Edge::Right));
        assert_eq!(logical_starts(WritingMode::VerticalRl, Ltr), (Edge::Right, Edge::Top));
        assert_eq!(logical_starts(WritingMode::VerticalLr, Rtl), (Edge::Left, Edge::Bottom));
        assert_eq!(logical_starts(WritingMode::SidewaysLr, Ltr), (Edge::Left, Edge::Bottom));
    }
}
