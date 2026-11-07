//! BUG-1719: UA-панель управления `<video controls>` / `<audio controls>`
//! (HTML LS §4.8.11.13 «User interface»: play/pause, время, seek, mute).
//!
//! Подход — тот же, что у полосы прокрутки: панель строится шеллом на каждом
//! `RedrawRequested` и prepend-ится к `overlay_buf` (рисуется над страницей).
//! Display list страницы кэшируется между кадрами, а состояние воспроизведения
//! (`VideoGifStore::playback`) меняется каждый тик, поэтому paint о панели не
//! знает: команды (`FillRoundedRect` / `DrawSvgFill` / `DrawText` / клипы)
//! собираются здесь из live-состояния.
//!
//! Координаты: бокс живёт в документных координатах; панель выводится в
//! оконных CSS px — накопленный скролл overflow-контейнеров
//! (`LayoutBox::scroll_x/scroll_y` предков) и страничный скролл вычитаются,
//! оффсет страницы (`page_offset()`) прибавляется. Зоны hit-test (`MediaHit`)
//! возвращаются в тех же оконных координатах, что получают обработчики мыши.
//!
//! Срез 2: клики по зонам (`handle_media_panel_click`) мутируют тот же store,
//! что и нативы `__lumen_video_play/pause` (семантика повторена 1:1), и будят
//! тик-цикл через `note_playback_change`.
//!
//! Ограничения среза: панели не следуют CSS-трансформам бокса (кроме
//! `position: fixed`, он учтён); `<video>` внутри `<iframe>` — своё дерево
//! layout, см. BUG-1720; у `<audio>` сессии воспроизведения ещё нет (Phase 0
//! аудио-декода), панель рисуется в paused-состоянии; hover-события
//! (mousemove/pointermove над панелью) доставляются странице — гасится
//! только press.

use crate::*;
use lumen_core::geom::Rect;
use lumen_dom::Document;
use lumen_layout::{BoxKind, Color, FontStretch, FontStyle, FontWeight, Position};
use lumen_paint::DisplayCommand;

/// Высота панели, CSS px. UA-дефолт `<audio controls>` — 40px
/// (layout резервирует ровно столько); у видео полоса прижимается к низу
/// бокса и не бывает выше его самого.
const PANEL_H: f32 = 40.0;
const BTN: f32 = 32.0;
const BTN_INSET: f32 = 4.0;
const SEEK_H: f32 = 4.0;
const TIME_SIZE: f32 = 12.0;
const GAP: f32 = 8.0;
/// Ниже этой ширины полосы текст времени прячется, остаётся play + seek + mute.
const TIME_MIN_BAR_W: f32 = 190.0;
/// Ниже этой ширины панель не рисуется вовсе.
const MIN_BAR_W: f32 = 72.0;

const BAR_BG: Color = Color { r: 24, g: 24, b: 24, a: 200 };
const FG: Color = Color { r: 255, g: 255, b: 255, a: 235 };
const TRACK: Color = Color { r: 255, g: 255, b: 255, a: 70 };

/// Зона панели, в которую попал клик.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaZone {
    PlayPause,
    Mute,
    /// Клик по seek-треку: прыжок на долю `x / width` длительности
    /// (drag-перемотка — отдельный срез).
    Seek,
}

/// Прямоугольник зоны в оконных CSS px — живёт между кадрами в
/// `Lumen::media_panel_hits` и опрашивается кликом до диспетчеризации в DOM.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MediaHit {
    pub(crate) nid: u32,
    pub(crate) zone: MediaZone,
    pub(crate) rect: Rect,
}

/// Состояние воспроизведения, достаточное для отрисовки панели.
#[derive(Debug, Clone, Copy)]
struct PanelState {
    paused: bool,
    cur_ms: u64,
    dur_ms: u64,
    muted: bool,
}

/// Геометрия одной панели в оконных координатах.
#[derive(Debug, Clone)]
struct PanelUi {
    bar: Rect,
    play_btn: Rect,
    mute_btn: Rect,
    /// Трек seek-полосы (заполненная часть рисуется внутри него).
    seek: Rect,
    /// Прямоугольник текста времени и сама строка; `None` на узких полосах.
    time: Option<(Rect, String)>,
}

/// m:ss, а с часом h:mm:ss.
fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

