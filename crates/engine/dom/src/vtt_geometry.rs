//! Настройки cue WebVTT (§6.3) и геометрия cue-бокса (§7.2 «WebVTT cue text
//! track rendering», обработка cue settings).
//!
//! Сырые строки из [`VttCueSettings`](crate::vtt::VttCueSettings) разбираются в
//! типизированные [`CueSettings`] (валидное значение — typed, невалидное —
//! значение по умолчанию, как требует §6.3), а [`cue_box_css`] вычисляет по
//! алгоритму §7.2 строку CSS-геометрии cue-бокса: `position:absolute`, смещение
//! и размер по строчной оси (computed position × maximum size), смещение по
//! блочной оси (`line`/`lineAlign`), `writing-mode` для вертикальных cue и
//! `text-align`. Потребители: раскладка cue в UA-тени `<video>` (срезы
//! VTT-RENDER-S3/S4) и мост JS→DOM (S5).

use crate::vtt::VttCueSettings;

/// Направление письма cue-бокса (§7.2, writing direction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vertical {
    /// `vertical:` (пустое значение) или настройка отсутствует — горизонтально.
    #[default]
    Horizontal,
    /// `vertical:rl` — блоки наращиваются справа налево (японский вертикальный).
    Rl,
    /// `vertical:lr` — блоки наращиваются слева направо (монгольский вертикальный).
    Lr,
}

/// Значение настройки `line` (§6.3): `auto`, номер строки или процент.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum CueLine {
    /// `auto` — позиция по умолчанию (низ области видео).
    #[default]
    Auto,
    /// Номер строки (может быть отрицательным — счёт с нижнего края);
    /// snap-to-lines остаётся включённым.
    Number(f64),
    /// Процент (0–100) от размера области видео по блочной оси; выключает
    /// snap-to-lines (§7.2: «`line` в процентах делает `snap_to_lines` false»).
    Percent(f64),
}

/// Привязка line-значения к боксу cue при процентной `line` (§6.3 line alignment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineAlign {
    /// Край бокса, откуда отсчитывается line, ставится в точку line.
    #[default]
    Start,
    /// Центр бокса ставится в точку line.
    Center,
    /// Противоположный край бокса ставится в точку line.
    End,
}

/// Выравнивание позиции cue-бокса по строчной оси (§7.2 position alignment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionAlign {
    /// `auto` — выводится из text alignment.
    #[default]
    Auto,
    /// `line-left` — у точки позиции строчный край бокса.
    LineLeft,
    /// `center` — центр бокса в точке позиции.
    Center,
    /// `line-right` — у точки позиции противолежащий строчный край.
    LineRight,
}

/// Выравнивание текста cue (§6.3, text alignment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CueAlign {
    Start,
    #[default]
    Center,
    End,
    Left,
    Right,
}

/// Типизированные настройки позиционирования cue — результат обработки сырых
/// строк [`VttCueSettings`] по §6.3 (значения по умолчанию — §7.1).
#[derive(Debug, Clone, PartialEq)]
pub struct CueSettings {
    /// Направление письма.
    pub vertical: Vertical,
    /// Привязка к строкам; `false`, когда `line` задан процентом.
    pub snap_to_lines: bool,
    /// Смещение по блочной оси.
    pub line: CueLine,
    /// Привязка процентной line к краю/центру бокса.
    pub line_align: LineAlign,
    /// Позиция по строчной оси в процентах (0–100); `None` — `auto`.
    pub position: Option<f64>,
    /// Выравнивание позиции; `Auto` выводится из text alignment.
    pub position_align: PositionAlign,
    /// Размер по строчной оси в процентах (0–100).
    pub size: f64,
    /// Выравнивание текста внутри бокса.
    pub align: CueAlign,
    /// Идентификатор региона (`region:`), если задан.
    pub region: Option<String>,
}

impl Default for CueSettings {
    fn default() -> Self {
        CueSettings {
            vertical: Vertical::Horizontal,
            snap_to_lines: true,
            line: CueLine::Auto,
            line_align: LineAlign::Start,
            position: None,
            position_align: PositionAlign::Auto,
            size: 100.0,
            align: CueAlign::Center,
            region: None,
        }
    }
}

