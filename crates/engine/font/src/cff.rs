//! `CFF ` table — Compact Font Format (PostScript Type 2 outlines).
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/cff>
//! and Adobe TN#5176 (CFF) + TN#5177 (Type 2 charstrings).
//!
//! OpenType fonts with `'OTTO'` sfnt version store glyph outlines in a `CFF `
//! table instead of `glyf`/`loca`. Outlines are described by Type 2
//! charstrings — a stack-based mini-language of relative moves, lines and
//! **cubic** Béziers (TrueType uses quadratic). To reuse the existing
//! quadratic-aware [`crate::rasterizer::Rasterizer`], cubics are flattened
//! into short on-curve line segments at parse time; the resulting
//! [`crate::glyf::Glyph`] is always an `Outline::Simple` of all-on-curve
//! points, indistinguishable to the rasterizer from a flattened TrueType glyph.
//!
//! Supported:
//! - CFF header + Name/TopDICT/String/GlobalSubr INDEX parsing.
//! - Private DICT (local Subr offset, default/nominal width — width is parsed
//!   only to keep operand parity, advances still come from `hmtx`).
//! - Type 2 charstring interpreter: all path operators (move/line/curve in
//!   every shorthand), hint operators (counted for `hintmask` byte skipping),
//!   `callsubr`/`callgsubr`/`return` with the standard subr bias, the four
//!   flex operators, and `endchar` (incl. `seac`-style accented composites).
//! - CID-keyed CFF: `ROS` + `FDArray` + `FDSelect` (formats 0 and 3) so the
//!   correct per-FD local subrs are used.
//!
//! Not supported (deferred): CFF2 (variable PostScript outlines), charstring
//! arithmetic/storage operators (`put`/`get`/`add`/...), hint replacement
//! semantics beyond byte skipping, and font-matrix scaling other than the
//! default 1/1000 em (handled via `head.units_per_em`, not the CFF FontMatrix).

use crate::binary::BinaryReader;
use crate::face::FontError;
use crate::glyf::{BoundingBox, Contour, Glyph, Outline, OutlinePoint};

const CFF_TAG: [u8; 4] = *b"CFF ";

/// A CFF INDEX: a count-prefixed array of variable-length objects.
///
/// Stores the absolute byte ranges of each object inside the parent CFF data
/// slice, plus a reference to that slice, so individual objects can be sliced
/// lazily via [`Index::get`].
#[derive(Debug, Clone)]
struct Index<'a> {
    /// Absolute offsets into `data` for each object boundary; length is
    /// `count + 1`. Object `i` spans `offsets[i]..offsets[i + 1]`.
    offsets: Vec<usize>,
    data: &'a [u8],
}

impl<'a> Index<'a> {
    /// Empty INDEX (count == 0).
    fn empty(data: &'a [u8]) -> Self {
        Self {
            offsets: vec![0],
            data,
        }
    }

    /// Number of objects.
    fn count(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    /// Object `i`, or `None` if out of range / out of bounds.
    fn get(&self, i: usize) -> Option<&'a [u8]> {
        let start = *self.offsets.get(i)?;
        let end = *self.offsets.get(i + 1)?;
        self.data.get(start..end)
    }

    /// Parse an INDEX starting at `pos` in `data`. Returns the parsed INDEX and
    /// the position immediately after it (start of the next structure).
    fn parse(data: &'a [u8], pos: usize) -> Result<(Self, usize), FontError> {
        let mut r = BinaryReader::new(data);
        r.seek(pos);
        let count = r.read_u16().ok_or(FontError::UnexpectedEof)? as usize;
        if count == 0 {
            // Empty INDEX occupies just the 2 count bytes.
            return Ok((Self::empty(data), pos + 2));
        }
        let off_size = r.read_u8().ok_or(FontError::UnexpectedEof)? as usize;
        if !(1..=4).contains(&off_size) {
            return Err(FontError::InvalidTable(CFF_TAG));
        }
        // Offsets are 1-based relative to the byte preceding the object data.
        let mut raw_offsets = Vec::with_capacity(count + 1);
        for _ in 0..=count {
            raw_offsets.push(read_offset(&mut r, off_size)?);
        }
        // Data block base: byte before it is `r.position() - 1` (offset 1 → first byte).
        let data_base = r.position().checked_sub(1).ok_or(FontError::UnexpectedEof)?;
        let mut offsets = Vec::with_capacity(count + 1);
        for raw in &raw_offsets {
            let abs = data_base
                .checked_add(*raw as usize)
                .ok_or(FontError::UnexpectedEof)?;
            if abs > data.len() {
                return Err(FontError::InvalidTable(CFF_TAG));
            }
            offsets.push(abs);
        }
        let end = *offsets.last().unwrap();
        Ok((Self { offsets, data }, end))
    }
}

/// Read an `off_size`-byte big-endian offset (1..=4 bytes).
fn read_offset(r: &mut BinaryReader, off_size: usize) -> Result<u32, FontError> {
    let mut v: u32 = 0;
    for _ in 0..off_size {
        v = (v << 8) | r.read_u8().ok_or(FontError::UnexpectedEof)? as u32;
    }
    Ok(v)
}

/// A parsed CFF DICT: operator → operands. Operators are encoded as `op` for
/// single-byte operators and `1200 + b` for the two-byte escape operator
/// `12 b`.
#[derive(Debug, Default, Clone)]
struct Dict {
    entries: Vec<(u16, Vec<f64>)>,
}

impl Dict {
    /// Operands of the last occurrence of operator `op`, or `None`.
    fn get(&self, op: u16) -> Option<&[f64]> {
        self.entries
            .iter()
            .rev()
            .find(|(o, _)| *o == op)
            .map(|(_, v)| v.as_slice())
    }

    /// First operand of `op` as an integer, or `None`.
    fn get_int(&self, op: u16) -> Option<i64> {
        self.get(op).and_then(|v| v.first()).map(|f| *f as i64)
    }

