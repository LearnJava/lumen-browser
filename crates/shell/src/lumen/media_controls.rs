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
//! Срез 3: press на seek/volume начинает drag (`Lumen::media_panel_drag`) —
//! `CursorMoved` тянет значение по сохранённому треку, release завершает и
//! вместе с ним гасится, чтобы страница не увидела mouseup без mousedown.
//! Вolume-полоса мутирует `audio_levels` и через `__lumen_media_ui_volume`
//! доводит значение до JS-модели элемента (`volume`/`muted` живут в замыкании
//! шима, не в store); кнопка fullscreen переключает оконный fullscreen по
//! образцу Escape-ветки `keyboard.rs` и синхронизирует JS-состояние через
//! `_lumen_notify_fullscreen_enter/exit`.
//!
//! Ограничения среза: панели не следуют CSS-трансформам бокса (кроме
//! `position: fixed`, он учтён); `<video>` внутри `<iframe>` — своё дерево
//! layout, см. BUG-1720; fullscreen растягивает окно, но не сам элемент —
//! позиционирование `:fullscreen` (CSS Fullscreen §5 UA-правила + top-layer)
//! отдельная работа; hover-события над панелью доставляются странице —
//! гасятся press и движение при активном drag; play/pause (и события
//! play/pause/volumechange) идут через JS-хуки, поэтому в сборке без
//! `v8` кнопка play неактивна (seek/mute продолжают работать — их store-
//! мутации шелловые).

use crate::*;
use lumen_core::geom::Rect;
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
/// Ширина полосы громкости.
const VOL_W: f32 = 56.0;
/// Ниже этой ширины полосы текст времени прячется, остаётся play + seek + mute.
const TIME_MIN_BAR_W: f32 = 190.0;
/// Ниже этой ширины кнопка fullscreen не ставится (seek остаётся ≥ 24px).
const FS_MIN_BAR_W: f32 = 148.0;
/// Ниже этой ширины полоса громкости не ставится.
const VOL_MIN_BAR_W: f32 = 330.0;
/// Ниже этой ширины панель не рисуется вовсе.
const MIN_BAR_W: f32 = 72.0;
/// Вертикальный запас hit-зон seek/volume вокруг видимого трека (4px) —
/// по 4px-полосе попасть трудно, ловим в полосе ±10px.
const TRACK_HIT_PAD: f32 = 10.0;

const BAR_BG: Color = Color { r: 24, g: 24, b: 24, a: 200 };
const FG: Color = Color { r: 255, g: 255, b: 255, a: 235 };
const TRACK: Color = Color { r: 255, g: 255, b: 255, a: 70 };

/// Зона панели, в которую попал клик.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaZone {
    PlayPause,
    Mute,
    /// Seek-трек: прыжок/перетаскивание на долю `x / width` длительности.
    Seek,
    /// Полоса громкости: доля `x / width` → `audio_levels[nid].0`.
    Volume,
    Fullscreen,
}

impl MediaZone {
    /// Зоны-треки: press запоминает трек, `CursorMoved` тянет значение.
    fn is_track(self) -> bool {
        matches!(self, MediaZone::Seek | MediaZone::Volume)
    }
}

/// Прямоугольник зоны в оконных CSS px — живёт между кадрами в
/// `Lumen::media_panel_hits` и опрашивается кликом до диспетчеризации в DOM.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MediaHit {
    pub(crate) nid: u32,
    pub(crate) zone: MediaZone,
    pub(crate) rect: Rect,
    /// Бокс `<audio>`: provider-сторона элемента живёт в JS-модели
    /// (`audio_element.rs`), поэтому коммит seek на release уходит туда.
    pub(crate) audio: bool,
}

/// Активное удержание зоны панели между press и release. Для треков —
/// перетаскивание значения, для кнопок — только захват: парный release
/// странице не принадлежит и гасится.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MediaDrag {
    pub(crate) nid: u32,
    pub(crate) zone: MediaZone,
    /// Трек, по которому считается доля (оконные координаты на момент press).
    pub(crate) track: Rect,
    /// См. `MediaHit::audio`.
    pub(crate) audio: bool,
}

/// Состояние воспроизведения, достаточное для отрисовки панели.
#[derive(Debug, Clone, Copy)]
struct PanelState {
    paused: bool,
    cur_ms: u64,
    dur_ms: u64,
    muted: bool,
    /// `audio_levels[nid].0` (громкость 0..1); дефолт 1.0 — как у audio sink.
    volume: f32,
}

