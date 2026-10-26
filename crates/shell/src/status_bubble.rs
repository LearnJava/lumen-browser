//! UX-STATUS-BUBBLE: пузырь с адресом ссылки у нижнего левого угла страницы
//! (docs/research/browser-ux/report.md §4.3 LINK-07…LINK-11).
//!
//! Модуль чистый — время (`now`, миллисекунды) и мерка ширины символа
//! параметры, окна нет, поэтому правила проверяются юнит-тестами:
//!
//! * [`StatusBubble`] — часы показа: 80 мс до появления (перезапуск на каждое
//!   движение мыши), fade-in 120 мс, 250 мс до ухода, fade-out 200 мс; смена
//!   ссылки при видимом пузыре подменяет текст сразу, без fade (LINK-07).
//! * [`elide_middle`] — сокращение середины пути: схема и хост не режутся
//!   никогда, хвост (имя файла, запрос, якорь) остаётся (LINK-08).
//! * [`layout`] — рамка пузыря: до половины ширины страницы, над горизонтальной
//!   полосой прокрутки, не ближе [`MOUSE_PAD`] к курсору (LINK-08, LINK-10).
//! * [`build_overlay`] — команды отрисовки. Пузырь — только рисунок поверх
//!   страницы, не узел и не хит-цель, поэтому мышь для него прозрачна.
//!
//! Склейка с окном (откуда берётся адрес, кто будит цикл) — `lumen/status_bubble.rs`.

use lumen_core::geom::Rect;
use lumen_layout::{BorderStyle, Color, FontStretch, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::panels::themes::Palette;

/// Задержка перед появлением (Chromium `kShowDelay`, Firefox — 80 мс).
const SHOW_DELAY_MS: f64 = 80.0;
/// Длительность появления (`kShowFadeDuration`).
const FADE_IN_MS: f64 = 120.0;
/// Задержка перед уходом (`kHideDelay`).
const HIDE_DELAY_MS: f64 = 250.0;
/// Длительность ухода (`kHideFadeDuration`).
const FADE_OUT_MS: f64 = 200.0;
/// Шаг кадра, пока идёт fade.
const FRAME_MS: f64 = 16.0;

/// Зазор до курсора, ближе которого пузырь уезжает (Chromium `kMousePadding`).
pub(crate) const MOUSE_PAD: f32 = 20.0;
/// Доля ширины страницы, которую пузырь занимает сразу (LINK-08).
const MAX_WIDTH_FRACTION: f32 = 0.5;
const FONT_SIZE: f32 = 12.0;
const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 4.0;
/// Радиус внутреннего верхнего угла; внешние углы стоят вплотную к краю страницы.
const CORNER_RADIUS: f32 = 6.0;
/// Доля бюджета, которую хвост не превышает, если не помещается целиком.
const TAIL_SHARE: f32 = 0.65;
const ELLIPSIS: char = '…';

/// Линейный переход непрозрачности.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fade {
    from: f32,
    to: f32,
    start: f64,
    dur: f64,
}

impl Fade {
    fn at(&self, now: f64) -> f32 {
        if self.dur <= 0.0 {
            return self.to;
        }
        let t = ((now - self.start) / self.dur).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * t
    }

    fn done(&self, now: f64) -> bool {
        now >= self.start + self.dur
    }
}

/// Что вернул [`StatusBubble::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Tick {
    /// Кадр изменился (идёт fade, фаза сменилась) — нужна перерисовка.
    pub(crate) repaint: bool,
    /// Когда цикл должен проснуться снова (мс той же шкалы, что `now`).
    pub(crate) wake: Option<f64>,
}

/// Часы пузыря: какой текст показан и с какой непрозрачностью.
#[derive(Debug, Default)]
pub(crate) struct StatusBubble {
    text: String,
    /// Непрозрачность вне перехода.
    level: f32,
    fade: Option<Fade>,
    show_at: Option<f64>,
    hide_at: Option<f64>,
    /// Ширина, ниже которой пузырь не сужается, пока виден: между соседними
    /// ссылками он не прыгает (LINK-08).
    min_width: f32,
    /// «Анимации выключены» в ОС: задержки остаются, плавных переходов нет.
    reduced_motion: bool,
}