    /// Parse a DICT from a byte slice (operands precede their operator).
    fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut entries = Vec::new();
        let mut operands: Vec<f64> = Vec::new();
        let mut i = 0usize;
        while i < data.len() {
            let b0 = data[i];
            match b0 {
                0..=21 => {
                    // Operator. 12 is a two-byte escape.
                    let op = if b0 == 12 {
                        i += 1;
                        let b1 = *data.get(i).ok_or(FontError::UnexpectedEof)?;
                        1200 + b1 as u16
                    } else {
                        b0 as u16
                    };
                    i += 1;
                    entries.push((op, std::mem::take(&mut operands)));
                }
                28 => {
                    let hi = *data.get(i + 1).ok_or(FontError::UnexpectedEof)?;
                    let lo = *data.get(i + 2).ok_or(FontError::UnexpectedEof)?;
                    operands.push(i16::from_be_bytes([hi, lo]) as f64);
                    i += 3;
                }
                29 => {
                    let b = data
                        .get(i + 1..i + 5)
                        .ok_or(FontError::UnexpectedEof)?;
                    operands.push(i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64);
                    i += 5;
                }
                30 => {
                    // Real number: packed BCD nibbles, terminated by nibble 0xf.
                    let (val, consumed) = parse_real(&data[i + 1..])?;
                    operands.push(val);
                    i += 1 + consumed;
                }
                32..=246 => {
                    operands.push(b0 as f64 - 139.0);
                    i += 1;
                }
                247..=250 => {
                    let b1 = *data.get(i + 1).ok_or(FontError::UnexpectedEof)? as f64;
                    operands.push((b0 as f64 - 247.0) * 256.0 + b1 + 108.0);
                    i += 2;
                }
                251..=254 => {
                    let b1 = *data.get(i + 1).ok_or(FontError::UnexpectedEof)? as f64;
                    operands.push(-(b0 as f64 - 251.0) * 256.0 - b1 - 108.0);
                    i += 2;
                }
                // 22..=27, 31, 255 are reserved in DICTs — skip defensively.
                _ => {
                    i += 1;
                }
            }
        }
        Ok(Self { entries })
    }
}

/// Parse a CFF DICT real (operator 30) from the nibble-packed bytes following
/// the `30` marker. Returns the value and the number of bytes consumed.
fn parse_real(data: &[u8]) -> Result<(f64, usize), FontError> {
    let mut s = String::new();
    let mut consumed = 0usize;
    'outer: for &byte in data {
        consumed += 1;
        for nibble in [byte >> 4, byte & 0x0f] {
            match nibble {
                0..=9 => s.push((b'0' + nibble) as char),
                0xa => s.push('.'),
                0xb => s.push('E'),
                0xc => s.push_str("E-"),
                0xe => s.push('-'),
                0xf => break 'outer,
                _ => {} // 0xd is reserved; ignore
            }
        }
    }
    let val = s.parse::<f64>().unwrap_or(0.0);
    Ok((val, consumed))
}

/// FDSelect: maps each glyph to its font-DICT index (CID-keyed CFF only).
#[derive(Debug, Clone)]
enum FdSelect {
    /// Format 0: one byte per glyph.
    Format0(Vec<u8>),
    /// Format 3: ranges of `(first_glyph, fd)` plus a sentinel.
    Format3 { ranges: Vec<(u16, u8)>, sentinel: u16 },
}

impl FdSelect {
    fn fd_for(&self, gid: u16) -> u8 {
        match self {
            Self::Format0(v) => v.get(gid as usize).copied().unwrap_or(0),
            Self::Format3 { ranges, sentinel } => {
                if gid >= *sentinel {
                    return 0;
                }
                // Ranges are sorted by first glyph; find the last whose start <= gid.
                let mut fd = 0u8;
                for &(first, f) in ranges {
                    if first <= gid {
                        fd = f;
                    } else {
                        break;
                    }
                }
                fd
            }
        }
    }

    fn parse(data: &[u8], offset: usize, num_glyphs: usize) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        r.seek(offset);
        let format = r.read_u8().ok_or(FontError::UnexpectedEof)?;
        match format {
            0 => {
                let mut v = Vec::with_capacity(num_glyphs);
                for _ in 0..num_glyphs {
                    v.push(r.read_u8().ok_or(FontError::UnexpectedEof)?);
                }
                Ok(Self::Format0(v))
            }
            3 => {
                let n_ranges = r.read_u16().ok_or(FontError::UnexpectedEof)? as usize;
                let mut ranges = Vec::with_capacity(n_ranges);
                for _ in 0..n_ranges {
                    let first = r.read_u16().ok_or(FontError::UnexpectedEof)?;
                    let fd = r.read_u8().ok_or(FontError::UnexpectedEof)?;
                    ranges.push((first, fd));
                }
                let sentinel = r.read_u16().ok_or(FontError::UnexpectedEof)?;
                Ok(Self::Format3 { ranges, sentinel })
            }
            _ => Err(FontError::InvalidTable(CFF_TAG)),
        }
    }
}

/// Per-FD or simple-font private state needed to render glyphs.
#[derive(Debug, Clone)]
enum Charset<'a> {
    /// Non-CID font: a single set of local subrs.
    Simple { local_subrs: Index<'a> },
    /// CID-keyed font: per-FD local subrs + glyph→FD mapping.
    Cid {
        local_subrs: Vec<Index<'a>>,
        fd_select: FdSelect,
    },
}

/// Parsed `CFF ` table ready to produce glyph outlines.
#[derive(Debug, Clone)]
pub struct Cff<'a> {
    char_strings: Index<'a>,
    global_subrs: Index<'a>,
    charset: Charset<'a>,
}

impl<'a> Cff<'a> {
    /// Number of glyphs (CharStrings INDEX count).
    pub fn num_glyphs(&self) -> usize {
        self.char_strings.count()
    }