fn rect_contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Пересечение с вьюпортом; `None`, если панель целиком вне экрана.
fn clip_to_viewport(r: Rect, vp: Rect) -> Option<Rect> {
    let x = r.x.max(vp.x);
    let y = r.y.max(vp.y);
    let right = r.right().min(vp.right());
    let bottom = r.bottom().min(vp.bottom());
    if right <= x || bottom <= y {
        return None;
    }
    Some(Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

fn center(r: Rect) -> [f32; 2] {
    [r.x + r.width * 0.5, r.y + r.height * 0.5]
}

/// Геометрия зоны панели внутри полосы `bar`. `None` — бокс слишком узкий.
fn panel_ui(bar: Rect, cur_ms: u64, dur_ms: u64) -> Option<PanelUi> {
    if bar.width < MIN_BAR_W {
        return None;
    }
    let cy = bar.y + bar.height * 0.5;
    let play_btn = Rect {
        x: bar.x + BTN_INSET,
        y: cy - BTN * 0.5,
        width: BTN,
        height: BTN,
    };
    let mute_btn = Rect {
        x: bar.right() - BTN_INSET - BTN,
        y: cy - BTN * 0.5,
        width: BTN,
        height: BTN,
    };
    let time = (bar.width >= TIME_MIN_BAR_W).then(|| {
        let label = format!("{} / {}", fmt_time(cur_ms), fmt_time(dur_ms));
        // ~6.2 px на символ при 12px; хватает и для «1:23:45 / 1:23:45» с
        // небольшим запасом справа.
        let r = Rect {
            x: play_btn.right() + GAP,
            y: cy - 8.0,
            width: 78.0,
            height: 16.0,
        };
        (r, label)
    });
    let seek_x = match &time {
        Some((r, _)) => r.right() + GAP,
        None => play_btn.right() + GAP,
    };
    let seek = Rect {
        x: seek_x,
        y: cy - SEEK_H * 0.5,
        width: (mute_btn.x - GAP - seek_x).max(0.0),
        height: SEEK_H,
    };
    if seek.width < 24.0 {
        return None;
    }
    Some(PanelUi {
        bar,
        play_btn,
        mute_btn,
        seek,
        time,
    })
}

/// Треугольник play (⏵) вокруг центра кнопки.
fn play_icon(c: [f32; 2]) -> Vec<Vec<[f32; 2]>> {
    let [cx, cy] = c;
    vec![vec![
        [cx - 5.0, cy - 7.0],
        [cx - 5.0, cy + 7.0],
        [cx + 7.0, cy],
    ]]
}

/// Две вертикальные скобки pause (⏸).
fn pause_icon(c: [f32; 2]) -> Vec<Vec<[f32; 2]>> {
    let [cx, cy] = c;
    let bar = |dx: f32| vec![[cx + dx, cy - 7.0], [cx + dx + 3.5, cy - 7.0], [cx + dx + 3.5, cy + 7.0], [cx + dx, cy + 7.0]];
    vec![bar(-6.5), bar(3.0)]
}

/// Динамик; `muted` добавляет косой крест поверх.
fn speaker_icon(c: [f32; 2], muted: bool) -> Vec<Vec<[f32; 2]>> {
    let [cx, cy] = c;
    // Прямоугольная часть + конус — один DrawSvgFill с двумя контурами.
    let mut contours = vec![
        vec![
            [cx - 9.0, cy - 3.0],
            [cx - 4.5, cy - 3.0],
            [cx - 4.5, cy + 3.0],
            [cx - 9.0, cy + 3.0],
        ],
        vec![
            [cx - 4.5, cy - 3.0],
            [cx - 4.5, cy + 3.0],
            [cx + 1.5, cy + 7.0],
            [cx + 1.5, cy - 7.0],
        ],
    ];
    if muted {
        // Косой крест тонкими четырёхугольниками.
        let slash = |(x0, y0): (f32, f32), (x1, y1): (f32, f32)| {
            let (nx, ny) = (-(y1 - y0), x1 - x0);
            let len = (nx * nx + ny * ny).sqrt().max(0.001);
            let (ux, uy) = (nx / len * 1.4, ny / len * 1.4);
            vec![
                [x0 + ux, y0 + uy],
                [x1 + ux, y1 + uy],
                [x1 - ux, y1 - uy],
                [x0 - ux, y0 - uy],
            ]
        };
        contours.push(slash((cx + 4.0, cy - 4.0), (cx + 10.0, cy + 4.0)));
        contours.push(slash((cx + 10.0, cy - 4.0), (cx + 4.0, cy + 4.0)));
    }
    contours
}

fn emit_panel(out: &mut Vec<DisplayCommand>, ui: &PanelUi, st: &PanelState) {
    out.push(DisplayCommand::FillRoundedRect {
        rect: ui.bar,
        color: BAR_BG,
        radii: Default::default(),
    });
    // Seek-полоса: трек + заполненная часть.
    out.push(DisplayCommand::FillRoundedRect {
        rect: ui.seek,
        color: TRACK,
        radii: Default::default(),
    });
    let frac = if st.dur_ms > 0 {
        (st.cur_ms as f32 / st.dur_ms as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if frac > 0.0 {
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect {
                width: ui.seek.width * frac,
                ..ui.seek
            },
            color: FG,
            radii: Default::default(),
        });
    }
    // Иконка play/pause по фактическому состоянию.
    let contours = if st.paused {
        play_icon(center(ui.play_btn))
    } else {
        pause_icon(center(ui.play_btn))
    };
    out.push(DisplayCommand::DrawSvgFill {
        contours,
        color: FG,
    });
    // Mute-иконка по фактическому `muted`.
    out.push(DisplayCommand::DrawSvgFill {
        contours: speaker_icon(center(ui.mute_btn), st.muted),
        color: FG,
    });
    // Время: текст шейпит сам бэкенд рендера.
    if let Some((rect, text)) = &ui.time {
        out.push(DisplayCommand::DrawText {
            rect: *rect,
            text: text.clone(),
            font_size: TIME_SIZE,
            color: FG,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_stretch: FontStretch::NORMAL,
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: None,
        });
    }
}

impl Lumen {
    /// Собирает команды панелей управления для всех видимых
    /// `<video controls>` / `<audio controls>` текущего layout-дерева
    /// (в документном порядке) и возвращает зоны hit-test в оконных
    /// координатах. Команды дописываются в `out` в оконных координатах.
    pub(crate) fn build_media_controls_overlay(&self, out: &mut DisplayList) -> Vec<MediaHit> {
        let hits = Vec::new();
        let Some(lb) = self.layout_box.as_ref() else {
            return hits;
        };
        let Some(ls) = self.layout_source.as_ref() else {
            return hits;
        };
        let doc = ls.document.lock().unwrap_or_else(|e| e.into_inner());
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: self.viewport_width_css(),
            height: self.viewport_height_css(),
        };
        let mut ctx = PanelCtx {
            doc: &doc,
            store: &self.video_gif_store,
            clock_ms: self.media_clock_ms(),
            scroll: (self.scroll_x, self.scroll_y),
            win_off: self.page_offset(),
            viewport,
            hits: Vec::new(),
        };
        // Корневой бокс не прибавляет свой scroll: страничный скролл живёт
        // только в шелле (`self.scroll_x/y`) и учтён в `ctx.scroll`.
        walk_media_boxes(lb, true, (0.0, 0.0), &mut ctx, out);
        ctx.hits
    }

    /// Клик по зоне UA-панели медиа. Возвращает `true`, если точка попала в
    /// панель — тогда диспетчеризация в DOM не нужна. Мутации playback-store
    /// повторяют нативы `__lumen_video_play` / `__lumen_video_pause` 1:1
    /// ( freeze при паузе, ре-якорь `play_epoch_ms` при старте), чтобы
    /// JS-наблюдатели (`video.paused`, `currentTime`) увидели то же самое.
    pub(crate) fn handle_media_panel_click(&mut self, x_css: f32, y_css: f32) -> bool {
        let Some(hit) = self
            .media_panel_hits
            .iter()
            .find(|h| rect_contains(h.rect, x_css, y_css))
            .copied()
        else {
            return false;
        };
        let nid = hit.nid;
        let now = self.media_clock_ms();
        let store = std::sync::Arc::clone(&self.video_gif_store);
        match hit.zone {
            MediaZone::PlayPause => {
                let rate = store
                    .playback_rates
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&nid)
                    .copied()
                    .unwrap_or(1.0);
                let mut pb = store.playback.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(e) = pb.get_mut(&nid) {
                    if e.paused {
                        e.play_epoch_ms = Some(now);
                        e.paused = false;
                    } else {
                        e.freeze(now, rate);
                        e.paused = true;
                    }
                    drop(pb);
                    store.note_playback_change(now as f64);
                    self.request_redraw();
                }
            }
            MediaZone::Mute => {
                let mut levels = store.audio_levels.lock().unwrap_or_else(|e| e.into_inner());
                let entry = levels.entry(nid).or_insert((1.0, false));
                entry.1 = !entry.1;
                drop(levels);
                self.request_redraw();
            }
            MediaZone::Seek => {
                // Доля клика по треку → цель в мс; семантика `__lumen_video_seek`:
                // `position_ms` переписывается и на паузе (тик передекодирует кадр),
                // на игре — ре-якорь эпохи под текущий rate.
                let dur = store
                    .playback
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&nid)
                    .map_or(0, |e| e.cycle_ms);
                if dur == 0 {
                    return true;
                }
                let frac = (((x_css - hit.rect.x) / hit.rect.width) as f64).clamp(0.0, 1.0);
                let mut pb = store.playback.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(e) = pb.get_mut(&nid) {
                    e.position_ms = (frac * dur as f64) as u64;
                    if !e.paused {
                        e.play_epoch_ms = Some(now);
                    }
                    drop(pb);
                    store.note_playback_change(now as f64);
                    self.request_redraw();
                }
            }
        }
        true
    }
}

