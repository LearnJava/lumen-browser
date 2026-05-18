//! `fvar` — Font Variations table. Описывает variation axes (wght/wdth/slnt/
//! opsz/ital и custom-теги): для каждой оси хранятся `min` / `default` / `max`
//! значения в axis units, плюс именованные **instances** («Regular», «Bold»,
//! «Light Italic» и т.д.) — заранее определённые точки в пространстве осей.
//! Это **enabler** для Variable Fonts Level 1 runtime — сам интерполяционный
//! pipeline (gvar / avar / HVAR / interpolation в rasterizer-е) появится
//! позже, когда P2 дойдёт до P2 «Variable fonts axes runtime» в roadmap-е.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/fvar>.
//!
//! Phase 0 ограничения:
//! - `axisNameID` / `subfamilyNameID` / `postScriptNameID` сохраняются как
//!   `u16` — резолв в строку через `name` table делает caller (если нужно
//!   вывести «Weight» / «Bold Italic» в picker-е).

use crate::binary::BinaryReader;
use crate::face::FontError;

const FVAR: [u8; 4] = *b"fvar";

/// Одна variation axis. Все значения в native axis units (не CSS-нормализо-
/// ванные): caller, который хочет получить CSS-совместимый `font-weight: 700`,
/// сам сравнивает с `axis(b"wght")` диапазоном.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VariationAxis {
    /// 4-байтовый OpenType tag. Стандартные (CSS Fonts L4): `wght` (weight),
    /// `wdth` (width), `slnt` (slant), `opsz` (optical size), `ital` (italic).
    /// Custom tags разрешены спекой; CSS-сторона передаёт их через
    /// `font-variation-settings`.
    pub tag: [u8; 4],
    /// Минимум диапазона (включительно).
    pub min: f32,
    /// Default — значение, используемое font-ом «по умолчанию» (= classic
    /// shape без вариаций). CSS-side: при `font-variation-settings: normal`
    /// rasterizer должен использовать default для всех axes.
    pub default: f32,
    /// Максимум диапазона (включительно).
    pub max: f32,
    /// Bit 0 — axis скрыт от UI font picker-а (рекомендация шрифтового
    /// дизайнера). Bits 1-15 reserved.
    pub flags: u16,
    /// Индекс в `name` table для human-readable названия оси (например,
    /// «Weight», «Optical Size»). Resolve в строку — задача caller-а через
    /// [`crate::Name`].
    pub axis_name_id: u16,
}

impl VariationAxis {
    /// Bit 0 — `HIDDEN_AXIS` flag из spec. UI font picker не должен
    /// показывать такие axes как настраиваемые.
    pub const FLAG_HIDDEN: u16 = 0x0001;

    pub fn is_hidden(self) -> bool {
        self.flags & Self::FLAG_HIDDEN != 0
    }

    /// Зажать значение в `[min, max]`. Полезно при подаче CSS-уровневого
    /// значения axis-а в rasterizer (вне диапазона — undefined behaviour
    /// per spec; clamp — типовое поведение Chromium / WebKit).
    pub fn clamp(self, value: f32) -> f32 {
        if value < self.min {
            self.min
        } else if value > self.max {
            self.max
        } else {
            value
        }
    }
}

/// Одна named instance — фиксированная точка в пространстве variation axes,
/// которой дизайнер дал имя («Bold Italic», «Light Condensed» и т.д.).
/// CSS-side: font-variation-settings picker может показать эти точки как
/// предустановки.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedInstance {
    /// `name` table id для отображаемого имени instance-а («Bold Italic»).
    pub subfamily_name_id: u16,
    /// Reserved per spec — обычно 0. Сохраняем для прозрачности.
    pub flags: u16,
    /// Координаты по каждой оси в порядке `Fvar.axes`. Длина = `axes.len()`.
    /// Значения в native axis units (как и `VariationAxis.default`).
    pub coordinates: Vec<f32>,
    /// `name` table id для PostScript-имени instance-а (используется для
    /// shaping engine-ов / OS-level font matching). `None`, если в таблице
    /// постскрипт-id не прописан (см. spec: instanceSize либо
    /// `4 + 4×axisCount`, либо `6 + 4×axisCount` — второй вариант с
    /// PostScript id).
    pub post_script_name_id: Option<u16>,
}