    /// Parse a `CFF ` table from its raw bytes.
    pub fn parse(data: &'a [u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let _major = r.read_u8().ok_or(FontError::UnexpectedEof)?;
        let _minor = r.read_u8().ok_or(FontError::UnexpectedEof)?;
        let hdr_size = r.read_u8().ok_or(FontError::UnexpectedEof)? as usize;
        let _off_size = r.read_u8().ok_or(FontError::UnexpectedEof)?;

        // Name INDEX → Top DICT INDEX → String INDEX → Global Subr INDEX.
        let (_name_index, pos) = Index::parse(data, hdr_size)?;
        let (top_dict_index, pos) = Index::parse(data, pos)?;
        let (_string_index, pos) = Index::parse(data, pos)?;
        let (global_subrs, _pos) = Index::parse(data, pos)?;

        let top_bytes = top_dict_index
            .get(0)
            .ok_or(FontError::InvalidTable(CFF_TAG))?;
        let top_dict = Dict::parse(top_bytes)?;

        // CharStrings INDEX (op 17, absolute offset into CFF data).
        let cs_offset = top_dict
            .get_int(17)
            .ok_or(FontError::InvalidTable(CFF_TAG))? as usize;
        let (char_strings, _) = Index::parse(data, cs_offset)?;
        let num_glyphs = char_strings.count();

        let charset = if top_dict.get(1230).is_some() {
            // CID-keyed: FDArray (1236) + FDSelect (1237).
            let fd_array_off = top_dict
                .get_int(1236)
                .ok_or(FontError::InvalidTable(CFF_TAG))? as usize;
            let fd_select_off = top_dict
                .get_int(1237)
                .ok_or(FontError::InvalidTable(CFF_TAG))? as usize;
            let (fd_array, _) = Index::parse(data, fd_array_off)?;
            let mut local_subrs = Vec::with_capacity(fd_array.count());
            for i in 0..fd_array.count() {
                let fd_bytes = fd_array.get(i).ok_or(FontError::InvalidTable(CFF_TAG))?;
                let fd_dict = Dict::parse(fd_bytes)?;
                local_subrs.push(parse_local_subrs(data, &fd_dict)?);
            }
            let fd_select = FdSelect::parse(data, fd_select_off, num_glyphs)?;
            Charset::Cid {
                local_subrs,
                fd_select,
            }
        } else {
            // Non-CID: single Private DICT → local subrs.
            Charset::Simple {
                local_subrs: parse_local_subrs(data, &top_dict)?,
            }
        };

        Ok(Self {
            char_strings,
            global_subrs,
            charset,
        })
    }

    /// Local subrs INDEX for `glyph_id` (FD-dependent for CID fonts).
    fn local_subrs_for(&self, glyph_id: u16) -> &Index<'a> {
        match &self.charset {
            Charset::Simple { local_subrs } => local_subrs,
            Charset::Cid {
                local_subrs,
                fd_select,
            } => {
                let fd = fd_select.fd_for(glyph_id) as usize;
                local_subrs
                    .get(fd)
                    .unwrap_or_else(|| local_subrs.first().unwrap_or(local_subrs.last().unwrap()))
            }
        }
    }

    /// Glyph outline for `glyph_id`, or `None` if the glyph is empty (e.g.
    /// space) or `glyph_id` is out of range. The returned glyph is always
    /// `Outline::Simple` with all points on-curve (cubics pre-flattened), and
    /// a bounding box computed from the flattened points.
    pub fn glyph(&self, glyph_id: u16) -> Result<Option<Glyph>, FontError> {
        let Some(charstring) = self.char_strings.get(glyph_id as usize) else {
            return Ok(None);
        };
        let local_subrs = self.local_subrs_for(glyph_id);
        let mut interp = Type2Interp::new(&self.global_subrs, local_subrs);
        interp.run(charstring, self)?;
        let contours = interp.finish();
        if contours.is_empty() {
            return Ok(None);
        }
        let bbox = compute_bbox(&contours);
        Ok(Some(Glyph {
            bbox,
            outline: Outline::Simple(contours),
        }))
    }
}

/// Parse the local Subr INDEX referenced by a (Top or FD) DICT's Private entry.
/// Returns an empty INDEX if there is no Private DICT or no local subrs.
fn parse_local_subrs<'a>(data: &'a [u8], dict: &Dict) -> Result<Index<'a>, FontError> {
    // Private (op 18) = [size, offset] (absolute offset into CFF data).
    let Some(private) = dict.get(18) else {
        return Ok(Index::empty(data));
    };
    if private.len() < 2 {
        return Ok(Index::empty(data));
    }
    let size = private[0] as usize;
    let offset = private[1] as usize;
    let priv_bytes = data
        .get(offset..offset.checked_add(size).ok_or(FontError::UnexpectedEof)?)
        .ok_or(FontError::InvalidTable(CFF_TAG))?;
    let priv_dict = Dict::parse(priv_bytes)?;
    // Subrs (op 19): offset is relative to the start of the Private DICT.
    match priv_dict.get_int(19) {
        Some(subr_rel) => {
            let subr_off = offset
                .checked_add(subr_rel as usize)
                .ok_or(FontError::UnexpectedEof)?;
            let (index, _) = Index::parse(data, subr_off)?;
            Ok(index)
        }
        None => Ok(Index::empty(data)),
    }
}

/// Compute a bounding box from flattened contours (all points on-curve).
fn compute_bbox(contours: &[Contour]) -> BoundingBox {
    let mut x_min = i16::MAX;
    let mut y_min = i16::MAX;
    let mut x_max = i16::MIN;
    let mut y_max = i16::MIN;
    let mut any = false;
    for c in contours {
        for p in &c.points {
            any = true;
            x_min = x_min.min(p.x);
            y_min = y_min.min(p.y);
            x_max = x_max.max(p.x);
            y_max = y_max.max(p.y);
        }
    }
    if !any {
        return BoundingBox {
            x_min: 0,
            y_min: 0,
            x_max: 0,
            y_max: 0,
        };
    }
    BoundingBox {
        x_min,
        y_min,
        x_max,
        y_max,
    }
}

/// Subroutine index bias per the Type 2 charstring spec (TN#5177 §16).
fn subr_bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

/// Number of line segments a cubic Bézier is flattened into. Mirrors the
/// quadratic flattener's fixed-step approach in `rasterizer.rs`; cubics curve
/// more sharply, so a slightly higher count keeps text edges smooth without
/// adaptive subdivision.
const CUBIC_STEPS: usize = 10;