/// Геометрия одной панели в оконных координатах.
#[derive(Debug, Clone)]
struct PanelUi {
    bar: Rect,
    play_btn: Rect,
    mute_btn: Rect,
    fs_btn: Option<Rect>,
    /// Трек seek-полосы (заполненная часть рисуется внутри него).
    seek: Rect,
    /// Трек громкости; `None` на узких полосах.
    volume: Option<Rect>,
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

/// Доля позиции `x` по горизонтальному треку, 0..1.
fn frac_of(track: Rect, x: f32) -> f32 {
    if track.width <= 0.0 {
        return 0.0;
    }
    ((x - track.x) / track.width).clamp(0.0, 1.0)
}

/// Геометрия зоны панели внутри полосы `bar`. `None` — бокс слишком узкий.
fn panel_ui(bar: Rect, cur_ms: u64, dur_ms: u64) -> Option<PanelUi> {
    if bar.width < MIN_BAR_W {
        return None;
    }
    let cy = bar.y + bar.height * 0.5;
    let vbtn = |x: f32| Rect { x, y: cy - BTN * 0.5, width: BTN, height: BTN };
    let play_btn = vbtn(bar.x + BTN_INSET);
    // Правый край: fullscreen (самый внешний), затем mute левее.
    let fs_btn = (bar.width >= FS_MIN_BAR_W).then(|| vbtn(bar.right() - BTN_INSET - BTN));
    let mute_right = fs_btn.map_or(bar.right() - BTN_INSET, |b| b.x - BTN_INSET);
    let mute_btn = vbtn(mute_right - BTN);
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
    let volume = (bar.width >= VOL_MIN_BAR_W).then_some(Rect {
        x: mute_btn.x - GAP - VOL_W,
        y: cy - SEEK_H * 0.5,
        width: VOL_W,
        height: SEEK_H,
    });
    let seek_right = match volume {
        Some(ref v) => v.x - GAP,
        None => mute_btn.x - GAP,
    };
    let seek = Rect {
        x: seek_x,
        y: cy - SEEK_H * 0.5,
        width: (seek_right - seek_x).max(0.0),
        height: SEEK_H,
    };
    if seek.width < 24.0 {
        return None;
    }
    Some(PanelUi {
        bar,
        play_btn,
        mute_btn,
        fs_btn,
        seek,
        volume,
        time,
    })
}

/// Полоса ±TRACK_HIT_PAD вокруг видимого трека — hit-прямоугольник.
fn track_hit(track: Rect) -> Rect {
    Rect {
        y: track.y - TRACK_HIT_PAD,
        height: track.height + TRACK_HIT_PAD * 2.0,
        ..track
    }
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

/// Четыре угловые скобки fullscreen (⤢): каждая — два тонких прямоугольника
/// (горизонтальное и вертикальное плечо), 8 выпуклых контуров.
fn fullscreen_icon(c: [f32; 2]) -> Vec<Vec<[f32; 2]>> {
    let [cx, cy] = c;
    let s = 9.0; // полудлина скобки
    let t = 1.8; // толщина плеча
    let bar = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let (nx, ny) = (-(y1 - y0), x1 - x0);
        let len = (nx * nx + ny * ny).sqrt().max(0.001);
        let (ux, uy) = (nx / len * t, ny / len * t);
        vec![[x0 + ux, y0 + uy], [x1 + ux, y1 + uy], [x1 - ux, y1 - uy], [x0 - ux, y0 - uy]]
    };
    let (l, r, tp, bt) = (cx - s, cx + s, cy - s, cy + s);
    let a = 5.0; // длина плеча угла
    vec![
        bar(l, tp, l + a, tp),
        bar(l, tp, l, tp + a),
        bar(r, tp, r - a, tp),
        bar(r, tp, r, tp + a),
        bar(l, bt, l + a, bt),
        bar(l, bt, l, bt - a),
        bar(r, bt, r - a, bt),
        bar(r, bt, r, bt - a),
    ]
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
    // Полоса громкости: тот же трек + заполненная часть по `volume`.
    if let Some(vol) = ui.volume {
        out.push(DisplayCommand::FillRoundedRect {
            rect: vol,
            color: TRACK,
            radii: Default::default(),
        });
        if st.volume > 0.0 {
            out.push(DisplayCommand::FillRoundedRect {
                rect: Rect {
                    width: vol.width * st.volume.clamp(0.0, 1.0),
                    ..vol
                },
                color: FG,
                radii: Default::default(),
            });
        }
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
    if let Some(fs) = ui.fs_btn {
        out.push(DisplayCommand::DrawSvgFill {
            contours: fullscreen_icon(center(fs)),
            color: FG,
        });
    }
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
    ///
    /// BUG-1802: DOM на UI-потоке здесь не читается — `controls` обоих
    /// элементов едет в `BoxKind` (у `<video>` с этого фикса, у `<audio>`
    /// с BUG-1719): `document.lock()` на каждом кадре блокировал UI-поток
    /// на время JS-хода движкового потока (до 0.5–1.9 с на ebay).
    pub(crate) fn build_media_controls_overlay(&self, out: &mut DisplayList) -> Vec<MediaHit> {
        let hits = Vec::new();
        let Some(lb) = self.layout_box.as_ref() else {
            return hits;
        };
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: self.viewport_width_css(),
            height: self.viewport_height_css(),
        };
        let mut ctx = PanelCtx {
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
    /// панель — тогда диспетчеризация в DOM не нужна.
    ///
    /// Мутации идут двумя дорогами: seek/mute/volume пишет store прямо из
    /// шелла (семантика нативов `__lumen_video_seek` и т.п. — freeze при
    /// паузе, ре-якорь `play_epoch_ms` на игре), а play/pause и звуковые
    /// хуки проводятся через JS-модель элемента (`__lumen_media_ui_*`), та
    /// же дорога, что у скриптового `el.play()` — иначе события play/pause/
    /// volumechange до страницы не доходят или дублируются.
    ///
    /// Press дополнительно захватывается в `media_panel_drag`: треки тянут
    /// значение в `update_media_panel_drag`, у кнопок он держит только
    /// захват — парный release странице не принадлежит.
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
        self.media_panel_drag = Some(MediaDrag {
            nid,
            zone: hit.zone,
            track: hit.rect,
            audio: hit.audio,
        });
        match hit.zone {
            // Play/pause мутирует store ТОЛЬКО через JS-хук (`el.play()` /
            // `el.pause()` → нативы `__lumen_video_play/pause`): если шелл
            // перевернёт `paused` синхронно, шим на хуке увидит уже
            // переключённое состояние, `wasPaused` внутри `el.pause()`
            // станет true и события play/pause до страницы не дойдут.
            // native-мутация будит тик через `note_playback_change`, панель
            // обновляется на следующем redraw (одна итерация цикла событий).
            MediaZone::PlayPause => {
                #[cfg(feature = "v8")]
                self.route_media_ui_hook(nid, "toggle", "");
            }
            MediaZone::Mute => {
                let muted = {
                    let mut levels = store.audio_levels.lock().unwrap_or_else(|e| e.into_inner());
                    let entry = levels.entry(nid).or_insert((1.0, false));
                    entry.1 = !entry.1;
                    entry.1
                };
                self.request_redraw();
                // `video.muted` живёт в замыкании шима, не в store — без
                // хука геттер после клика по панели врёт.
                self.route_media_ui_hook(nid, "muted", if muted { "true" } else { "false" });
            }
            MediaZone::Volume | MediaZone::Seek => {
                self.apply_media_track(nid, hit.zone, hit.rect, x_css, now, hit.audio);
            }
            MediaZone::Fullscreen => {
                self.toggle_media_fullscreen(nid);
            }
        }
        true
    }

    /// Трек-зоны (seek / volume): доля курсора по треку → store (+ для
    /// `<audio>` — provider через JS-хук). Семантика seek повторяет натив
    /// `__lumen_video_seek`: `position_ms` переписывается и на паузе (тик
    /// передекодирует кадр), на игре — ре-якорь эпохи под текущий rate.
    fn apply_media_track(
        &mut self,
        nid: u32,
        zone: MediaZone,
        track: Rect,
        x_css: f32,
        now: u64,
        audio: bool,
    ) {
        let store = std::sync::Arc::clone(&self.video_gif_store);
        match zone {
            MediaZone::Seek => {
                let dur = store
                    .playback
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&nid)
                    .map_or(0, |e| e.cycle_ms);
                if dur == 0 {
                    return;
                }
                let frac = frac_of(track, x_css) as f64;
                let mut pb = store.playback.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(e) = pb.get_mut(&nid) {
                    e.position_ms = (frac * dur as f64) as u64;
                    if !e.paused {
                        e.play_epoch_ms = Some(now);
                    }
                    drop(pb);
                    store.note_playback_change(now as f64);
                    self.request_redraw();
                    // У `<audio>` позиция живёт в provider; без вызова
                    // provider останется на старом месте, а следующий
                    // pump-синк откатит панель.
                    if audio {
                        let secs = frac * dur as f64 / 1000.0;
                        self.route_media_ui_hook(nid, "seek", &format!("{secs:.3}"));
                    }
                }
            }
            MediaZone::Volume => {
                let frac = frac_of(track, x_css);
                store
                    .audio_levels
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .entry(nid)
                    .or_insert((1.0, false))
                    .0 = frac;
                self.request_redraw();
                self.route_media_ui_hook(nid, "volume", &format!("{frac:.3}"));
            }
            _ => {}
        }
    }

    /// Кнопка fullscreen на панели: войти/выйти из оконного fullscreen того
    /// же вида, что делает Escape-ветка `keyboard.rs` и drain
    /// `take_fullscreen_requests` в `about_to_wait.rs`. JS-состояние
    /// (`fullscreenElement`, `fullscreenchange`, sentinel-атрибут
    /// `data-lumen-fullscreen`) синхронизируется уведомлением шима.
    fn toggle_media_fullscreen(&mut self, nid: u32) {
        if self.fullscreen_nid == Some(nid) {
            self.fullscreen_nid = None;
            let prev = self.window.as_ref().map(|w| {
                w.set_fullscreen(None);
                w.inner_size()
            });
            if let Some(prev) = prev {
                self.arm_fullscreen_resize(prev);
            }
            #[cfg(feature = "v8")]
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                "if(typeof _lumen_notify_fullscreen_exit==='function')_lumen_notify_fullscreen_exit()"
                    .to_string(),
            );
        } else {
            self.fullscreen_nid = Some(nid);
            let prev = self.window.as_ref().map(|w| {
                w.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                w.inner_size()
            });
            if let Some(prev) = prev {
                self.arm_fullscreen_resize(prev);
            }
            #[cfg(feature = "v8")]
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!("if(typeof _lumen_notify_fullscreen_enter==='function')_lumen_notify_fullscreen_enter({nid})"),
            );
        }
        self.request_redraw();
    }

    /// `CursorMoved` при удержании трека панели: значение следует за
    /// курсором по сохранённому на press треку. Кнопочные захваты ничего
    /// не делают.
    pub(crate) fn update_media_panel_drag(&mut self, x_css: f32, _y_css: f32) {
        let Some(drag) = self.media_panel_drag else {
            return;
        };
        if !drag.zone.is_track() {
            return;
        }
        let now = self.media_clock_ms();
        self.apply_media_track(drag.nid, drag.zone, drag.track, x_css, now, drag.audio);
    }

    /// Release после захвата панели. Возвращает `true`, если press был
    /// панельным — тогда mouseup/pointerup/click странице не диспетчерятся.
    /// Для `<audio>` здесь дотягивается финальный seek: последний
    /// `CursorMoved` мог не дойти до актуальной позиции курсора.
    pub(crate) fn finish_media_panel_drag(&mut self, x_css: f32, _y_css: f32) -> bool {
        let Some(drag) = self.media_panel_drag.take() else {
            return false;
        };
        if drag.zone == MediaZone::Seek && drag.audio {
            let dur = self
                .video_gif_store
                .playback
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&drag.nid)
                .map_or(0, |e| e.cycle_ms);
            if dur > 0 {
                let secs = frac_of(drag.track, x_css) as f64 * dur as f64 / 1000.0;
                self.route_media_ui_hook(drag.nid, "seek", &format!("{secs:.3}"));
            }
        }
        true
    }

    /// Вызов `__lumen_media_ui_*`-хука JS-модели медиаэлемента (шим ставит
    /// их на каждый `<video>`/`<audio>`). Fire-and-forget: хук проводит
    /// мутацию той же дорогой, что и скриптовый вызов, со всеми событиями.
    #[cfg(feature = "v8")]
    fn route_media_ui_hook(&self, nid: u32, hook: &str, args: &str) {
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            format!(
                "var _me=_lumen_make_element({nid});if(_me&&typeof _me.__lumen_media_ui_{hook}==='function')_me.__lumen_media_ui_{hook}({args})"
            ),
        );
    }
}