/// Все axes и instances из `fvar`. Порядок — как в таблице (важно: координаты
/// instance-а индексируются по позиции оси в `axes`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fvar {
    pub axes: Vec<VariationAxis>,
    pub instances: Vec<NamedInstance>,
}

impl Fvar {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let major = r.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
        let _minor = r.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
        if major != 1 {
            return Err(FontError::InvalidTable(FVAR));
        }
        let axes_array_offset = r.read_u16().ok_or(FontError::InvalidTable(FVAR))? as usize;
        // countSizePairs (uint16) — reserved в spec, всегда 2. Пропускаем.
        r.skip(2).ok_or(FontError::InvalidTable(FVAR))?;
        let axis_count = r.read_u16().ok_or(FontError::InvalidTable(FVAR))? as usize;
        let axis_size = r.read_u16().ok_or(FontError::InvalidTable(FVAR))? as usize;
        let instance_count = r.read_u16().ok_or(FontError::InvalidTable(FVAR))? as usize;
        let instance_size = r.read_u16().ok_or(FontError::InvalidTable(FVAR))? as usize;

        // По spec axisSize всегда 20: tag (4) + min/default/max (3×4 Fixed)
        // + flags (2) + nameID (2). Других значений в практике нет; если
        // встретили — отказ парсинга безопаснее, чем silent wrong-offsets.
        if axis_size != 20 {
            return Err(FontError::InvalidTable(FVAR));
        }

        // По spec instanceSize либо `4 + 4×axisCount` (без postScriptNameID),
        // либо `6 + 4×axisCount` (с postScriptNameID). Любое другое значение —
        // невалидно. Проверка не применяется при instance_count == 0
        // (тогда instance_size часто пишут 0, и tail-данных всё равно нет).
        let coord_bytes = 4usize
            .checked_mul(axis_count)
            .ok_or(FontError::InvalidTable(FVAR))?;
        let inst_size_without_psid = coord_bytes
            .checked_add(4)
            .ok_or(FontError::InvalidTable(FVAR))?;
        let inst_size_with_psid = coord_bytes
            .checked_add(6)
            .ok_or(FontError::InvalidTable(FVAR))?;
        let has_post_script_id = if instance_count == 0 || instance_size == inst_size_without_psid
        {
            false
        } else if instance_size == inst_size_with_psid {
            true
        } else {
            return Err(FontError::InvalidTable(FVAR));
        };

        let mut axes = Vec::with_capacity(axis_count);
        for i in 0..axis_count {
            let off = axes_array_offset
                .checked_add(i.checked_mul(axis_size).ok_or(FontError::InvalidTable(FVAR))?)
                .ok_or(FontError::InvalidTable(FVAR))?;
            let end = off
                .checked_add(axis_size)
                .ok_or(FontError::InvalidTable(FVAR))?;
            if end > data.len() {
                return Err(FontError::InvalidTable(FVAR));
            }
            let mut a = BinaryReader::new(&data[off..end]);
            let tag = a.read_tag().ok_or(FontError::InvalidTable(FVAR))?;
            let min = read_fixed(&mut a).ok_or(FontError::InvalidTable(FVAR))?;
            let default = read_fixed(&mut a).ok_or(FontError::InvalidTable(FVAR))?;
            let max = read_fixed(&mut a).ok_or(FontError::InvalidTable(FVAR))?;
            let flags = a.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
            let axis_name_id = a.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
            axes.push(VariationAxis {
                tag,
                min,
                default,
                max,
                flags,
                axis_name_id,
            });
        }

        // Instances идут в массиве после axes, начиная с offset
        // `axesArrayOffset + axisCount * axisSize`. Spec явно требует именно
        // такое расположение (instance records — компактный массив без
        // padding между записями).
        let instances_offset = axes_array_offset
            .checked_add(
                axis_count
                    .checked_mul(axis_size)
                    .ok_or(FontError::InvalidTable(FVAR))?,
            )
            .ok_or(FontError::InvalidTable(FVAR))?;