/// Maximum subroutine call depth (TN#5177 caps nesting at 10).
const MAX_SUBR_DEPTH: u32 = 10;

/// Type 2 charstring interpreter. Accumulates outline contours as it executes;
/// cubics are flattened to on-curve line segments so the result feeds the
/// existing quadratic rasterizer unchanged.
struct Type2Interp<'a, 'b> {
    stack: Vec<f64>,
    x: f64,
    y: f64,
    contours: Vec<Contour>,
    current: Vec<OutlinePoint>,
    /// Total stem hints seen so far (drives `hintmask`/`cntrmask` byte counts).
    n_stems: u32,
    /// Whether the optional leading width operand has been consumed.
    width_parsed: bool,
    /// Whether the interpreter has hit `endchar`.
    done: bool,
    global_subrs: &'b Index<'a>,
    local_subrs: &'b Index<'a>,
    global_bias: i32,
    local_bias: i32,
    depth: u32,
}

impl<'a, 'b> Type2Interp<'a, 'b> {
    fn new(global_subrs: &'b Index<'a>, local_subrs: &'b Index<'a>) -> Self {
        Self {
            stack: Vec::new(),
            x: 0.0,
            y: 0.0,
            contours: Vec::new(),
            current: Vec::new(),
            n_stems: 0,
            width_parsed: false,
            done: false,
            global_subrs,
            local_subrs,
            global_bias: subr_bias(global_subrs.count()),
            local_bias: subr_bias(local_subrs.count()),
            depth: 0,
        }
    }

    /// Finish the current contour and return all accumulated contours.
    fn finish(mut self) -> Vec<Contour> {
        self.close_contour();
        self.contours
    }

    /// Push the in-progress contour (if any) to the contour list.
    fn close_contour(&mut self) {
        if self.current.len() >= 2 {
            self.contours.push(Contour {
                points: std::mem::take(&mut self.current),
            });
        } else {
            self.current.clear();
        }
    }

    /// Emit the current pen position as an on-curve point.
    fn emit(&mut self) {
        self.current.push(OutlinePoint {
            x: self.x.round() as i16,
            y: self.y.round() as i16,
            on_curve: true,
        });
    }

    /// Begin a new contour at the current pen position.
    fn move_to(&mut self) {
        self.close_contour();
        self.emit();
    }

    /// Relative line to `(dx, dy)`.
    fn line_to(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        self.emit();
    }

    /// Relative cubic Bézier with control deltas, flattened to line segments.
    fn curve_to(&mut self, d: [f64; 6]) {
        let x0 = self.x;
        let y0 = self.y;
        let x1 = x0 + d[0];
        let y1 = y0 + d[1];
        let x2 = x1 + d[2];
        let y2 = y1 + d[3];
        let x3 = x2 + d[4];
        let y3 = y2 + d[5];
        for i in 1..=CUBIC_STEPS {
            let t = i as f64 / CUBIC_STEPS as f64;
            let mt = 1.0 - t;
            let a = mt * mt * mt;
            let b = 3.0 * mt * mt * t;
            let c = 3.0 * mt * t * t;
            let e = t * t * t;
            let px = a * x0 + b * x1 + c * x2 + e * x3;
            let py = a * y0 + b * y1 + c * y2 + e * y3;
            self.current.push(OutlinePoint {
                x: px.round() as i16,
                y: py.round() as i16,
                on_curve: true,
            });
        }
        self.x = x3;
        self.y = y3;
    }

    /// Consume the optional leading width operand before the first
    /// width-bearing operator. `even_args` is true when a width is present iff
    /// the operand count is odd (stem/hintmask); for move/endchar the caller
    /// passes the expected non-width count via `expected`.
    fn parse_width_stems(&mut self, even_expected_odd: bool) {
        if self.width_parsed {
            return;
        }
        self.width_parsed = true;
        if even_expected_odd && self.stack.len() % 2 == 1 {
            self.stack.remove(0);
        }
    }

    /// Drop a leading width operand if `stack.len()` exceeds `max_args`.
    fn parse_width_move(&mut self, max_args: usize) {
        if self.width_parsed {
            return;
        }
        self.width_parsed = true;
        if self.stack.len() > max_args {
            self.stack.remove(0);
        }
    }