/// Контекст одного прохода по layout-дереву.
struct PanelCtx<'a> {
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
            // BUG-1802: `controls` обоих медиа-элементов живёт в kind —
            // DOM на UI-потоке не читается (см. doc у `build_media_controls_overlay`).
            let has_controls = match kind {
                BoxKind::Video { controls, .. } | BoxKind::Audio { controls, .. } => *controls,
                _ => false,
            };
            if has_controls {
                let is_audio = matches!(kind, BoxKind::Audio { .. });
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
                let (volume, muted) = ctx
                    .store
                    .audio_levels
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&nid)
                    .map_or((1.0, false), |(v, m)| (*v, *m));
                let st = PanelState {
                    paused,
                    cur_ms,
                    dur_ms,
                    muted,
                    volume,
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
                    let hit = |zone, rect| MediaHit { nid, zone, rect, audio: is_audio };
                    // Порядок = приоритет hit-test-а; зоны не пересекаются.
                    ctx.hits.push(hit(MediaZone::PlayPause, ui.play_btn));
                    ctx.hits.push(hit(MediaZone::Seek, track_hit(ui.seek)));
                    if let Some(v) = ui.volume {
                        ctx.hits.push(hit(MediaZone::Volume, track_hit(v)));
                    }
                    ctx.hits.push(hit(MediaZone::Mute, ui.mute_btn));
                    if let Some(fs) = ui.fs_btn {
                        ctx.hits.push(hit(MediaZone::Fullscreen, fs));
                    }
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
        // Правый край: fullscreen, затем mute; между ними зазор BTN_INSET.
        let fs = ui.fs_btn.expect("400px bar has fullscreen");
        assert!((fs.right() - bar.right() + BTN_INSET).abs() < 0.01);
        assert!((ui.mute_btn.right() - fs.x + BTN_INSET).abs() < 0.01);
        // Порядок play → seek → volume → mute, без перекрытий.
        assert!(ui.seek.width > 80.0);
        assert!(ui.seek.x > ui.play_btn.right());
        let vol = ui.volume.expect("400px bar has volume");
        assert!(ui.seek.right() < vol.x);
        assert!(vol.right() < ui.mute_btn.x);
        assert!((vol.width - VOL_W).abs() < 0.01);
    }

    #[test]
    fn panel_ui_on_a_medium_bar_hides_volume() {
        let bar = r(0.0, 0.0, 300.0, PANEL_H);
        let ui = panel_ui(bar, 0, 0).expect("300px bar");
        assert!(ui.time.is_some());
        assert!(ui.volume.is_none(), "volume starts at VOL_MIN_BAR_W");
        assert!(ui.fs_btn.is_some());
        assert!(ui.seek.width >= 24.0);
    }

    #[test]
    fn panel_ui_on_a_narrow_bar_hides_time() {
        let bar = r(0.0, 0.0, 120.0, PANEL_H);
        let ui = panel_ui(bar, 0, 0).expect("120px bar");
        assert!(ui.time.is_none());
        assert!(ui.volume.is_none());
        assert!(ui.fs_btn.is_none());
        assert!(ui.seek.width >= 24.0);
    }

    #[test]
    fn panel_ui_rejects_a_degenerate_bar() {
        assert!(panel_ui(r(0.0, 0.0, 40.0, PANEL_H), 0, 0).is_none());
        assert!(panel_ui(r(0.0, 0.0, MIN_BAR_W, 4.0), 0, 0).is_none());
    }

    #[test]
    fn track_hit_pads_the_visual_track() {
        let t = r(10.0, 118.0, 100.0, SEEK_H);
        let hit = track_hit(t);
        assert!((hit.height - (SEEK_H + TRACK_HIT_PAD * 2.0)).abs() < 0.01);
        assert_eq!(hit.x, t.x);
        assert!((hit.width - t.width).abs() < 0.01);
        // Доля по расширенному hit-прямоугольнику совпадает с видимой полосой.
        assert!((frac_of(hit, t.x + t.width * 0.5) - 0.5).abs() < 0.001);
        assert_eq!(frac_of(hit, t.x - 5.0), 0.0);
        assert_eq!(frac_of(hit, t.right() + 5.0), 1.0);
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
        assert_eq!(fullscreen_icon([16.0, 16.0]).len(), 8);
    }
}