/// Разбирает процентное значение настройки (`"50%"` → `Some(50.0)`).
/// Строка обязана заканчиваться `%` и содержать до `%` корректное число;
/// результат клампится в 0.0..=100.0.
fn parse_percent_value(s: &str) -> Option<f64> {
    let num = s.strip_suffix('%')?;
    let v: f64 = num.parse().ok()?;
    Some(v.clamp(0.0, 100.0))
}

impl CueSettings {
    /// Обрабатывает сырые строки настроек cue по правилам §6.3: каждая
    /// настройка разбирается независимо, невалидное значение даёт значение
    /// по умолчанию (п. 6 алгоритма «parse the WebVTT cue settings»).
    pub fn parse(raw: &VttCueSettings) -> CueSettings {
        let mut out = CueSettings::default();

        // vertical: пустое значение (ключ без значения) валиден и означает
        // горизонтальное письмо; любое иное неизвестное значение — по умолчанию.
        if let Some(v) = &raw.vertical {
            out.vertical = match v.as_str() {
                "rl" => Vertical::Rl,
                "lr" => Vertical::Lr,
                "" => Vertical::Horizontal,
                _ => Vertical::Horizontal,
            };
        }

        // line: `auto`, номер строки или процент с необязательной привязкой
        // `,%lineAlign` (§6.3). Процент выключает snap-to-lines.
        if let Some(v) = &raw.line {
            let (main, align) = split_setting(v);
            let align_valid = align.is_none_or(|a| matches!(a, "start" | "center" | "end"));
            if align_valid && !(main == "auto" && align.is_some()) {
                out.line_align = match align {
                    Some("center") => LineAlign::Center,
                    Some("end") => LineAlign::End,
                    _ => LineAlign::Start,
                };
                out.line = if main == "auto" {
                    CueLine::Auto
                } else if let Some(p) = parse_percent_value(main) {
                    out.snap_to_lines = false;
                    CueLine::Percent(p)
                } else if let Ok(n) = main.parse::<f64>() {
                    CueLine::Number(n)
                } else {
                    // Основная часть невалидна — вся настройка по умолчанию.
                    out.line_align = LineAlign::Start;
                    CueLine::Auto
                };
            }
        }

        // position: `auto`, процент и необязательная привязка `,%positionAlign`.
        if let Some(v) = &raw.position {
            let (main, align) = split_setting(v);
            let align_valid = align.is_none_or(|a| matches!(a, "line-left" | "center" | "line-right"));
            if main == "auto" && align.is_none() {
                out.position = None;
                out.position_align = PositionAlign::Auto;
            } else if align_valid && parse_percent_value(main).is_some() {
                out.position = parse_percent_value(main);
                // Без суффикса выравнивание позиции остаётся `auto` и
                // выводится из text alignment на этапе геометрии (§7.2).
                out.position_align = match align {
                    Some("line-left") => PositionAlign::LineLeft,
                    Some("center") => PositionAlign::Center,
                    Some(_) => PositionAlign::LineRight,
                    None => PositionAlign::Auto,
                };
            } else {
                // Невалидное значение (или неизвестная привязка) — по умолчанию.
                out.position = None;
                out.position_align = PositionAlign::Auto;
            }
        }

        // size: только процент; хвост после запятой делает настройку невалидной.
        if let Some(v) = &raw.size
            && let Some(p) = parse_percent_value(v)
            && !v.contains(',')
        {
            out.size = p;
        }

        // align: значения регистрозависимы; неизвестное — центр по умолчанию.
        if let Some(v) = &raw.align {
            out.align = match v.as_str() {
                "start" => CueAlign::Start,
                "end" => CueAlign::End,
                "left" => CueAlign::Left,
                "right" => CueAlign::Right,
                _ => CueAlign::Center,
            };
        }

        // region: любой непустой идентификатор (привязка к объявленному
        // региону проверяется при рендере, не здесь).
        if let Some(v) = &raw.region {
            let id = v.trim();
            if !id.is_empty() {
                out.region = Some(id.to_owned());
            }
        }

        out
    }
}