    /// Run a charstring (recursively for subrs). `cff` is only used for `seac`
    /// resolution at `endchar`.
    fn run(&mut self, cs: &[u8], cff: &Cff<'a>) -> Result<(), FontError> {
        if self.depth > MAX_SUBR_DEPTH {
            return Ok(());
        }
        let mut i = 0usize;
        while i < cs.len() && !self.done {
            let b0 = cs[i];
            i += 1;
            match b0 {
                // ── Numbers ───────────────────────────────────────────────
                28 => {
                    let hi = *cs.get(i).ok_or(FontError::UnexpectedEof)?;
                    let lo = *cs.get(i + 1).ok_or(FontError::UnexpectedEof)?;
                    self.stack.push(i16::from_be_bytes([hi, lo]) as f64);
                    i += 2;
                }
                32..=246 => self.stack.push(b0 as f64 - 139.0),
                247..=250 => {
                    let b1 = *cs.get(i).ok_or(FontError::UnexpectedEof)? as f64;
                    self.stack.push((b0 as f64 - 247.0) * 256.0 + b1 + 108.0);
                    i += 1;
                }
                251..=254 => {
                    let b1 = *cs.get(i).ok_or(FontError::UnexpectedEof)? as f64;
                    self.stack.push(-(b0 as f64 - 251.0) * 256.0 - b1 - 108.0);
                    i += 1;
                }
                255 => {
                    let b = cs.get(i..i + 4).ok_or(FontError::UnexpectedEof)?;
                    let raw = i32::from_be_bytes([b[0], b[1], b[2], b[3]]);
                    self.stack.push(raw as f64 / 65536.0);
                    i += 4;
                }
                // ── Hint operators ────────────────────────────────────────
                1 | 3 | 18 | 23 => {
                    // hstem / vstem / hstemhm / vstemhm
                    self.parse_width_stems(true);
                    self.n_stems += (self.stack.len() / 2) as u32;
                    self.stack.clear();
                }
                19 | 20 => {
                    // hintmask / cntrmask: also implicitly ends vstem args.
                    self.parse_width_stems(true);
                    self.n_stems += (self.stack.len() / 2) as u32;
                    self.stack.clear();
                    let mask_bytes = self.n_stems.div_ceil(8) as usize;
                    i += mask_bytes;
                }
                // ── Path construction ─────────────────────────────────────
                21 => {
                    // rmoveto
                    self.parse_width_move(2);
                    let dy = self.stack.pop().unwrap_or(0.0);
                    let dx = self.stack.pop().unwrap_or(0.0);
                    self.x += dx;
                    self.y += dy;
                    self.move_to();
                    self.stack.clear();
                }
                22 => {
                    // hmoveto
                    self.parse_width_move(1);
                    let dx = self.stack.pop().unwrap_or(0.0);
                    self.x += dx;
                    self.move_to();
                    self.stack.clear();
                }
                4 => {
                    // vmoveto
                    self.parse_width_move(1);
                    let dy = self.stack.pop().unwrap_or(0.0);
                    self.y += dy;
                    self.move_to();
                    self.stack.clear();
                }
                5 => {
                    // rlineto: { dxa dya }+
                    let args = std::mem::take(&mut self.stack);
                    let mut j = 0;
                    while j + 1 < args.len() {
                        self.line_to(args[j], args[j + 1]);
                        j += 2;
                    }
                }
                6 => self.h_v_lineto(true),  // hlineto
                7 => self.h_v_lineto(false), // vlineto
                8 => {
                    // rrcurveto: { dxa dya dxb dyb dxc dyc }+
                    let args = std::mem::take(&mut self.stack);
                    let mut j = 0;
                    while j + 5 < args.len() {
                        self.curve_to([
                            args[j],
                            args[j + 1],
                            args[j + 2],
                            args[j + 3],
                            args[j + 4],
                            args[j + 5],
                        ]);
                        j += 6;
                    }
                }
                24 => {
                    // rcurveline: { dxa dya dxb dyb dxc dyc }+ dxd dyd
                    let args = std::mem::take(&mut self.stack);
                    let mut j = 0;
                    while j + 5 < args.len().saturating_sub(2) {
                        self.curve_to([
                            args[j],
                            args[j + 1],
                            args[j + 2],
                            args[j + 3],
                            args[j + 4],
                            args[j + 5],
                        ]);
                        j += 6;
                    }
                    if j + 1 < args.len() {
                        self.line_to(args[j], args[j + 1]);
                    }
                }
                25 => {
                    // rlinecurve: { dxa dya }+ dxb dyb dxc dyc dxd dyd
                    let args = std::mem::take(&mut self.stack);
                    let mut j = 0;
                    while j + 1 < args.len().saturating_sub(6) {
                        self.line_to(args[j], args[j + 1]);
                        j += 2;
                    }
                    if j + 5 < args.len() {
                        self.curve_to([
                            args[j],
                            args[j + 1],
                            args[j + 2],
                            args[j + 3],
                            args[j + 4],
                            args[j + 5],
                        ]);
                    }
                }
                26 => self.vv_curveto(),
                27 => self.hh_curveto(),
                30 => self.vh_hv_curveto(false), // vhcurveto (starts vertical)
                31 => self.vh_hv_curveto(true),  // hvcurveto (starts horizontal)
                // ── Subroutines ───────────────────────────────────────────
                10 => {
                    // callsubr (local)
                    if let Some(idx) = self.stack.pop() {
                        let n = idx as i32 + self.local_bias;
                        if n >= 0
                            && let Some(sub) = self.local_subrs.get(n as usize)
                        {
                            self.depth += 1;
                            self.run(sub, cff)?;
                            self.depth -= 1;
                        }
                    }
                }
                29 => {
                    // callgsubr (global)
                    if let Some(idx) = self.stack.pop() {
                        let n = idx as i32 + self.global_bias;
                        if n >= 0
                            && let Some(sub) = self.global_subrs.get(n as usize)
                        {
                            self.depth += 1;
                            self.run(sub, cff)?;
                            self.depth -= 1;
                        }
                    }
                }
                11 => return Ok(()), // return
                14 => {
                    // endchar (optional seac: adx ady bchar achar).
                    self.parse_width_endchar();
                    if self.stack.len() >= 4 {
                        let achar = self.stack.pop().unwrap() as u8;
                        let bchar = self.stack.pop().unwrap() as u8;
                        let ady = self.stack.pop().unwrap();
                        let adx = self.stack.pop().unwrap();
                        self.apply_seac(adx, ady, bchar, achar, cff)?;
                    }
                    self.done = true;
                    return Ok(());
                }
                12 => {
                    // Escape: two-byte operators (flex family + arithmetic).
                    let b1 = *cs.get(i).ok_or(FontError::UnexpectedEof)?;
                    i += 1;
                    self.escape_op(b1);
                }
                // 0, 2, 9 (dotsection legacy), 13, 15, 16, 17 — no path effect.
                _ => self.stack.clear(),
            }
        }
        Ok(())
    }

    /// Consume the optional leading width before `endchar`. Width is present
    /// when the arg count is 1 (width only) or 5 (width + seac's 4 args).
    fn parse_width_endchar(&mut self) {
        if self.width_parsed {
            return;
        }
        self.width_parsed = true;
        let n = self.stack.len();
        if n == 1 || n == 5 {
            self.stack.remove(0);
        }
    }