impl StatusBubble {
    /// Непрозрачность в момент `now`.
    pub(crate) fn alpha(&self, now: f64) -> f32 {
        self.fade.map_or(self.level, |f| f.at(now))
    }

    /// Нечего ни рисовать, ни ждать — цикл можно не будить.
    pub(crate) fn is_idle(&self) -> bool {
        self.text.is_empty() && self.fade.is_none() && self.show_at.is_none() && self.hide_at.is_none()
    }

    /// Показанный текст и непрозрачность, если пузырь сейчас виден.
    pub(crate) fn visible(&self, now: f64) -> Option<(&str, f32)> {
        let a = self.alpha(now);
        (a > 0.0 && !self.text.is_empty()).then_some((self.text.as_str(), a))
    }

    /// Включить или выключить режим «без анимаций» (`prefers-reduced-motion`).
    pub(crate) fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
    }

    fn fade_ms(&self, ms: f64) -> f64 {
        if self.reduced_motion { 0.0 } else { ms }
    }

    /// Ширина-«пол» для [`layout`].
    pub(crate) fn min_width(&self) -> f32 {
        self.min_width
    }

    /// Запомнить ширину нарисованного пузыря (только растёт, пока пузырь виден).
    pub(crate) fn note_width(&mut self, width: f32) {
        self.min_width = self.min_width.max(width);
    }

    /// Подвести к тому, на что сейчас указывает пользователь: `Some(текст)` —
    /// наведение или Tab на ссылке, `None` — ссылки нет. `moved` — это новое
    /// движение мыши: оно перезапускает 80 мс, пока пузырь ещё не виден.
    pub(crate) fn set_target(&mut self, target: Option<&str>, now: f64, moved: bool) {
        let alpha = self.alpha(now);
        let heading_in = match self.fade {
            Some(f) => f.to >= 1.0,
            None => self.level >= 1.0,
        };
        match target {
            Some(text) => {
                self.hide_at = None;
                if alpha > 0.0 || heading_in {
                    // Уже виден (или появляется): текст меняется мгновенно;
                    // уходящий пузырь возвращается к полной непрозрачности.
                    self.show_at = None;
                    if self.text != text {
                        self.text = text.to_owned();
                    }
                    if !heading_in {
                        self.fade = Some(Fade {
                            from: alpha,
                            to: 1.0,
                            start: now,
                            dur: self.fade_ms(FADE_IN_MS) * f64::from(1.0 - alpha),
                        });
                    }
                } else {
                    let changed = self.text != text;
                    if changed {
                        self.text = text.to_owned();
                    }
                    if changed || moved || self.show_at.is_none() {
                        self.show_at = Some(now + SHOW_DELAY_MS);
                    }
                }
            }
            None => {
                self.show_at = None;
                let leaving = self.fade.is_some_and(|f| f.to <= 0.0);
                if alpha > 0.0 || heading_in {
                    if !leaving && self.hide_at.is_none() {
                        self.hide_at = Some(now + HIDE_DELAY_MS);
                    }
                } else if self.fade.is_none() {
                    self.text.clear();
                    self.min_width = 0.0;
                }
            }
        }
    }

    /// Продвинуть часы до `now`.
    pub(crate) fn tick(&mut self, now: f64) -> Tick {
        let mut repaint = false;
        if let Some(t) = self.show_at.filter(|&t| now >= t) {
            self.show_at = None;
            self.fade = Some(Fade { from: 0.0, to: 1.0, start: t, dur: self.fade_ms(FADE_IN_MS) });
            repaint = true;
        }
        if let Some(t) = self.hide_at.filter(|&t| now >= t) {
            self.hide_at = None;
            let a = self.alpha(t);
            self.fade = Some(Fade { from: a, to: 0.0, start: t, dur: self.fade_ms(FADE_OUT_MS) * f64::from(a) });
            repaint = true;
        }
        if let Some(f) = self.fade.filter(|f| f.done(now)) {
            self.level = f.to;
            self.fade = None;
            if f.to <= 0.0 {
                self.text.clear();
                self.min_width = 0.0;
            }
            repaint = true;
        }
        let mut wake = [self.show_at, self.hide_at].into_iter().flatten().reduce(f64::min);
        if self.fade.is_some() {
            repaint = true;
            let frame = now + FRAME_MS;
            wake = Some(wake.map_or(frame, |w| w.min(frame)));
        }
        Tick { repaint, wake }
    }
}