/// Контекст одного прохода по layout-дереву.
struct PanelCtx<'a> {
    doc: &'a std::sync::MutexGuard<'a, Document>,
    store: &'a std::sync::Arc<lumen_js::VideoGifStore>,
    clock_ms: u64,
    scroll: (f32, f32),
    win_off: (f32, f32),
    viewport: Rect,
    hits: Vec<MediaHit>,
}

/// Документный rect бокса → оконный. `acc` — накопленный скролл
/// overflow-контейнеров-предков; `fixed`-боксы его игнорируют.
fn box_window_rect(b: &lumen_layout::LayoutBox, acc: (f32, f32), ctx: &PanelCtx, fixed: bool) -> Rect {
    let (sx, sy) = if fixed { (0.0, 0.0) } else { (acc.0 + ctx.scroll.0, acc.1 + ctx.scroll.1) };
    Rect {
        x: b.rect.x - sx + ctx.win_off.0,
        y: b.rect.y - sy + ctx.win_off.1,
        width: b.rect.width,
        height: b.rect.height,
    }
}

fn walk_media_boxes(
    b: &lumen_layout::LayoutBox,
    is_root: bool,
    acc: (f32, f32),
    ctx: &mut PanelCtx,
    out: &mut DisplayList,
) {
    let kind = &b.kind;
    let is_media = matches!(kind, BoxKind::Video { .. } | BoxKind::Audio { .. });
    if is_media && b.rect.width > 0.0 && b.rect.height > 0.0 {
        let fixed = b.style.position == Position::Fixed;
        let win = box_window_rect(b, acc, ctx, fixed);
        // Панель видна, только если сам бокс пересекает вьюпорт.
        if clip_to_viewport(win, ctx.viewport).is_some() {
            let has_controls = match kind {
                BoxKind::Video { .. } => ctx.doc.get(b.node).get_attr("controls").is_some(),
                BoxKind::Audio { controls, .. } => *controls,
                _ => false,
            };
            if has_controls {
                let nid = b.node.index() as u32;
                let rate = ctx
                    .store
                    .playback_rates
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&nid)
                    .copied()
                    .unwrap_or(1.0);
                let (paused, cur_ms, dur_ms) = {
                    let pb = ctx.store.playback.lock().unwrap_or_else(|e| e.into_inner());
                    pb.get(&nid)
                        .map_or((true, 0, 0), |s| (s.paused, s.current_ms(ctx.clock_ms, rate), s.cycle_ms))
                };
                let muted = ctx
                    .store
                    .audio_levels
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&nid)
                    .is_some_and(|(_, m)| *m);
                let st = PanelState {
                    paused,
                    cur_ms,
                    dur_ms,
                    muted,
                };
                let bar_h = PANEL_H.min(win.height);
                let bar = Rect {
                    y: win.y + win.height - bar_h,
                    height: bar_h,
                    ..win
                };
                if let Some(ui) = panel_ui(bar, st.cur_ms, st.dur_ms) {
                    // Частично ушедший за вьюпорт бокс клипим, чтобы панель не
                    // залезала на chrome/за край окна.
                    let clip = clip_to_viewport(ui.bar, ctx.viewport).filter(|c| *c != ui.bar);
                    let clipped = clip.is_some();
                    if let Some(c) = clip {
                        out.push(DisplayCommand::PushClipRect { rect: c });
                    }
                    emit_panel(out, &ui, &st);
                    if clipped {
                        out.push(DisplayCommand::PopClip);
                    }
                    ctx.hits.push(MediaHit {
                        nid,
                        zone: MediaZone::PlayPause,
                        rect: ui.play_btn,
                    });
                    ctx.hits.push(MediaHit {
                        nid,
                        zone: MediaZone::Mute,
                        rect: ui.mute_btn,
                    });
                    ctx.hits.push(MediaHit {
                        nid,
                        zone: MediaZone::Seek,
                        rect: ui.seek,
                    });
                }
            }
        }
    }
    // Скролл самого бокса действует на потомков; корень не прибавляем
    // (страничный скролл — в `ctx.scroll`).
    let child_acc = if is_root {
        acc
    } else {
        (acc.0 + b.scroll_x, acc.1 + b.scroll_y)
    };
    for child in &b.children {
        walk_media_boxes(child, false, child_acc, ctx, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    #[test]
    fn fmt_time_matches_mmss_and_hmmss() {
        assert_eq!(fmt_time(0), "0:00");
        assert_eq!(fmt_time(65_000), "1:05");
        assert_eq!(fmt_time(600_000), "10:00");
        assert_eq!(fmt_time(3_725_000), "1:02:05");
    }

    #[test]
    fn panel_ui_on_a_wide_bar_shows_all_zones() {
        let bar = r(0.0, 100.0, 400.0, PANEL_H);
        let ui = panel_ui(bar, 65_000, 600_000).expect("wide bar");
        assert!(ui.time.is_some(), "400px bar shows time text");
        assert_eq!(ui.play_btn.x, bar.x + BTN_INSET);
        assert!((ui.mute_btn.right() - bar.right() + BTN_INSET).abs() < 0.01);
        // Seek между текстом времени и mute, достаточно широкий.
        assert!(ui.seek.width > 100.0);
        assert!(ui.seek.x > ui.play_btn.right());
        assert!(ui.seek.right() < ui.mute_btn.x);
    }

    #[test]
    fn panel_ui_on_a_narrow_bar_hides_time() {
        let bar = r(0.0, 0.0, 120.0, PANEL_H);
        let ui = panel_ui(bar, 0, 0).expect("120px bar");
        assert!(ui.time.is_none());
        assert!(ui.seek.width >= 24.0);
    }

    #[test]
    fn panel_ui_rejects_a_degenerate_bar() {
        assert!(panel_ui(r(0.0, 0.0, 40.0, PANEL_H), 0, 0).is_none());
        assert!(panel_ui(r(0.0, 0.0, MIN_BAR_W, 4.0), 0, 0).is_none());
    }

    #[test]
    fn clip_to_viewport_cuts_and_rejects() {
        let vp = r(0.0, 0.0, 800.0, 600.0);
        // x: -10..290 → 0..290; y: 580..620 → 580..600 (нижний край).
        let half = clip_to_viewport(r(-10.0, 580.0, 300.0, 40.0), vp).expect("intersects");
        assert_eq!(half.x, 0.0);
        assert!((half.height - 20.0).abs() < 0.01);
        assert!(clip_to_viewport(r(0.0, 700.0, 100.0, 40.0), vp).is_none());
    }

    #[test]
    fn icons_are_non_empty_contours() {
        assert_eq!(play_icon([16.0, 16.0]).len(), 1);
        assert_eq!(pause_icon([16.0, 16.0]).len(), 2);
        assert_eq!(speaker_icon([16.0, 16.0], false).len(), 2);
        assert_eq!(speaker_icon([16.0, 16.0], true).len(), 4);
    }
}