    /// Resolve a `seac`-style accented composite: render base glyph `bchar`
    /// then accent glyph `achar` offset by `(adx, ady)`, both via Standard
    /// Encoding → GID. Accent/base are looked up by Standard Encoding code,
    /// which for non-CID Latin fonts maps code → glyph id directly enough for
    /// the common accented Latin range. Failures are silently skipped.
    fn apply_seac(
        &mut self,
        adx: f64,
        ady: f64,
        bchar: u8,
        achar: u8,
        cff: &Cff<'a>,
    ) -> Result<(), FontError> {
        // Standard Encoding code → glyph id is font-specific; we approximate by
        // using the code as the glyph id, which holds for fonts whose charset
        // is the standard ordering. This is a best-effort legacy path.
        for (code, ox, oy) in [(bchar, 0.0, 0.0), (achar, adx, ady)] {
            let gid = code as u16;
            if let Some(g) = cff.glyph(gid)?
                && let Outline::Simple(sub) = g.outline
            {
                for c in sub {
                    let pts = c
                        .points
                        .into_iter()
                        .map(|p| OutlinePoint {
                            x: (p.x as f64 + ox).round() as i16,
                            y: (p.y as f64 + oy).round() as i16,
                            on_curve: p.on_curve,
                        })
                        .collect();
                    self.contours.push(Contour { points: pts });
                }
            }
        }
        Ok(())
    }

    /// hlineto / vlineto: alternating horizontal/vertical lines.
    fn h_v_lineto(&mut self, mut horizontal: bool) {
        let args = std::mem::take(&mut self.stack);
        for &d in &args {
            if horizontal {
                self.line_to(d, 0.0);
            } else {
                self.line_to(0.0, d);
            }
            horizontal = !horizontal;
        }
    }

    /// vvcurveto: { dxa? { dya dxb dyb dyc }+ } — curves with vertical tangents.
    fn vv_curveto(&mut self) {
        let args = std::mem::take(&mut self.stack);
        let mut j = 0;
        let dx1 = if args.len() % 4 == 1 {
            j = 1;
            args[0]
        } else {
            0.0
        };
        let mut first = true;
        while j + 3 < args.len() {
            let dxa = if first { dx1 } else { 0.0 };
            first = false;
            self.curve_to([dxa, args[j], args[j + 1], args[j + 2], 0.0, args[j + 3]]);
            j += 4;
        }
    }

    /// hhcurveto: { dy1? { dxa dxb dyb dxc }+ } — curves with horizontal tangents.
    fn hh_curveto(&mut self) {
        let args = std::mem::take(&mut self.stack);
        let mut j = 0;
        let dy1 = if args.len() % 4 == 1 {
            j = 1;
            args[0]
        } else {
            0.0
        };
        let mut first = true;
        while j + 3 < args.len() {
            let dya = if first { dy1 } else { 0.0 };
            first = false;
            self.curve_to([args[j], dya, args[j + 1], args[j + 2], args[j + 3], 0.0]);
            j += 4;
        }
    }

    /// hvcurveto / vhcurveto: alternating tangent direction per curve, with an
    /// optional trailing fifth operand on the last curve. `start_horizontal`
    /// selects which tangent the first curve uses.
    fn vh_hv_curveto(&mut self, start_horizontal: bool) {
        let args = std::mem::take(&mut self.stack);
        let n = args.len();
        let mut j = 0;
        let mut horizontal = start_horizontal;
        while j + 4 <= n {
            // A trailing odd operand (df) applies only to the final curve.
            let last = j + 8 > n;
            let df = if last && (n - j) == 5 { args[j + 4] } else { 0.0 };
            if horizontal {
                // hv: dx1 dx2 dy2 dy3 [dxf]
                self.curve_to([args[j], 0.0, args[j + 1], args[j + 2], df, args[j + 3]]);
            } else {
                // vh: dy1 dx2 dy2 dx3 [dyf]
                self.curve_to([0.0, args[j], args[j + 1], args[j + 2], args[j + 3], df]);
            }
            horizontal = !horizontal;
            j += 4;
        }
    }