/// Сократить `text` до `max_w`: схема и хост остаются целиком, из пути уходит
/// середина (`…`), хвост — имя файла, запрос, якорь — виден. Если не помещается
/// даже хост, от пути остаётся одно `…`: хост не режется никогда.
pub(crate) fn elide_middle(text: &str, max_w: f32, char_w: &dyn Fn(char) -> f32) -> String {
    let width = |s: &str| s.chars().map(char_w).sum::<f32>();
    if width(text) <= max_w {
        return text.to_owned();
    }
    let prefix_len = text.find("://").map_or(0, |i| {
        let auth = i + 3;
        text[auth..].find(['/', '?', '#']).map_or(text.len(), |p| auth + p)
    });
    let (prefix, rest) = text.split_at(prefix_len);
    let ellipsis_w = char_w(ELLIPSIS);
    let budget = max_w - width(prefix) - ellipsis_w;
    if rest.is_empty() {
        return text.to_owned();
    }
    if budget <= 0.0 {
        return format!("{prefix}{ELLIPSIS}");
    }
    // Хвост — от последнего «/» (без учёта завершающих), там имя файла.
    let tail_start = rest.trim_end_matches('/').rfind('/').unwrap_or(0);
    let (head, tail) = rest.split_at(tail_start);
    let tail_w = width(tail);
    let tail_budget = if tail_w <= TAIL_SHARE * budget { tail_w } else { TAIL_SHARE * budget };
    let head_budget = budget - tail_budget;
    let mut kept_head = String::new();
    let mut used = 0.0;
    for c in head.chars() {
        let w = char_w(c);
        if used + w > head_budget {
            break;
        }
        used += w;
        kept_head.push(c);
    }
    let mut kept_tail: Vec<char> = Vec::new();
    let mut used = 0.0;
    for c in tail.chars().rev() {
        let w = char_w(c);
        if used + w > tail_budget {
            break;
        }
        used += w;
        kept_tail.push(c);
    }
    kept_tail.reverse();
    let tail: String = kept_tail.into_iter().collect();
    format!("{prefix}{kept_head}{ELLIPSIS}{tail}")
}

/// Рамка и текст пузыря.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BubbleLayout {
    pub(crate) rect: Rect,
    /// Текст после [`elide_middle`].
    pub(crate) text: String,
    /// Пузырь стоит у правого края (уступил курсору): скруглён левый верхний угол.
    pub(crate) at_right: bool,
}

fn near(r: Rect, p: (f32, f32)) -> bool {
    p.0 >= r.x - MOUSE_PAD && p.0 < r.right() + MOUSE_PAD && p.1 >= r.y - MOUSE_PAD && p.1 < r.bottom() + MOUSE_PAD
}

/// Рассчитать пузырь для `text` на странице `host` (окно, CSS px).
/// `bottom_inset` — высота горизонтальной полосы прокрутки, `pointer` —
/// курсор в тех же координатах, `min_width` — [`StatusBubble::min_width`].
///
/// Курсор ближе [`MOUSE_PAD`]: пузырь съезжает вниз на свою высоту, если ниже
/// есть место, иначе перескакивает в противоположный угол (LINK-10).
pub(crate) fn layout(
    text: &str,
    host: Rect,
    bottom_inset: f32,
    pointer: Option<(f32, f32)>,
    min_width: f32,
    char_w: &dyn Fn(char) -> f32,
) -> BubbleLayout {
    let max_w = (host.width * MAX_WIDTH_FRACTION).max(0.0);
    let elided = elide_middle(text, (max_w - 2.0 * PAD_X).max(0.0), char_w);
    let text_w: f32 = elided.chars().map(char_w).sum();
    // Хост не режется: пузырь шире половины, но не шире страницы.
    let w = (text_w + 2.0 * PAD_X).max(min_width).min(host.width);
    let h = FONT_SIZE + 2.0 * PAD_Y;
    let y = host.bottom() - bottom_inset - h;
    let left = Rect::new(host.x, y, w, h);
    let mut out = BubbleLayout { rect: left, text: elided, at_right: false };
    let Some(p) = pointer.filter(|&p| near(left, p)) else { return out };
    if host.bottom() - left.bottom() >= h {
        out.rect = Rect::new(left.x, left.y + h, w, h);
        return out;
    }
    let right = Rect::new(host.right() - w, y, w, h);
    if !near(right, p) {
        out.rect = right;
        out.at_right = true;
    }
    out
}