/// Делит значение настройки на основную часть и хвост после первой запятой
/// (`"50%,center"` → `("50%", Some("center"))`). Возвращает уже срезанные части.
fn split_setting(v: &str) -> (&str, Option<&str>) {
    match v.split_once(',') {
        Some((main, tail)) => (main, Some(tail)),
        None => (v, None),
    }
}

/// Число без лишних нулей: целые значения печатаются без дробной части
/// (`0%`, `100%`, `1lh`), дробные — как есть.
fn fmt_num(v: f64) -> String {
    let rounded = v.round();
    if (v - rounded).abs() < 1e-9 && rounded.abs() < 9.0e15 {
        format!("{}", rounded as i64)
    } else {
        format!("{}", v)
    }
}

/// Вычисляет строку CSS-геометрии cue-бокса по алгоритму §7.2.
///
/// Строка всегда начинается с `position:absolute;` и содержит только
/// геометрию (позиция/размер по строчной оси через computed position и
/// maximum size; смещение по блочной оси через `line`), `writing-mode` для
/// вертикального письма и `text-align`. Свойства текста (шрифт, цвет, фон,
/// `white-space`) выставляет `apply_ua_vtt` на фазе UA-каскада (срез S4).
///
/// Поправка `lineAlign` для процентной `line` (§7.2: start — край бокса в
/// точке line, center — центр, end — противолежащий край) выражается
/// `transform: translate*`, потому что высота бокса в `lh` известна только
/// на этапе раскладки; статический CSS без transform её не выражает.
pub fn cue_box_css(s: &CueSettings) -> String {
    // §7.2 шаг 2: position alignment; `auto` выводится из text alignment.
    let pos_align = match s.position_align {
        PositionAlign::Auto => match s.align {
            CueAlign::Start | CueAlign::Left => PositionAlign::LineLeft,
            CueAlign::Center => PositionAlign::Center,
            CueAlign::End | CueAlign::Right => PositionAlign::LineRight,
        },
        resolved => resolved,
    };

    // §7.2 шаг 3: computed position; `auto` — 0/50/100 % по выравниванию.
    let position = s.position.unwrap_or(match pos_align {
        PositionAlign::LineLeft => 0.0,
        PositionAlign::Center => 50.0,
        PositionAlign::LineRight => 100.0,
        PositionAlign::Auto => 50.0, // недостижимо после разрешения выше
    });

    // §7.2 шаг 4: maximum size по выравниванию позиции.
    let max_size = match pos_align {
        PositionAlign::LineLeft => 100.0 - position,
        PositionAlign::LineRight => position,
        PositionAlign::Center | PositionAlign::Auto => position.min(100.0 - position) * 2.0,
    };
    // §7.2 шаг 5: размер не превышает maximum size.
    let size = s.size.min(max_size);

    // Ось блока: горизонтальное письмо — top/bottom; rl — блок от правого
    // края (block-start = right); lr — от левого (block-start = left).
    let (block_start, block_end, inline_prop, size_prop, translate_axis, translate_sign) =
        match s.vertical {
            Vertical::Horizontal => ("top", "bottom", "left", "width", 'Y', -1.0),
            Vertical::Rl => ("right", "left", "top", "height", 'X', 1.0),
            Vertical::Lr => ("left", "right", "top", "height", 'X', -1.0),
        };

    // Смещение по строчной оси: у точки позиции — строчный край (line-left),
    // центр или противолежащий край (line-right).
    let inline_offset = match pos_align {
        PositionAlign::LineLeft => position,
        PositionAlign::Center => position - size / 2.0,
        PositionAlign::LineRight => position - size,
        PositionAlign::Auto => 0.0, // недостижимо после разрешения выше
    };

    let mut css = String::from("position:absolute;");
    css.push_str(&format!(
        "{}:{}%;{}:{}%;",
        inline_prop,
        fmt_num(inline_offset),
        size_prop,
        fmt_num(size)
    ));

    // Смещение по блочной оси (§7.2 шаг 6-7): номер строки >= 0 — от
    // block-start края, отрицательный или auto — от block-end края
    // ((|N|-1) строк; auto = -1, т.е. вплотную к краю). Процент — от
    // block-start края с поправкой lineAlign (только для процента).
    match s.line {
        CueLine::Number(n) if n >= 0.0 => {
            css.push_str(&format!("{}:{}lh;", block_start, fmt_num(n)));
        }
        CueLine::Number(n) => {
            css.push_str(&format!("{}:{}lh;", block_end, fmt_num(-n - 1.0)));
        }
        CueLine::Auto => {
            css.push_str(&format!("{}:0lh;", block_end));
        }
        CueLine::Percent(p) => {
            css.push_str(&format!("{}:{}%;", block_start, fmt_num(p)));
            match s.line_align {
                LineAlign::Start => {}
                LineAlign::Center | LineAlign::End => {
                    let shift = match s.line_align {
                        LineAlign::Center => 50.0,
                        _ => 100.0,
                    } * translate_sign;
                    css.push_str(&format!("transform:translate{}({}%);", translate_axis, fmt_num(shift)));
                }
            }
        }
    }

    // Вертикальное письмо — writing-mode; горизонтальный не пишем (значение
    // по умолчанию).
    match s.vertical {
        Vertical::Horizontal => {}
        Vertical::Rl => css.push_str("writing-mode:vertical-rl;"),
        Vertical::Lr => css.push_str("writing-mode:vertical-lr;"),
    }

    // text-align из text alignment (§7.4); вертикальное письмо CSS трактует
    // сам — свойство то же.
    let align = match s.align {
        CueAlign::Start => "start",
        CueAlign::Center => "center",
        CueAlign::End => "end",
        CueAlign::Left => "left",
        CueAlign::Right => "right",
    };
    // text-align — последнее свойство, без хвостового разделителя.
    css.push_str(&format!("text-align:{}", align));
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(raw: VttCueSettings) -> CueSettings {
        CueSettings::parse(&raw)
    }

    fn raw(line: Option<&str>, position: Option<&str>, size: Option<&str>, align: Option<&str>, vertical: Option<&str>) -> VttCueSettings {
        VttCueSettings {
            vertical: vertical.map(str::to_owned),
            line: line.map(str::to_owned),
            position: position.map(str::to_owned),
            size: size.map(str::to_owned),
            align: align.map(str::to_owned),
            region: None,
        }
    }

    // ── разбор настроек ─────────────────────────────────────────────────────

    #[test]
    fn vtt_geometry_parse_defaults() {
        let s = settings(VttCueSettings::default());
        assert_eq!(s.vertical, Vertical::Horizontal);
        assert!(s.snap_to_lines);
        assert_eq!(s.line, CueLine::Auto);
        assert_eq!(s.line_align, LineAlign::Start);
        assert_eq!(s.position, None);
        assert_eq!(s.position_align, PositionAlign::Auto);
        assert_eq!(s.size, 100.0);
        assert_eq!(s.align, CueAlign::Center);
        assert_eq!(s.region, None);
    }

    #[test]
    fn vtt_geometry_parse_line_number_keeps_snap() {
        let s = settings(raw(Some("-2"), None, None, None, None));
        assert_eq!(s.line, CueLine::Number(-2.0));
        assert!(s.snap_to_lines);
    }

    #[test]
    fn vtt_geometry_parse_line_percent_disables_snap() {
        let s = settings(raw(Some("50%"), None, None, None, None));
        assert_eq!(s.line, CueLine::Percent(50.0));
        assert!(!s.snap_to_lines);
    }

    #[test]
    fn vtt_geometry_parse_line_with_alignment() {
        let s = settings(raw(Some("50%,center"), None, None, None, None));
        assert_eq!(s.line, CueLine::Percent(50.0));
        assert_eq!(s.line_align, LineAlign::Center);
    }

    #[test]
    fn vtt_geometry_parse_invalid_line_falls_back_to_default() {
        let s = settings(raw(Some("abc"), None, None, None, None));
        assert_eq!(s.line, CueLine::Auto);
        assert!(s.snap_to_lines);
    }

    #[test]
    fn vtt_geometry_parse_invalid_line_alignment_is_whole_setting_invalid() {
        let s = settings(raw(Some("50%,bogus"), None, None, None, None));
        assert_eq!(s.line, CueLine::Auto);
        assert_eq!(s.line_align, LineAlign::Start);
        assert!(s.snap_to_lines);
    }

    #[test]
    fn vtt_geometry_parse_position_with_align_suffix() {
        let s = settings(raw(None, Some("10%,line-left"), None, None, None));
        assert_eq!(s.position, Some(10.0));
        assert_eq!(s.position_align, PositionAlign::LineLeft);
    }

    #[test]
    fn vtt_geometry_parse_position_auto() {
        let s = settings(raw(None, Some("auto"), None, None, None));
        assert_eq!(s.position, None);
    }

    #[test]
    fn vtt_geometry_parse_invalid_position_falls_back_to_default() {
        let s = settings(raw(None, Some("oops"), None, None, None));
        assert_eq!(s.position, None);
        assert_eq!(s.position_align, PositionAlign::Auto);
    }

    #[test]
    fn vtt_geometry_parse_size_clamped() {
        let s = settings(raw(None, None, Some("150%"), None, None));
        assert_eq!(s.size, 100.0);
        let s2 = settings(raw(None, None, Some("50%"), None, None));
        assert_eq!(s2.size, 50.0);
    }

    #[test]
    fn vtt_geometry_parse_invalid_size_falls_back_to_default() {
        let s = settings(raw(None, None, Some("wide"), None, None));
        assert_eq!(s.size, 100.0);
        let s2 = settings(raw(None, None, Some("50%,x"), None, None));
        assert_eq!(s2.size, 100.0);
    }

    #[test]
    fn vtt_geometry_parse_align_variants() {
        assert_eq!(settings(raw(None, None, None, Some("start"), None)).align, CueAlign::Start);
        assert_eq!(settings(raw(None, None, None, Some("left"), None)).align, CueAlign::Left);
        assert_eq!(settings(raw(None, None, None, Some("end"), None)).align, CueAlign::End);
        assert_eq!(settings(raw(None, None, None, Some("right"), None)).align, CueAlign::Right);
        assert_eq!(settings(raw(None, None, None, Some("bogus"), None)).align, CueAlign::Center);
    }

    #[test]
    fn vtt_geometry_parse_vertical() {
        assert_eq!(settings(raw(None, None, None, None, Some("rl"))).vertical, Vertical::Rl);
        assert_eq!(settings(raw(None, None, None, None, Some("lr"))).vertical, Vertical::Lr);
        assert_eq!(settings(raw(None, None, None, None, Some(""))).vertical, Vertical::Horizontal);
        assert_eq!(settings(raw(None, None, None, None, Some("xx"))).vertical, Vertical::Horizontal);
    }

    #[test]
    fn vtt_geometry_parse_region() {
        let r = VttCueSettings { region: Some("fred".to_owned()), ..Default::default() };
        let s = settings(r);
        assert_eq!(s.region.as_deref(), Some("fred"));
    }

    // ── геометрия cue-бокса §7.2 (ожидания из -ref.html рефтестов WPT) ─────

    #[test]
    fn vtt_geometry_default_matches_reference() {
        // Рефтест basic (видео 320x180, line auto, size 100): ширина 100 %,
        // левый край 0 %, низ вплотную.
        let css = cue_box_css(&CueSettings::default());
        assert_eq!(css, "position:absolute;left:0%;width:100%;bottom:0lh;text-align:center");
    }

    #[test]
    fn vtt_geometry_size_half_centers() {
        let css = cue_box_css(&settings(raw(None, None, Some("50%"), None, None)));
        assert!(css.contains("left:25%;width:50%"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_line_zero_is_top() {
        let css = cue_box_css(&settings(raw(Some("0"), None, None, None, None)));
        assert!(css.contains("top:0lh"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_line_negative_two_with_size_position() {
        let css = cue_box_css(&settings(raw(Some("-2"), Some("50%"), Some("50%"), None, None)));
        assert!(css.contains("bottom:1lh;"), "got: {css}");
        assert!(css.contains("left:25%;width:50%"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_line_percent_is_top_percent() {
        let css = cue_box_css(&settings(raw(Some("50%"), None, None, None, None)));
        assert!(css.contains("top:50%"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_center_position_90_shrinks_to_maximum_size() {
        let css = cue_box_css(&settings(raw(None, Some("90%"), None, Some("center"), None)));
        assert!(css.contains("left:80%;width:20%"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_vertical_rl_swaps_axes() {
        let css = cue_box_css(&settings(raw(None, None, None, None, Some("rl"))));
        assert!(css.contains("writing-mode:vertical-rl"), "got: {css}");
        // line → блочная ось = горизонталь (auto → левый край вплотную),
        // position/size → строчная ось = вертикаль (top/height).
        assert!(css.contains("left:0lh"), "got: {css}");
        assert!(css.contains("top:0%;height:100%"), "got: {css}");
        assert!(!css.contains("width:"), "got: {css}");
        assert!(!css.contains("bottom:"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_vertical_lr_line_zero_grows_from_left() {
        let css = cue_box_css(&settings(raw(Some("1"), None, Some("50%"), None, Some("lr"))));
        assert!(css.contains("writing-mode:vertical-lr"), "got: {css}");
        assert!(css.contains("left:1lh"), "got: {css}");
        assert!(css.contains("height:50%"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_align_start_position_auto_starts_at_left_edge() {
        let css = cue_box_css(&settings(raw(None, None, None, Some("start"), None)));
        assert!(css.contains("left:0%"), "got: {css}");
        assert!(css.contains("width:100%"), "got: {css}");
        assert!(css.contains("text-align:start"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_line_align_center_shifts_percent_line() {
        let css = cue_box_css(&settings(raw(Some("50%,center"), None, None, None, None)));
        assert!(css.contains("top:50%"), "got: {css}");
        assert!(css.contains("transform:translateY(-50%)"), "got: {css}");
        let rl = cue_box_css(&settings(raw(Some("50%,center"), None, None, None, Some("rl"))));
        assert!(rl.contains("right:50%"), "got: {rl}");
        assert!(rl.contains("transform:translateX(50%)"), "got: {rl}");
    }

    #[test]
    fn vtt_geometry_line_align_end_shifts_full_box() {
        let css = cue_box_css(&settings(raw(Some("50%,end"), None, None, None, None)));
        assert!(css.contains("transform:translateY(-100%)"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_line_align_ignored_for_integer_line() {
        let css = cue_box_css(&settings(raw(Some("1,end"), None, None, None, None)));
        assert!(css.contains("top:1lh"), "got: {css}");
        assert!(!css.contains("transform"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_position_line_right_anchors_end_edge() {
        // position:90%,line-right → правый край бокса в точке 90 %.
        let css = cue_box_css(&settings(raw(None, Some("90%,line-right"), None, None, None)));
        // max size при line-right = position = 90, size 100 → 90; left = 90-90 = 0.
        assert!(css.contains("left:0%;width:90%"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_fractional_values_keep_decimals() {
        // center: size 20 % (max 67 не режет), left = 33.5 − 10 = 23.5.
        let css = cue_box_css(&settings(raw(None, Some("33.5%"), Some("20%"), None, None)));
        assert!(css.contains("left:23.5%;width:20%"), "got: {css}");
    }

    #[test]
    fn vtt_geometry_fmt_num_drops_trailing_zero() {
        assert_eq!(fmt_num(0.0), "0");
        assert_eq!(fmt_num(100.0), "100");
        assert_eq!(fmt_num(12.5), "12.5");
    }
}