    /// Two-byte escape operators: the four flex variants build two consecutive
    /// cubics; arithmetic/storage operators are unsupported and clear the stack.
    fn escape_op(&mut self, b1: u8) {
        match b1 {
            34 => {
                // hflex: dx1 dx2 dy2 dx3 dx4 dx5 dx6
                let a = std::mem::take(&mut self.stack);
                if a.len() >= 7 {
                    self.curve_to([a[0], 0.0, a[1], a[2], a[3], 0.0]);
                    self.curve_to([a[4], 0.0, a[5], -a[2], a[6], 0.0]);
                }
            }
            35 => {
                // flex: dx1 dy1 dx2 dy2 dx3 dy3 dx4 dy4 dx5 dy5 dx6 dy6 fd
                let a = std::mem::take(&mut self.stack);
                if a.len() >= 12 {
                    self.curve_to([a[0], a[1], a[2], a[3], a[4], a[5]]);
                    self.curve_to([a[6], a[7], a[8], a[9], a[10], a[11]]);
                }
            }
            36 => {
                // hflex1: dx1 dy1 dx2 dy2 dx3 dx4 dx5 dy5 dx6
                let a = std::mem::take(&mut self.stack);
                if a.len() >= 9 {
                    self.curve_to([a[0], a[1], a[2], a[3], a[4], 0.0]);
                    let dy = -(a[1] + a[3] + a[7]);
                    self.curve_to([a[5], 0.0, a[6], a[7], a[8], dy]);
                }
            }
            37 => {
                // flex1: dx1 dy1 dx2 dy2 dx3 dy3 dx4 dy4 dx5 dy5 d6
                let a = std::mem::take(&mut self.stack);
                if a.len() >= 11 {
                    let dx = a[0] + a[2] + a[4] + a[6] + a[8];
                    let dy = a[1] + a[3] + a[5] + a[7] + a[9];
                    self.curve_to([a[0], a[1], a[2], a[3], a[4], a[5]]);
                    if dx.abs() > dy.abs() {
                        self.curve_to([a[6], a[7], a[8], a[9], a[10], -dy]);
                    } else {
                        self.curve_to([a[6], a[7], a[8], a[9], -dx, a[10]]);
                    }
                }
            }
            _ => self.stack.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a CFF INDEX from a list of objects, matching [`Index::parse`].
    fn build_index(objects: &[&[u8]]) -> Vec<u8> {
        if objects.is_empty() {
            return vec![0, 0];
        }
        let count = objects.len() as u16;
        let mut offsets = vec![1u32];
        for o in objects {
            offsets.push(offsets.last().unwrap() + o.len() as u32);
        }
        let max = *offsets.last().unwrap();
        let off_size = if max < 0x100 {
            1
        } else if max < 0x10000 {
            2
        } else if max < 0x100_0000 {
            3
        } else {
            4
        };
        let mut out = Vec::new();
        out.extend_from_slice(&count.to_be_bytes());
        out.push(off_size as u8);
        for off in &offsets {
            let bytes = off.to_be_bytes();
            out.extend_from_slice(&bytes[4 - off_size..]);
        }
        for o in objects {
            out.extend_from_slice(o);
        }
        out
    }

    /// Encode a DICT integer operand using the 5-byte (op 29) form, so DICT
    /// sizes are fixed regardless of value.
    fn enc_off(v: u32) -> Vec<u8> {
        let mut out = vec![29u8];
        out.extend_from_slice(&v.to_be_bytes());
        out
    }

    /// Assemble a minimal, valid CFF table from charstrings + optional subrs.
    fn build_cff(
        charstrings: &[&[u8]],
        global_subrs: &[&[u8]],
        local_subrs: &[&[u8]],
    ) -> Vec<u8> {
        let header = [1u8, 0, 4, 1];
        let name = build_index(&[b"Test"]);
        let string = build_index(&[]);
        let global = build_index(global_subrs);
        let cs_index = build_index(charstrings);
        let local_index = build_index(local_subrs);

        // Private DICT: includes a Subrs (op 19) entry only when local subrs
        // exist. Subrs offset is relative to the Private DICT start; the local
        // subr INDEX is placed immediately after the Private DICT.
        let private_dict: Vec<u8> = if local_subrs.is_empty() {
            Vec::new()
        } else {
            // private dict = enc_off(rel) + [19]; its own length is 6, so the
            // relative offset to the trailing local subr INDEX is 6.
            let mut d = enc_off(6);
            d.push(19);
            d
        };
        let private_size = private_dict.len() as u32;

        // Top DICT INDEX is a fixed 22 bytes (17-byte dict, offSize 1).
        const TOP_DICT_INDEX_LEN: u32 = 22;
        let cs_offset = 4 + name.len() as u32 + TOP_DICT_INDEX_LEN + string.len() as u32
            + global.len() as u32;
        let private_offset = cs_offset + cs_index.len() as u32;

        let mut top_dict = Vec::new();
        top_dict.extend_from_slice(&enc_off(cs_offset)); // CharStrings operand
        top_dict.push(17); // CharStrings operator
        top_dict.extend_from_slice(&enc_off(private_size)); // Private size
        top_dict.extend_from_slice(&enc_off(private_offset)); // Private offset
        top_dict.push(18); // Private operator
        assert_eq!(top_dict.len(), 17, "top dict must be a fixed 17 bytes");
        let top_dict_index = build_index(&[&top_dict]);
        assert_eq!(top_dict_index.len() as u32, TOP_DICT_INDEX_LEN);

        let mut out = Vec::new();
        out.extend_from_slice(&header);
        out.extend_from_slice(&name);
        out.extend_from_slice(&top_dict_index);
        out.extend_from_slice(&string);
        out.extend_from_slice(&global);
        out.extend_from_slice(&cs_index);
        out.extend_from_slice(&private_dict);
        out.extend_from_slice(&local_index);
        out
    }

    /// Charstring drawing a 100×100 square via rmoveto + rlineto + endchar.
    fn square_charstring() -> Vec<u8> {
        vec![
            139, 139, 21, // rmoveto 0 0
            239, 139, 139, 239, 39, 139, 5, // rlineto (100,0)(0,100)(-100,0)
            14, // endchar
        ]
    }

    #[test]
    fn index_parse_empty() {
        let data = vec![0u8, 0];
        let (idx, end) = Index::parse(&data, 0).unwrap();
        assert_eq!(idx.count(), 0);
        assert_eq!(end, 2);
    }

    #[test]
    fn index_parse_two_objects() {
        let data = build_index(&[b"AB", b"CDE"]);
        let (idx, _) = Index::parse(&data, 0).unwrap();
        assert_eq!(idx.count(), 2);
        assert_eq!(idx.get(0), Some(&b"AB"[..]));
        assert_eq!(idx.get(1), Some(&b"CDE"[..]));
        assert_eq!(idx.get(2), None);
    }

    #[test]
    fn dict_parse_operands_and_operators() {
        // 100 200 17  (CharStrings op with two operands via small-int encoding)
        let bytes = vec![239u8, 28, 0, 200, 17];
        let dict = Dict::parse(&bytes).unwrap();
        assert_eq!(dict.get(17), Some(&[100.0, 200.0][..]));
        assert_eq!(dict.get_int(17), Some(100));
    }

    #[test]
    fn dict_parse_escape_operator() {
        // operand 5 + escape operator 12 7 (FontMatrix-ish) → key 1207
        let bytes = vec![144u8, 12, 7];
        let dict = Dict::parse(&bytes).unwrap();
        assert_eq!(dict.get(1207), Some(&[5.0][..]));
    }

    #[test]
    fn real_number_parse() {
        // -2.25 = nibbles: e 2 a 2 5 f
        let bytes = [0xe2, 0xa2, 0x5f];
        let (v, consumed) = parse_real(&bytes).unwrap();
        assert!((v - (-2.25)).abs() < 1e-9, "got {v}");
        assert_eq!(consumed, 3);
    }

    #[test]
    fn subr_bias_thresholds() {
        assert_eq!(subr_bias(0), 107);
        assert_eq!(subr_bias(1239), 107);
        assert_eq!(subr_bias(1240), 1131);
        assert_eq!(subr_bias(33899), 1131);
        assert_eq!(subr_bias(33900), 32768);
    }

    #[test]
    fn parse_simple_square_glyph() {
        let cs = square_charstring();
        let data = build_cff(&[&cs], &[], &[]);
        let cff = Cff::parse(&data).unwrap();
        assert_eq!(cff.num_glyphs(), 1);
        let glyph = cff.glyph(0).unwrap().expect("square has outline");
        let Outline::Simple(contours) = &glyph.outline else {
            panic!("expected simple outline");
        };
        assert_eq!(contours.len(), 1);
        assert_eq!(contours[0].points.len(), 4);
        assert_eq!(glyph.bbox.x_min, 0);
        assert_eq!(glyph.bbox.y_min, 0);
        assert_eq!(glyph.bbox.x_max, 100);
        assert_eq!(glyph.bbox.y_max, 100);
    }

    #[test]
    fn empty_charstring_returns_none() {
        // Just endchar → no path.
        let data = build_cff(&[&[14u8]], &[], &[]);
        let cff = Cff::parse(&data).unwrap();
        assert!(cff.glyph(0).unwrap().is_none());
    }

    #[test]
    fn rrcurveto_flattens_to_multiple_points() {
        // rmoveto 0 0 ; rrcurveto (50 100 50 -100 50 0) ; endchar
        // 50 → 189, 100 → 28-form, -100 → 39, 0 → 139
        let cs = vec![
            139u8, 139, 21, // rmoveto 0 0
            189, 28, 0, 100, 189, 28, 255, 156, 189, 139, 8, // rrcurveto
            14,
        ];
        let data = build_cff(&[&cs], &[], &[]);
        let cff = Cff::parse(&data).unwrap();
        let glyph = cff.glyph(0).unwrap().expect("curve glyph");
        let Outline::Simple(contours) = &glyph.outline else {
            panic!();
        };
        // moveto point + CUBIC_STEPS curve points.
        assert_eq!(contours[0].points.len(), 1 + CUBIC_STEPS);
    }

    #[test]
    fn callsubr_expands_local_subroutine() {
        // local subr 0 draws the square's lines; charstring moves then calls it.
        let subr: Vec<u8> = vec![239, 139, 139, 239, 39, 139, 5, 11]; // rlineto…; return
        // 1 local subr → bias 107 → call index 0 = operand -107 (byte 32).
        let cs = vec![139u8, 139, 21, 32, 10, 14]; // rmoveto 0 0 ; callsubr ; endchar
        let data = build_cff(&[&cs], &[], &[&subr]);
        let cff = Cff::parse(&data).unwrap();
        let glyph = cff.glyph(0).unwrap().expect("subr glyph");
        let Outline::Simple(contours) = &glyph.outline else {
            panic!();
        };
        assert_eq!(contours[0].points.len(), 4, "subr lineto path");
        assert_eq!(glyph.bbox.x_max, 100);
    }

    #[test]
    fn hintmask_bytes_are_skipped() {
        // hstem (100 50) ; hintmask + 1 mask byte ; rmoveto ; rlineto ; endchar.
        // A correctly-skipped 0xFF mask byte yields a clean 4-point square; if
        // not skipped it would be decoded as a 255-prefixed number and corrupt
        // the path.
        let cs = vec![
            239u8, 189, 1, // hstem 100 50
            19, 0xFF, // hintmask + 1 mask byte
            139, 139, 21, // rmoveto 0 0
            239, 139, 139, 239, 39, 139, 5, // rlineto square
            14,
        ];
        let data = build_cff(&[&cs], &[], &[]);
        let cff = Cff::parse(&data).unwrap();
        let glyph = cff.glyph(0).unwrap().expect("hinted glyph");
        let Outline::Simple(contours) = &glyph.outline else {
            panic!();
        };
        assert_eq!(contours[0].points.len(), 4);
        assert_eq!(glyph.bbox.x_max, 100);
    }

    #[test]
    fn rmoveto_strips_leading_width() {
        // rmoveto with THREE operands: first is the width, dropped; pen → (10,20).
        // 5(width)→144, 10→149, 20→159
        let cs = vec![
            144u8, 149, 159, 21, // (width 5) rmoveto 10 20
            149, 139, 5, // rlineto 10 0
            14,
        ];
        let data = build_cff(&[&cs], &[], &[]);
        let cff = Cff::parse(&data).unwrap();
        let glyph = cff.glyph(0).unwrap().expect("glyph");
        let Outline::Simple(contours) = &glyph.outline else {
            panic!();
        };
        // First emitted point is the moveto target (10,20), not (5,10).
        assert_eq!(contours[0].points[0].x, 10);
        assert_eq!(contours[0].points[0].y, 20);
    }

    #[test]
    fn hlineto_vlineto_alternate() {
        // rmoveto 0 0 ; hlineto with 3 args (h, v, h) ; endchar
        // 100→239, 50→189, 30→169
        let cs = vec![139u8, 139, 21, 239, 189, 169, 6, 14];
        let data = build_cff(&[&cs], &[], &[]);
        let cff = Cff::parse(&data).unwrap();
        let glyph = cff.glyph(0).unwrap().expect("glyph");
        let Outline::Simple(contours) = &glyph.outline else {
            panic!();
        };
        // moveto + 3 line points.
        assert_eq!(contours[0].points.len(), 4);
        // h:100 → (100,0); v:50 → (100,50); h:30 → (130,50)
        assert_eq!(contours[0].points[1], OutlinePoint { x: 100, y: 0, on_curve: true });
        assert_eq!(contours[0].points[2], OutlinePoint { x: 100, y: 50, on_curve: true });
        assert_eq!(contours[0].points[3], OutlinePoint { x: 130, y: 50, on_curve: true });
    }

    #[test]
    fn fd_select_format0_lookup() {
        let fs = FdSelect::Format0(vec![0, 1, 1, 2]);
        assert_eq!(fs.fd_for(0), 0);
        assert_eq!(fs.fd_for(2), 1);
        assert_eq!(fs.fd_for(3), 2);
        assert_eq!(fs.fd_for(99), 0); // out of range
    }

    #[test]
    fn fd_select_format3_lookup() {
        let fs = FdSelect::Format3 {
            ranges: vec![(0, 0), (5, 1), (10, 2)],
            sentinel: 15,
        };
        assert_eq!(fs.fd_for(0), 0);
        assert_eq!(fs.fd_for(4), 0);
        assert_eq!(fs.fd_for(5), 1);
        assert_eq!(fs.fd_for(9), 1);
        assert_eq!(fs.fd_for(10), 2);
        assert_eq!(fs.fd_for(14), 2);
        assert_eq!(fs.fd_for(15), 0); // >= sentinel
    }
}