fn scaled(c: Color, alpha: f32) -> Color {
    Color { a: (f32::from(c.a) * alpha.clamp(0.0, 1.0)).round() as u8, ..c }
}

/// Команды отрисовки пузыря с непрозрачностью `alpha`.
pub(crate) fn build_overlay(lay: &BubbleLayout, alpha: f32, pal: &Palette) -> DisplayList {
    let r = lay.rect;
    let radius = CORNER_RADIUS.min(r.height / 2.0);
    let radii = if lay.at_right {
        CornerRadii { tl: radius, tl_y: radius, ..CornerRadii::default() }
    } else {
        CornerRadii { tr: radius, tr_y: radius, ..CornerRadii::default() }
    };
    vec![
        DisplayCommand::FillRoundedRect { rect: r, color: scaled(pal.overlay_bg, alpha), radii },
        DisplayCommand::DrawBorder {
            rect: r,
            widths: [1.0; 4],
            colors: [scaled(pal.overlay_border, alpha); 4],
            styles: [BorderStyle::Solid; 4],
            radii,
        },
        DisplayCommand::PushClipRect { rect: r },
        DisplayCommand::DrawText {
            font_stretch: FontStretch::NORMAL,
            rect: Rect::new(r.x + PAD_X, r.y + PAD_Y, (r.width - 2.0 * PAD_X).max(0.0), FONT_SIZE),
            text: lay.text.clone(),
            font_size: FONT_SIZE,
            color: scaled(pal.text, alpha),
            // Тот же Inter, которым меряется ширина (`lumen/status_bubble.rs`).
            font_family: vec!["Inter".to_string()],
            font_weight: FontWeight::new(400),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: None,
        },
        DisplayCommand::PopClip,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ширина символа 1 для всех — длину строки можно считать в уме.
    fn unit(_: char) -> f32 {
        1.0
    }

    #[test]
    fn appears_after_80ms_and_fades_in_over_120() {
        let mut b = StatusBubble::default();
        b.set_target(Some("https://a.ru/"), 0.0, true);
        assert_eq!(b.visible(0.0), None, "до 80 мс пузыря нет");
        let t = b.tick(79.0);
        assert_eq!(b.visible(79.0), None);
        assert_eq!(t.wake, Some(80.0), "будить ровно на показе");
        b.tick(80.0);
        assert!(b.visible(80.0).is_none(), "в момент старта fade прозрачность 0");
        let (_, a) = b.visible(140.0).expect("виден на полпути");
        assert!((a - 0.5).abs() < 1e-4, "{a}");
        b.tick(200.0);
        assert_eq!(b.visible(200.0), Some(("https://a.ru/", 1.0)));
        assert_eq!(b.tick(300.0).wake, None, "устоявшийся пузырь не будит цикл");
    }

    #[test]
    fn reduced_motion_keeps_the_delays_but_drops_the_fades() {
        let mut b = StatusBubble::default();
        b.set_reduced_motion(true);
        b.set_target(Some("https://a.ru/"), 0.0, true);
        b.tick(79.0);
        assert_eq!(b.visible(79.0), None, "задержка 80 мс остаётся");
        b.tick(80.0);
        assert_eq!(b.visible(80.0).map(|v| v.1), Some(1.0), "без fade-in сразу полная непрозрачность");
        b.set_target(None, 1000.0, true);
        b.tick(1249.0);
        assert!(b.visible(1249.0).is_some());
        b.tick(1250.0);
        assert_eq!(b.visible(1250.0), None, "без fade-out пузырь пропадает сразу");
        assert!(b.is_idle());
    }

    #[test]
    fn every_mouse_move_restarts_the_show_timer() {
        let mut b = StatusBubble::default();
        b.set_target(Some("https://a.ru/"), 0.0, true);
        b.set_target(Some("https://a.ru/"), 60.0, true);
        b.tick(100.0);
        assert!(b.visible(150.0).is_none(), "80 мс отсчитываются от последнего движения");
        b.tick(140.0);
        assert!(b.visible(200.0).is_some());
    }

    #[test]
    fn a_non_move_resync_does_not_restart_the_timer() {
        let mut b = StatusBubble::default();
        b.set_target(Some("https://a.ru/"), 0.0, true);
        b.set_target(Some("https://a.ru/"), 60.0, false);
        b.tick(80.0);
        assert!(b.visible(140.0).is_some());
    }

    #[test]
    fn hides_after_250ms_then_fades_out_over_200() {
        let mut b = StatusBubble::default();
        b.set_target(Some("https://a.ru/"), 0.0, true);
        b.tick(80.0);
        b.tick(200.0);
        b.set_target(None, 1000.0, true);
        b.tick(1249.0);
        assert_eq!(b.visible(1249.0).map(|v| v.1), Some(1.0), "до 250 мс не гаснет");
        b.tick(1250.0);
        let (_, a) = b.visible(1350.0).expect("ещё виден");
        assert!((a - 0.5).abs() < 1e-4, "{a}");
        b.tick(1450.0);
        assert_eq!(b.visible(1450.0), None);
        assert!(b.is_idle(), "после ухода часы пусты");
    }

    #[test]
    fn leaving_and_returning_before_the_delay_keeps_it_solid() {
        let mut b = StatusBubble::default();
        b.set_target(Some("https://a.ru/"), 0.0, true);
        b.tick(80.0);
        b.tick(200.0);
        b.set_target(None, 1000.0, true);
        b.set_target(Some("https://a.ru/"), 1100.0, true);
        b.tick(1300.0);
        assert_eq!(b.visible(1300.0).map(|v| v.1), Some(1.0));
    }

    #[test]
    fn switching_links_swaps_text_without_fade() {
        let mut b = StatusBubble::default();
        b.set_target(Some("https://a.ru/"), 0.0, true);
        b.tick(80.0);
        b.tick(200.0);
        b.set_target(Some("https://b.ru/"), 500.0, true);
        assert_eq!(b.visible(500.0), Some(("https://b.ru/", 1.0)), "текст сменился сразу, без fade");
    }

    #[test]
    fn switching_links_during_fade_out_brings_it_back() {
        let mut b = StatusBubble::default();
        b.set_target(Some("https://a.ru/"), 0.0, true);
        b.tick(80.0);
        b.tick(200.0);
        b.set_target(None, 1000.0, true);
        b.tick(1350.0);
        let (_, mid) = b.visible(1350.0).expect("в середине ухода");
        b.set_target(Some("https://b.ru/"), 1350.0, true);
        let (t, a) = b.visible(1350.0).expect("виден");
        assert_eq!(t, "https://b.ru/");
        assert!((a - mid).abs() < 1e-4, "возврат стартует с текущей прозрачности");
        b.tick(1500.0);
        assert_eq!(b.visible(1500.0).map(|v| v.1), Some(1.0));
    }

    #[test]
    fn leaving_before_it_shows_never_shows() {
        let mut b = StatusBubble::default();
        b.set_target(Some("https://a.ru/"), 0.0, true);
        b.set_target(None, 40.0, true);
        b.tick(500.0);
        assert_eq!(b.visible(500.0), None);
        assert!(b.is_idle());
    }

    #[test]
    fn width_only_grows_while_visible() {
        let mut b = StatusBubble::default();
        b.note_width(100.0);
        b.note_width(60.0);
        assert_eq!(b.min_width(), 100.0);
    }

    #[test]
    fn short_text_is_not_elided() {
        assert_eq!(elide_middle("https://a.ru/x", 100.0, &unit), "https://a.ru/x");
    }

    #[test]
    fn elision_keeps_host_and_file_name_and_drops_the_middle() {
        let url = "https://example.org/aaaa/bbbb/cccc/dddd/report-final.pdf?v=2#p3";
        let out = elide_middle(url, 40.0, &unit);
        assert!(out.starts_with("https://example.org/"), "{out}");
        assert!(out.contains('…'), "{out}");
        assert!(out.ends_with("pdf?v=2#p3"), "{out}");
        assert!(out.chars().count() <= 40, "{out} ({})", out.chars().count());
    }

    #[test]
    fn host_is_never_cut_even_when_alone_it_overflows() {
        let url = "https://very-long-subdomain.example-hosting.org/path/to/file.html";
        let out = elide_middle(url, 10.0, &unit);
        assert_eq!(out, "https://very-long-subdomain.example-hosting.org…");
    }

    #[test]
    fn elision_with_no_scheme_cuts_the_middle() {
        let out = elide_middle("mailto:someone.with.a.long.address@example.org", 20.0, &unit);
        assert!(out.contains('…') && out.chars().count() <= 20, "{out}");
    }

    #[test]
    fn query_less_url_with_long_tail_keeps_some_head_and_tail() {
        let url = "https://h.ru/a/b/c/d/e/f/g/h/i/j/k/l/m/nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn";
        let out = elide_middle(url, 30.0, &unit);
        assert!(out.starts_with("https://h.ru/"), "{out}");
        assert!(out.ends_with('n'), "{out}");
        assert!(out.chars().count() <= 30, "{out}");
    }

    fn host() -> Rect {
        Rect::new(200.0, 100.0, 1000.0, 700.0)
    }

    #[test]
    fn sits_bottom_left_above_the_scrollbar_and_within_half_width() {
        let lay = layout("https://a.ru/x", host(), 8.0, None, 0.0, &unit);
        assert_eq!(lay.rect.x, 200.0);
        assert!((lay.rect.bottom() - (800.0 - 8.0)).abs() < 1e-3);
        assert!(lay.rect.width <= 500.0 + 1e-3);
        let long = "https://a.ru/".to_owned() + &"p".repeat(2000);
        let lay = layout(&long, host(), 0.0, None, 0.0, &unit);
        assert!(lay.rect.width <= 500.0 + 1e-3, "{}", lay.rect.width);
    }

    #[test]
    fn width_does_not_shrink_between_links() {
        let wide = layout("https://a.ru/a-rather-long-path", host(), 0.0, None, 0.0, &unit);
        let next = layout("https://a.ru/x", host(), 0.0, None, wide.rect.width, &unit);
        assert_eq!(next.rect.width, wide.rect.width);
    }

    #[test]
    fn pointer_near_the_bubble_pushes_it_to_the_opposite_corner() {
        let calm = layout("https://a.ru/x", host(), 0.0, None, 0.0, &unit);
        let p = (calm.rect.x + 5.0, calm.rect.y - 10.0);
        let lay = layout("https://a.ru/x", host(), 0.0, Some(p), 0.0, &unit);
        assert!(lay.at_right, "внизу места нет — угол напротив");
        assert!(!near(lay.rect, p), "рамка дальше {MOUSE_PAD} px от курсора");
        assert!((lay.rect.right() - host().right()).abs() < 1e-3);
    }

    #[test]
    fn pointer_far_from_the_bubble_changes_nothing() {
        let calm = layout("https://a.ru/x", host(), 0.0, None, 0.0, &unit);
        let lay = layout("https://a.ru/x", host(), 0.0, Some((600.0, 300.0)), 0.0, &unit);
        assert_eq!(lay, calm);
    }

    #[test]
    fn room_below_slides_it_down_instead_of_flipping() {
        let lay = layout("https://a.ru/x", host(), 40.0, Some((205.0, 770.0)), 0.0, &unit);
        assert!(!lay.at_right);
        assert!(lay.rect.y > 800.0 - 40.0 - lay.rect.height - 0.5);
    }

    #[test]
    fn overlay_fades_with_alpha() {
        let lay = layout("https://a.ru/x", host(), 0.0, None, 0.0, &unit);
        let pal = Palette::DARK;
        let full = build_overlay(&lay, 1.0, &pal);
        let half = build_overlay(&lay, 0.5, &pal);
        let alpha = |dl: &DisplayList| match &dl[0] {
            DisplayCommand::FillRoundedRect { color, .. } => color.a,
            other => panic!("первой идёт заливка, а не {other:?}"),
        };
        assert_eq!(alpha(&full), pal.overlay_bg.a);
        assert_eq!(alpha(&half), (f32::from(pal.overlay_bg.a) * 0.5).round() as u8);
        assert!(full.iter().any(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "https://a.ru/x")));
    }
}