        let mut instances = Vec::with_capacity(instance_count);
        for i in 0..instance_count {
            let off = instances_offset
                .checked_add(
                    i.checked_mul(instance_size)
                        .ok_or(FontError::InvalidTable(FVAR))?,
                )
                .ok_or(FontError::InvalidTable(FVAR))?;
            let end = off
                .checked_add(instance_size)
                .ok_or(FontError::InvalidTable(FVAR))?;
            if end > data.len() {
                return Err(FontError::InvalidTable(FVAR));
            }
            let mut s = BinaryReader::new(&data[off..end]);
            let subfamily_name_id = s.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
            let flags = s.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
            let mut coordinates = Vec::with_capacity(axis_count);
            for _ in 0..axis_count {
                coordinates.push(read_fixed(&mut s).ok_or(FontError::InvalidTable(FVAR))?);
            }
            let post_script_name_id = if has_post_script_id {
                Some(s.read_u16().ok_or(FontError::InvalidTable(FVAR))?)
            } else {
                None
            };
            instances.push(NamedInstance {
                subfamily_name_id,
                flags,
                coordinates,
                post_script_name_id,
            });
        }

        Ok(Self { axes, instances })
    }

    /// Найти axis по tag-у. Возвращает `None`, если в шрифте нет такой
    /// оси (например, font не вариативный по `wght`).
    pub fn axis(&self, tag: &[u8; 4]) -> Option<&VariationAxis> {
        self.axes.iter().find(|a| &a.tag == tag)
    }

    /// `true`, если шрифт имеет хотя бы одну variation axis. Для non-variable
    /// fonts таблица `fvar` обычно отсутствует, и `Font::fvar()` вернёт
    /// `Err(TableNotFound)`; этот хелпер удобен в коде, который сам создаёт
    /// пустой `Fvar` через `Default`.
    pub fn is_variable(&self) -> bool {
        !self.axes.is_empty()
    }

    /// Найти named instance с указанным `subfamily_name_id`. Возвращает
    /// `None`, если такого instance нет. Caller сам резолвит name_id в
    /// строку через `name` table; для имени instance-а это обычно его
    /// видимое имя в UI font picker-е.
    pub fn instance_by_name_id(&self, name_id: u16) -> Option<&NamedInstance> {
        self.instances.iter().find(|i| i.subfamily_name_id == name_id)
    }
}

/// `F16Dot16` (fixed-point 16.16) → f32. OpenType хранит axis values в
/// этом формате — sign-extended big-endian i32, разделён на 65536.
fn read_fixed(r: &mut BinaryReader<'_>) -> Option<f32> {
    Some(r.read_i32()? as f32 / 65536.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Кладёт f32 как F16Dot16 (sign-extended 32-bit BE).
    fn put_fixed(v: f32) -> [u8; 4] {
        let raw = (v * 65536.0).round() as i32;
        raw.to_be_bytes()
    }

    /// (tag, min, default, max, flags, name_id) — для синтетических axes
    /// в тестах. Type alias чтобы успокоить clippy::type-complexity.
    type AxisSpec<'a> = (&'a [u8; 4], f32, f32, f32, u16, u16);

    /// (subfamily_name_id, flags, coordinates, post_script_name_id) — для
    /// синтетических instances. `post_script_name_id` = None означает «не
    /// записывать postScriptNameID» (instanceSize без него).
    type InstSpec = (u16, u16, Vec<f32>, Option<u16>);

    fn build_fvar(axes: &[AxisSpec<'_>]) -> Vec<u8> {
        build_fvar_with_instances(axes, &[])
    }

    /// Минимальная корректная fvar с указанным набором axes и instances.
    /// `instanceSize` определяется по первому instance: если у первого есть
    /// postScriptNameID — `6 + 4*axisCount`, иначе `4 + 4*axisCount`. Все
    /// instances должны быть гомогенны (per spec).
    fn build_fvar_with_instances(
        axes: &[AxisSpec<'_>],
        instances: &[InstSpec],
    ) -> Vec<u8> {
        let header_size = 16u16;
        let axis_size = 20u16;
        let axis_count = axes.len() as u16;
        let coord_bytes = 4u16 * axis_count;
        let with_psid = instances.first().is_some_and(|i| i.3.is_some());
        let instance_size = if instances.is_empty() {
            0
        } else if with_psid {
            6 + coord_bytes
        } else {
            4 + coord_bytes
        };
        let instance_count = instances.len() as u16;
        let mut out = Vec::with_capacity(
            header_size as usize
                + axes.len() * axis_size as usize
                + instances.len() * instance_size as usize,
        );
        out.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
        out.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
        out.extend_from_slice(&header_size.to_be_bytes()); // axesArrayOffset
        out.extend_from_slice(&2u16.to_be_bytes()); // reserved (countSizePairs)
        out.extend_from_slice(&axis_count.to_be_bytes()); // axisCount
        out.extend_from_slice(&axis_size.to_be_bytes()); // axisSize
        out.extend_from_slice(&instance_count.to_be_bytes());
        out.extend_from_slice(&instance_size.to_be_bytes());
        for (tag, min, default, max, flags, name_id) in axes {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&put_fixed(*min));
            out.extend_from_slice(&put_fixed(*default));
            out.extend_from_slice(&put_fixed(*max));
            out.extend_from_slice(&flags.to_be_bytes());
            out.extend_from_slice(&name_id.to_be_bytes());
        }
        for (subfamily_name_id, flags, coords, post_script_name_id) in instances {
            assert_eq!(coords.len(), axes.len(), "coords must match axes count");
            out.extend_from_slice(&subfamily_name_id.to_be_bytes());
            out.extend_from_slice(&flags.to_be_bytes());
            for c in coords {
                out.extend_from_slice(&put_fixed(*c));
            }
            if with_psid {
                let psid = post_script_name_id
                    .expect("post_script_name_id required when instanceSize includes PSID");
                out.extend_from_slice(&psid.to_be_bytes());
            }
        }
        out
    }

    #[test]
    fn parses_empty_fvar() {
        let data = build_fvar(&[]);
        let fvar = Fvar::parse(&data).unwrap();
        assert!(!fvar.is_variable());
        assert_eq!(fvar.axes.len(), 0);
    }

    #[test]
    fn parses_single_wght_axis() {
        let data = build_fvar(&[(b"wght", 100.0, 400.0, 900.0, 0, 256)]);
        let fvar = Fvar::parse(&data).unwrap();
        assert!(fvar.is_variable());
        assert_eq!(fvar.axes.len(), 1);
        let a = &fvar.axes[0];
        assert_eq!(&a.tag, b"wght");
        assert!((a.min - 100.0).abs() < 1e-3);
        assert!((a.default - 400.0).abs() < 1e-3);
        assert!((a.max - 900.0).abs() < 1e-3);
        assert_eq!(a.flags, 0);
        assert_eq!(a.axis_name_id, 256);
        assert!(!a.is_hidden());
    }

    #[test]
    fn parses_multiple_axes() {
        let data = build_fvar(&[
            (b"wght", 100.0, 400.0, 900.0, 0, 256),
            (b"wdth", 50.0, 100.0, 200.0, 0, 257),
            (b"slnt", -15.0, 0.0, 15.0, 0, 258),
        ]);
        let fvar = Fvar::parse(&data).unwrap();
        assert_eq!(fvar.axes.len(), 3);
        assert_eq!(&fvar.axes[0].tag, b"wght");
        assert_eq!(&fvar.axes[1].tag, b"wdth");
        assert_eq!(&fvar.axes[2].tag, b"slnt");
        // slnt с negative min — F16Dot16 sign-extension работает.
        assert!((fvar.axes[2].min - (-15.0)).abs() < 1e-3);
    }

    #[test]
    fn lookup_axis_by_tag() {
        let data = build_fvar(&[
            (b"wght", 100.0, 400.0, 900.0, 0, 256),
            (b"opsz", 8.0, 14.0, 144.0, 0, 257),
        ]);
        let fvar = Fvar::parse(&data).unwrap();
        let opsz = fvar.axis(b"opsz").expect("opsz axis");
        assert!((opsz.default - 14.0).abs() < 1e-3);
        assert!(fvar.axis(b"GRAD").is_none(), "несуществующий tag — None");
    }

    #[test]
    fn hidden_flag_recognised() {
        let data = build_fvar(&[(b"GRAD", -200.0, 0.0, 200.0, VariationAxis::FLAG_HIDDEN, 256)]);
        let fvar = Fvar::parse(&data).unwrap();
        assert!(fvar.axes[0].is_hidden());
    }

    #[test]
    fn clamp_below_min_returns_min() {
        let axis = VariationAxis {
            tag: *b"wght",
            min: 100.0,
            default: 400.0,
            max: 900.0,
            flags: 0,
            axis_name_id: 256,
        };
        assert!((axis.clamp(50.0) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn clamp_above_max_returns_max() {
        let axis = VariationAxis {
            tag: *b"wght",
            min: 100.0,
            default: 400.0,
            max: 900.0,
            flags: 0,
            axis_name_id: 256,
        };
        assert!((axis.clamp(1500.0) - 900.0).abs() < 1e-6);
    }

    #[test]
    fn clamp_in_range_returns_value() {
        let axis = VariationAxis {
            tag: *b"wght",
            min: 100.0,
            default: 400.0,
            max: 900.0,
            flags: 0,
            axis_name_id: 256,
        };
        assert!((axis.clamp(700.0) - 700.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_unsupported_major_version() {
        let mut data = build_fvar(&[(b"wght", 100.0, 400.0, 900.0, 0, 256)]);
        // Меняем majorVersion на 2.
        data[0] = 0;
        data[1] = 2;
        assert!(Fvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_wrong_axis_size() {
        let mut data = build_fvar(&[(b"wght", 100.0, 400.0, 900.0, 0, 256)]);
        // axisSize — на offset 10 (4×u16 в header до неё). Меняем 20 → 22.
        let pos = 10;
        data[pos] = 0;
        data[pos + 1] = 22;
        assert!(Fvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_table() {
        let data = build_fvar(&[(b"wght", 100.0, 400.0, 900.0, 0, 256)]);
        // Обрезаем последний axis record на 5 байт.
        let truncated = &data[..data.len() - 5];
        assert!(Fvar::parse(truncated).is_err());
    }

    #[test]
    fn fractional_axis_values_roundtrip() {
        let data = build_fvar(&[(b"slnt", -9.5, 0.25, 12.75, 0, 256)]);
        let fvar = Fvar::parse(&data).unwrap();
        // F16Dot16 (1/65536 precision) дробные round-trip-ятся точно.
        assert!((fvar.axes[0].min - (-9.5)).abs() < 1e-4);
        assert!((fvar.axes[0].default - 0.25).abs() < 1e-4);
        assert!((fvar.axes[0].max - 12.75).abs() < 1e-4);
    }

    #[test]
    fn parses_no_instances() {
        let data = build_fvar(&[(b"wght", 100.0, 400.0, 900.0, 0, 256)]);
        let fvar = Fvar::parse(&data).unwrap();
        assert!(fvar.instances.is_empty());
    }

    #[test]
    fn parses_single_instance_without_psid() {
        let data = build_fvar_with_instances(
            &[(b"wght", 100.0, 400.0, 900.0, 0, 256)],
            &[(258, 0, vec![700.0], None)],
        );
        let fvar = Fvar::parse(&data).unwrap();
        assert_eq!(fvar.instances.len(), 1);
        let inst = &fvar.instances[0];
        assert_eq!(inst.subfamily_name_id, 258);
        assert_eq!(inst.flags, 0);
        assert_eq!(inst.coordinates.len(), 1);
        assert!((inst.coordinates[0] - 700.0).abs() < 1e-3);
        assert!(inst.post_script_name_id.is_none());
    }

    #[test]
    fn parses_single_instance_with_psid() {
        let data = build_fvar_with_instances(
            &[(b"wght", 100.0, 400.0, 900.0, 0, 256)],
            &[(258, 0, vec![700.0], Some(259))],
        );
        let fvar = Fvar::parse(&data).unwrap();
        assert_eq!(fvar.instances.len(), 1);
        let inst = &fvar.instances[0];
        assert_eq!(inst.post_script_name_id, Some(259));
    }

    #[test]
    fn parses_multiple_instances_multi_axis() {
        // 2 оси (wght, wdth), 3 instances — Regular / Bold / Bold Condensed.
        let data = build_fvar_with_instances(
            &[
                (b"wght", 100.0, 400.0, 900.0, 0, 256),
                (b"wdth", 75.0, 100.0, 125.0, 0, 257),
            ],
            &[
                (258, 0, vec![400.0, 100.0], None),
                (259, 0, vec![700.0, 100.0], None),
                (260, 0, vec![700.0, 75.0], None),
            ],
        );
        let fvar = Fvar::parse(&data).unwrap();
        assert_eq!(fvar.instances.len(), 3);
        assert_eq!(fvar.instances[0].coordinates.len(), 2);
        assert!((fvar.instances[0].coordinates[0] - 400.0).abs() < 1e-3);
        assert!((fvar.instances[0].coordinates[1] - 100.0).abs() < 1e-3);
        assert!((fvar.instances[1].coordinates[0] - 700.0).abs() < 1e-3);
        assert!((fvar.instances[2].coordinates[1] - 75.0).abs() < 1e-3);
    }

    #[test]
    fn lookup_instance_by_name_id() {
        let data = build_fvar_with_instances(
            &[(b"wght", 100.0, 400.0, 900.0, 0, 256)],
            &[
                (258, 0, vec![400.0], None),
                (259, 0, vec![700.0], None),
            ],
        );
        let fvar = Fvar::parse(&data).unwrap();
        let bold = fvar.instance_by_name_id(259).expect("Bold instance");
        assert!((bold.coordinates[0] - 700.0).abs() < 1e-3);
        assert!(fvar.instance_by_name_id(999).is_none());
    }

    #[test]
    fn instance_flags_preserved() {
        let data = build_fvar_with_instances(
            &[(b"wght", 100.0, 400.0, 900.0, 0, 256)],
            // Per spec flags поле reserved; сохраняем raw для прозрачности.
            &[(258, 0xBEEF, vec![500.0], None)],
        );
        let fvar = Fvar::parse(&data).unwrap();
        assert_eq!(fvar.instances[0].flags, 0xBEEF);
    }

    #[test]
    fn rejects_wrong_instance_size() {
        let mut data = build_fvar_with_instances(
            &[(b"wght", 100.0, 400.0, 900.0, 0, 256)],
            &[(258, 0, vec![700.0], None)],
        );
        // instanceSize — на offset 14 (header_size=16, последние 2 байта).
        // Spec: 4 + 4*axisCount = 8 для одной оси. Меняем на 9 (невалидно).
        data[14] = 0;
        data[15] = 9;
        assert!(Fvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_instances() {
        let data = build_fvar_with_instances(
            &[(b"wght", 100.0, 400.0, 900.0, 0, 256)],
            &[(258, 0, vec![700.0], None)],
        );
        // Срезаем последние 4 байта (часть instance record).
        let truncated = &data[..data.len() - 4];
        assert!(Fvar::parse(truncated).is_err());
    }

    #[test]
    fn parses_fvar_with_zero_instance_size_when_no_instances() {
        // Real-world Inter v4 имеет instanceCount=0, instanceSize=0 —
        // parser должен это принимать без проверки instanceSize.
        let data = build_fvar(&[
            (b"wght", 100.0, 400.0, 900.0, 0, 256),
            (b"opsz", 8.0, 14.0, 144.0, 0, 257),
        ]);
        let fvar = Fvar::parse(&data).unwrap();
        assert_eq!(fvar.axes.len(), 2);
        assert!(fvar.instances.is_empty());
    }
}
