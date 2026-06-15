/// WOFF2 decoder — CSS Fonts L4, W3C WOFF2 spec.
///
/// Converts WOFF2 binary to a raw sfnt (TrueType/OpenType) byte vector that
/// `Font::parse` can consume. Transformed `glyf`/`loca` tables are fully
/// reconstructed per WOFF2 spec §5.2 (triplet coordinate decoding). The
/// reconstructed `loca` is always emitted in long (`u32`) form and `head`'s
/// `indexToLocFormat` is patched to match. Tables with an unknown transform
/// are passed through verbatim.
use crate::FontError;
use std::io::Cursor;

/// Magic bytes identifying WOFF2 data: ASCII "wOF2".
pub const WOFF2_MAGIC: u32 = 0x774F_4632;
/// Magic bytes identifying WOFF1 data: ASCII "wOFF".
pub const WOFF1_MAGIC: u32 = 0x774F_4646;

/// Returns `true` if `data` begins with the WOFF2 magic signature.
pub fn is_woff2(data: &[u8]) -> bool {
    data.len() >= 4 && u32::from_be_bytes([data[0], data[1], data[2], data[3]]) == WOFF2_MAGIC
}

/// Returns `true` if `data` begins with the WOFF1 magic signature.
pub fn is_woff1(data: &[u8]) -> bool {
    data.len() >= 4 && u32::from_be_bytes([data[0], data[1], data[2], data[3]]) == WOFF1_MAGIC
}

// ── WOFF2 base-128 variable-length encoding ────────────────────────────────

fn read_base128(data: &[u8], pos: &mut usize) -> Result<u32, FontError> {
    let mut result: u32 = 0;
    for i in 0..5 {
        if *pos >= data.len() {
            return Err(FontError::UnexpectedEof);
        }
        let b = data[*pos];
        *pos += 1;
        // Leading zero bytes are not allowed (except the value 0 itself).
        if i == 0 && b == 0x80 {
            return Err(FontError::InvalidData("woff2: base128 leading zero"));
        }
        result = (result << 7) | (b & 0x7F) as u32;
        if result > 0x1FFF_FFFF {
            return Err(FontError::InvalidData("woff2: base128 overflow"));
        }
        if b & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(FontError::InvalidData("woff2: base128 too long"))
}

// ── Table tag lookup table (WOFF2 spec Appendix C) ────────────────────────

const KNOWN_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post",
    b"cvt ", b"fpgm", b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT",
    b"EBLC", b"gasp", b"hdmx", b"kern", b"LTSH", b"PCLT", b"VDMX", b"vhea",
    b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC", b"JSTF", b"MATH",
    b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
    b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar",
    b"gvar", b"hsty", b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop",
    b"trak", b"Zapf", b"Silf", b"Glat", b"Gloc", b"Feat", b"Sill",
];

fn tag_from_flags(flags_byte: u8, extra_tag: Option<u32>) -> [u8; 4] {
    let idx = (flags_byte & 0x3F) as usize;
    if idx == 63 {
        let t = extra_tag.unwrap_or(0);
        t.to_be_bytes()
    } else if idx < KNOWN_TAGS.len() {
        *KNOWN_TAGS[idx]
    } else {
        [0, 0, 0, 0]
    }
}

// ── WOFF2 table directory entry ───────────────────────────────────────────

struct W2TableEntry {
    tag: [u8; 4],
    /// Original (uncompressed) length of the transformed table data.
    orig_length: u32,
    /// If Some: table is transformed and the brotli-stream contains `xform_len` bytes.
    /// If None: plain table data, `orig_length` bytes from brotli stream.
    xform_length: Option<u32>,
}

// ── glyf/loca transform ───────────────────────────────────────────────────

/// A reconstructed outline point in absolute font-unit coordinates.
struct GlyfPoint {
    x: i32,
    y: i32,
    on_curve: bool,
}

/// Apply the WOFF2 triplet sign convention: the low bit of `flag` selects the
/// sign (`1` → positive, `0` → negative). WOFF2 spec §5.2.
fn with_sign(flag: u8, baseval: i32) -> i32 {
    if flag & 1 != 0 { baseval } else { -baseval }
}

/// Read a big-endian `u16` at `*pos` and advance. Caller must guarantee bounds.
fn read_u16(data: &[u8], pos: &mut usize) -> u16 {
    let v = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    v
}

/// Read a big-endian `u32` at `*pos` and advance. Caller must guarantee bounds.
fn read_u32(data: &[u8], pos: &mut usize) -> u32 {
    let v = u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    v
}

/// Slice `len` bytes from `data` at `*pos`, advancing the cursor.
fn take<'a>(data: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8], FontError> {
    let end = pos.checked_add(len).ok_or(FontError::UnexpectedEof)?;
    let s = data.get(*pos..end).ok_or(FontError::UnexpectedEof)?;
    *pos = end;
    Ok(s)
}

/// Bounding box derived from reconstructed points; `[0;4]` for an empty set.
fn compute_bbox(points: &[GlyfPoint]) -> [i16; 4] {
    let mut x_min = i32::MAX;
    let mut y_min = i32::MAX;
    let mut x_max = i32::MIN;
    let mut y_max = i32::MIN;
    for p in points {
        x_min = x_min.min(p.x);
        y_min = y_min.min(p.y);
        x_max = x_max.max(p.x);
        y_max = y_max.max(p.y);
    }
    if points.is_empty() {
        [0, 0, 0, 0]
    } else {
        [x_min as i16, y_min as i16, x_max as i16, y_max as i16]
    }
}

/// Decode `n_points` coordinate triplets (WOFF2 spec §5.2) from the glyph
/// stream `data`, using one per-point flag byte each from `flags`. Returns the
/// absolute points and the number of glyph-stream bytes consumed.
fn decode_triplets(
    flags: &[u8],
    data: &[u8],
    n_points: usize,
) -> Result<(Vec<GlyfPoint>, usize), FontError> {
    let mut points = Vec::with_capacity(n_points);
    let mut x = 0i32;
    let mut y = 0i32;
    let mut ti = 0usize; // triplet index into `data`
    for i in 0..n_points {
        let raw = *flags.get(i).ok_or(FontError::UnexpectedEof)?;
        let on_curve = raw >> 7 == 0;
        let flag = raw & 0x7F;
        let n_data_bytes = if flag < 84 {
            1
        } else if flag < 120 {
            2
        } else if flag < 124 {
            3
        } else {
            4
        };
        if ti + n_data_bytes > data.len() {
            return Err(FontError::UnexpectedEof);
        }
        let (dx, dy) = if flag < 10 {
            (0, with_sign(flag, (((flag & 14) as i32) << 7) + data[ti] as i32))
        } else if flag < 20 {
            (
                with_sign(flag, ((((flag - 10) & 14) as i32) << 7) + data[ti] as i32),
                0,
            )
        } else if flag < 84 {
            let b0 = (flag - 20) as i32;
            let b1 = data[ti] as i32;
            (
                with_sign(flag, 1 + (b0 & 0x30) + (b1 >> 4)),
                with_sign(flag >> 1, 1 + ((b0 & 0x0C) << 2) + (b1 & 0x0F)),
            )
        } else if flag < 120 {
            let b0 = (flag - 84) as i32;
            (
                with_sign(flag, 1 + ((b0 / 12) << 8) + data[ti] as i32),
                with_sign(flag >> 1, 1 + (((b0 % 12) >> 2) << 8) + data[ti + 1] as i32),
            )
        } else if flag < 124 {
            let b1 = data[ti + 1] as i32;
            (
                with_sign(flag, ((data[ti] as i32) << 4) + (b1 >> 4)),
                with_sign(flag >> 1, ((b1 & 0x0F) << 8) + data[ti + 2] as i32),
            )
        } else {
            (
                with_sign(flag, ((data[ti] as i32) << 8) + data[ti + 1] as i32),
                with_sign(flag >> 1, ((data[ti + 2] as i32) << 8) + data[ti + 3] as i32),
            )
        };
        ti += n_data_bytes;
        x += dx;
        y += dy;
        points.push(GlyfPoint { x, y, on_curve });
    }
    Ok((points, ti))
}

/// Reconstruct a classic `glyf` table (WOFF2 spec §5.2) from its transformed
/// representation. Fills `loca_entries` with `num_glyphs + 1` byte offsets into
/// the produced table (long-loca form: the caller emits them as `u32`).
///
/// Simple glyphs are re-encoded in canonical TrueType form: one flag byte per
/// point (no RLE), followed by signed `int16` x- then y-delta arrays. Bounding
/// boxes are taken from the bbox stream when present, otherwise computed from
/// the reconstructed outline.
pub(crate) fn decode_transformed_glyf(
    data: &[u8],
    loca_entries: &mut Vec<u32>,
) -> Result<Vec<u8>, FontError> {
    if data.len() < 36 {
        return Err(FontError::InvalidData("woff2: transformed glyf too short"));
    }
    let mut pos = 0usize;
    let _reserved = read_u16(data, &mut pos);
    let option_flags = read_u16(data, &mut pos);
    let num_glyphs = read_u16(data, &mut pos);
    let _index_format = read_u16(data, &mut pos);
    let n_contour_size = read_u32(data, &mut pos) as usize;
    let n_points_size = read_u32(data, &mut pos) as usize;
    let flag_size = read_u32(data, &mut pos) as usize;
    let glyph_size = read_u32(data, &mut pos) as usize;
    let composite_size = read_u32(data, &mut pos) as usize;
    let bbox_size = read_u32(data, &mut pos) as usize;
    let instruction_size = read_u32(data, &mut pos) as usize;

    // Substreams follow the 36-byte header in fixed order (WOFF2 spec §5.2).
    let n_contour_stream = take(data, &mut pos, n_contour_size)?;
    let n_points_stream = take(data, &mut pos, n_points_size)?;
    let flag_stream = take(data, &mut pos, flag_size)?;
    let glyph_stream = take(data, &mut pos, glyph_size)?;
    let composite_stream = take(data, &mut pos, composite_size)?;
    let bbox_stream = take(data, &mut pos, bbox_size)?;
    let instruction_stream = take(data, &mut pos, instruction_size)?;

    // Optional overlapSimpleBitmap (WOFF2 erratum): present iff optionFlags
    // bit 0 set; one bit per glyph, located after the instruction stream.
    let overlap_bitmap = if option_flags & 1 != 0 {
        let n = num_glyphs.div_ceil(8) as usize;
        Some(take(data, &mut pos, n)?)
    } else {
        None
    };

    // bBox stream: leading bitmap (1 bit/glyph), then explicit Int16×4 boxes.
    let bbox_bitmap_bytes = num_glyphs.div_ceil(8).max(1) as usize;
    let bbox_bitmap = bbox_stream
        .get(..bbox_bitmap_bytes)
        .ok_or(FontError::UnexpectedEof)?;
    let bbox_data = bbox_stream
        .get(bbox_bitmap_bytes..)
        .ok_or(FontError::UnexpectedEof)?;

    let mut np_pos = 0usize;
    let mut flag_pos = 0usize;
    let mut glyph_pos = 0usize;
    let mut composite_pos = 0usize;
    let mut instr_pos = 0usize;
    let mut bbox_data_pos = 0usize;

    let mut out = Vec::<u8>::new();
    loca_entries.clear();

    for g in 0..num_glyphs as usize {
        loca_entries.push(out.len() as u32);

        let n_contours = i16::from_be_bytes([
            *n_contour_stream.get(g * 2).ok_or(FontError::UnexpectedEof)?,
            *n_contour_stream.get(g * 2 + 1).ok_or(FontError::UnexpectedEof)?,
        ]);
        if n_contours == 0 {
            continue; // empty glyph: no outline, loca[g] == loca[g+1]
        }

        let has_bbox = bbox_bitmap[g / 8] >> (7 - (g % 8)) & 1 != 0;
        let explicit_bbox = if has_bbox {
            let b = bbox_data
                .get(bbox_data_pos..bbox_data_pos + 8)
                .ok_or(FontError::UnexpectedEof)?;
            bbox_data_pos += 8;
            Some([
                i16::from_be_bytes([b[0], b[1]]),
                i16::from_be_bytes([b[2], b[3]]),
                i16::from_be_bytes([b[4], b[5]]),
                i16::from_be_bytes([b[6], b[7]]),
            ])
        } else {
            None
        };

        if n_contours < 0 {
            // ── Composite glyph: bbox is always explicit per spec ──
            let bbox = explicit_bbox.unwrap_or([0, 0, 0, 0]);
            out.extend_from_slice(&n_contours.to_be_bytes());
            for v in bbox {
                out.extend_from_slice(&v.to_be_bytes());
            }
            let mut have_instructions = false;
            loop {
                let flags = u16::from_be_bytes([
                    *composite_stream.get(composite_pos).ok_or(FontError::UnexpectedEof)?,
                    *composite_stream.get(composite_pos + 1).ok_or(FontError::UnexpectedEof)?,
                ]);
                composite_pos += 2;
                out.extend_from_slice(&flags.to_be_bytes());
                let glyph_index = composite_stream
                    .get(composite_pos..composite_pos + 2)
                    .ok_or(FontError::UnexpectedEof)?;
                out.extend_from_slice(glyph_index);
                composite_pos += 2;
                const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
                const WE_HAVE_A_SCALE: u16 = 0x0008;
                const MORE_COMPONENTS: u16 = 0x0020;
                const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
                const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
                const WE_HAVE_INSTRUCTIONS: u16 = 0x0100;
                let arg_size = if flags & ARG_1_AND_2_ARE_WORDS != 0 { 4 } else { 2 };
                let scale_size = if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
                    8
                } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
                    4
                } else if flags & WE_HAVE_A_SCALE != 0 {
                    2
                } else {
                    0
                };
                let chunk = composite_stream
                    .get(composite_pos..composite_pos + arg_size + scale_size)
                    .ok_or(FontError::UnexpectedEof)?;
                out.extend_from_slice(chunk);
                composite_pos += arg_size + scale_size;
                if flags & WE_HAVE_INSTRUCTIONS != 0 {
                    have_instructions = true;
                }
                if flags & MORE_COMPONENTS == 0 {
                    break;
                }
            }
            if have_instructions {
                // instructionLength is a 255UInt16 in the glyph stream; the
                // instruction bytes live in the instruction stream.
                let (instr_len, consumed) = read_255uint16(glyph_stream, glyph_pos)?;
                glyph_pos += consumed;
                out.extend_from_slice(&instr_len.to_be_bytes());
                let instrs = instruction_stream
                    .get(instr_pos..instr_pos + instr_len as usize)
                    .ok_or(FontError::UnexpectedEof)?;
                out.extend_from_slice(instrs);
                instr_pos += instr_len as usize;
            }
        } else {
            // ── Simple glyph ──
            let n_contours = n_contours as usize;
            let mut end_pts = Vec::with_capacity(n_contours);
            let mut total_points: u32 = 0;
            for _ in 0..n_contours {
                let (v, consumed) = read_255uint16(n_points_stream, np_pos)?;
                np_pos += consumed;
                if v == 0 {
                    continue; // degenerate empty contour — skip (BUG-059)
                }
                total_points = total_points
                    .checked_add(v as u32)
                    .ok_or(FontError::InvalidData("woff2: point count overflow"))?;
                if total_points > 65535 {
                    return Err(FontError::InvalidData("woff2: glyph exceeds 65535 points"));
                }
                end_pts.push(total_points - 1);
            }
            let n_pts = total_points as usize;

            // Flags: one byte per point from the flag stream.
            let flags_slice = flag_stream
                .get(flag_pos..flag_pos + n_pts)
                .ok_or(FontError::UnexpectedEof)?;
            flag_pos += n_pts;
            // Coordinate triplets from the glyph stream.
            let remaining = glyph_stream.get(glyph_pos..).ok_or(FontError::UnexpectedEof)?;
            let (points, consumed) = decode_triplets(flags_slice, remaining, n_pts)?;
            glyph_pos += consumed;
            // instructionLength (255UInt16) from the glyph stream, bytes from
            // the instruction stream — read even for all-empty-contour glyphs.
            let (instr_len, consumed) = read_255uint16(glyph_stream, glyph_pos)?;
            glyph_pos += consumed;
            let instrs = instruction_stream
                .get(instr_pos..instr_pos + instr_len as usize)
                .ok_or(FontError::UnexpectedEof)?
                .to_vec();
            instr_pos += instr_len as usize;

            if end_pts.is_empty() {
                continue; // all contours were empty → treat as empty glyph
            }

            let bbox = explicit_bbox.unwrap_or_else(|| compute_bbox(&points));
            out.extend_from_slice(&(end_pts.len() as i16).to_be_bytes());
            for v in bbox {
                out.extend_from_slice(&v.to_be_bytes());
            }
            for ep in &end_pts {
                out.extend_from_slice(&(*ep as u16).to_be_bytes());
            }
            out.extend_from_slice(&instr_len.to_be_bytes());
            out.extend_from_slice(&instrs);

            // Canonical simple-glyph encoding: one flag byte/point (no RLE),
            // then int16 x-deltas, then int16 y-deltas.
            let overlap_first = overlap_bitmap
                .map(|bm| bm[g / 8] >> (7 - (g % 8)) & 1 != 0)
                .unwrap_or(false);
            for (i, p) in points.iter().enumerate() {
                let mut f = if p.on_curve { 0x01u8 } else { 0 };
                if i == 0 && overlap_first {
                    f |= 0x40; // OVERLAP_SIMPLE
                }
                out.push(f);
            }
            let mut prev = 0i32;
            for p in &points {
                out.extend_from_slice(&((p.x - prev) as i16).to_be_bytes());
                prev = p.x;
            }
            let mut prev = 0i32;
            for p in &points {
                out.extend_from_slice(&((p.y - prev) as i16).to_be_bytes());
                prev = p.y;
            }
        }

        // Pad each glyph to a 2-byte boundary (long-loca offsets are exact).
        if !out.len().is_multiple_of(2) {
            out.push(0);
        }
    }

    loca_entries.push(out.len() as u32);
    Ok(out)
}

/// 255UInt16 encoding from WOFF2 spec §5.1.
fn read_255uint16(data: &[u8], pos: usize) -> Result<(u16, usize), FontError> {
    let b0 = data.get(pos).copied().ok_or(FontError::UnexpectedEof)?;
    match b0 {
        253 => {
            // 2-byte value
            let hi = data.get(pos + 1).copied().ok_or(FontError::UnexpectedEof)?;
            let lo = data.get(pos + 2).copied().ok_or(FontError::UnexpectedEof)?;
            Ok((u16::from_be_bytes([hi, lo]), 3))
        }
        254 => {
            // value = next_byte + 506
            let b1 = data.get(pos + 1).copied().ok_or(FontError::UnexpectedEof)?;
            Ok((b1 as u16 + 506, 2))
        }
        255 => {
            // value = next_byte + 253
            let b1 = data.get(pos + 1).copied().ok_or(FontError::UnexpectedEof)?;
            Ok((b1 as u16 + 253, 2))
        }
        _ => Ok((b0 as u16, 1)),
    }
}

// ── WOFF2 main decoder ────────────────────────────────────────────────────

/// Decode WOFF2 bytes into a raw sfnt byte vector.
///
/// On success returns the raw TrueType/OpenType bytes. On failure returns
/// `FontError::InvalidData` or `FontError::UnexpectedEof`.
pub fn decode_woff2(woff2: &[u8]) -> Result<Vec<u8>, FontError> {
    if woff2.len() < 48 {
        return Err(FontError::UnexpectedEof);
    }
    let signature = u32::from_be_bytes([woff2[0], woff2[1], woff2[2], woff2[3]]);
    if signature != WOFF2_MAGIC {
        return Err(FontError::InvalidData("woff2: bad magic"));
    }
    let flavor             = u32::from_be_bytes([woff2[4], woff2[5], woff2[6], woff2[7]]);
    // length, reserved, totalSfntSize, totalCompressedSize at 8..24
    let total_compressed   = u32::from_be_bytes([woff2[20], woff2[21], woff2[22], woff2[23]]);
    let num_tables         = u16::from_be_bytes([woff2[12], woff2[13]]) as usize;
    // metaOffset starts at 28, privOffset at 40 — skip for now.

    let mut pos = 48usize;

    // Parse table directory
    let mut entries: Vec<W2TableEntry> = Vec::with_capacity(num_tables);
    for _ in 0..num_tables {
        if pos >= woff2.len() {
            return Err(FontError::UnexpectedEof);
        }
        let flags_byte = woff2[pos]; pos += 1;
        let tag = if flags_byte & 0x3F == 63 {
            if pos + 4 > woff2.len() { return Err(FontError::UnexpectedEof); }
            let t = u32::from_be_bytes([woff2[pos], woff2[pos+1], woff2[pos+2], woff2[pos+3]]);
            pos += 4;
            tag_from_flags(flags_byte, Some(t))
        } else {
            tag_from_flags(flags_byte, None)
        };

        let xform_version = (flags_byte >> 6) & 0x03;
        let orig_length = read_base128(woff2, &mut pos)?;

        // glyf and loca have a transform length even if xform_version == 0
        let is_glyf = &tag == b"glyf";
        let is_loca = &tag == b"loca";
        let has_xform_length = (is_glyf || is_loca) && xform_version != 3
            || (!is_glyf && !is_loca && xform_version != 0);

        let xform_length = if has_xform_length {
            Some(read_base128(woff2, &mut pos)?)
        } else {
            None
        };

        entries.push(W2TableEntry { tag, orig_length, xform_length });
    }

    // Compressed data starts at `pos`, length = total_compressed_size
    if pos + total_compressed as usize > woff2.len() {
        return Err(FontError::UnexpectedEof);
    }
    let compressed = &woff2[pos..pos + total_compressed as usize];

    // Decompress with Brotli
    let mut decompressed = Vec::new();
    {
        let mut reader = Cursor::new(compressed);
        brotli_decompressor::BrotliDecompress(&mut reader, &mut decompressed)
            .map_err(|_| FontError::InvalidData("woff2: brotli decompress failed"))?;
    }

    // Reconstruct table data for each entry. glyf is decoded first (it also
    // produces the loca offsets); loca is then synthesised in long form.
    let glyf_idx = entries.iter().position(|e| &e.tag == b"glyf");
    let loca_idx = entries.iter().position(|e| &e.tag == b"loca");

    let mut table_data: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    let mut decomp_pos = 0usize;
    let mut loca_entries: Vec<u32> = Vec::new();
    let mut glyf_decoded = false;
    // True when loca was rebuilt in long (u32) form → head must be patched.
    let mut loca_is_long = false;

    for (i, entry) in entries.iter().enumerate() {
        let byte_count = entry.xform_length.unwrap_or(entry.orig_length) as usize;
        let chunk = decompressed.get(decomp_pos..decomp_pos + byte_count)
            .ok_or(FontError::UnexpectedEof)?
            .to_vec();
        decomp_pos += byte_count;

        if Some(i) == glyf_idx && entry.xform_length.is_some() {
            // Transformed glyf — reconstruct it (also fills loca_entries).
            let glyf_out = decode_transformed_glyf(&chunk, &mut loca_entries)?;
            glyf_decoded = true;
            table_data.push(glyf_out);
        } else if Some(i) == loca_idx && entry.xform_length.is_some() {
            // Transformed loca — synthesise long offsets from loca_entries.
            if glyf_decoded && !loca_entries.is_empty() {
                let mut loca_out = Vec::with_capacity(loca_entries.len() * 4);
                for off in &loca_entries {
                    loca_out.extend_from_slice(&off.to_be_bytes());
                }
                loca_is_long = true;
                table_data.push(loca_out);
            } else {
                // Fallback: pass chunk through (zero-length loca)
                table_data.push(chunk);
            }
        } else {
            table_data.push(chunk);
        }
    }

    // The synthesised loca is long-form; make `head.indexToLocFormat` agree.
    if loca_is_long
        && let Some(head_idx) = entries.iter().position(|e| &e.tag == b"head")
    {
        let head = &mut table_data[head_idx];
        // indexToLocFormat is the Int16 at byte offset 50 of `head`.
        if head.len() >= 52 {
            head[50] = 0;
            head[51] = 1;
        }
    }

    // Build output sfnt binary
    build_sfnt(flavor, &entries, &table_data)
}

/// Build a raw sfnt binary from table entries and data blobs.
fn build_sfnt(
    sfnt_version: u32,
    entries: &[W2TableEntry],
    table_data: &[Vec<u8>],
) -> Result<Vec<u8>, FontError> {
    let num_tables = u16::try_from(entries.len())
        .map_err(|_| FontError::InvalidData("woff2: too many tables"))?;
    let (search_range, entry_selector, range_shift) = sfnt_search_params(num_tables);

    let header_size = 12usize
        .checked_add(num_tables as usize * 16)
        .ok_or(FontError::InvalidData("woff2: header size overflow"))?;
    let mut offset_after_header = u32::try_from(header_size)
        .map_err(|_| FontError::InvalidData("woff2: font too large"))?;

    // Pre-compute padded offsets
    let mut offsets = Vec::with_capacity(entries.len());
    for data in table_data {
        offsets.push(offset_after_header);
        let padded = u32::try_from((data.len() + 3) & !3)
            .map_err(|_| FontError::InvalidData("woff2: table too large"))?;
        offset_after_header = offset_after_header
            .checked_add(padded)
            .ok_or(FontError::InvalidData("woff2: total font size overflow"))?;
    }

    let total_size = usize::try_from(offset_after_header)
        .map_err(|_| FontError::InvalidData("woff2: font too large for allocation"))?;
    let mut out = vec![0u8; total_size];

    // Write offset table
    out[0..4].copy_from_slice(&sfnt_version.to_be_bytes());
    out[4..6].copy_from_slice(&num_tables.to_be_bytes());
    out[6..8].copy_from_slice(&search_range.to_be_bytes());
    out[8..10].copy_from_slice(&entry_selector.to_be_bytes());
    out[10..12].copy_from_slice(&range_shift.to_be_bytes());

    // Write table records and data
    for (i, (entry, data)) in entries.iter().zip(table_data.iter()).enumerate() {
        let checksum = table_checksum(data);
        let record_off = 12 + i * 16;
        out[record_off..record_off + 4].copy_from_slice(&entry.tag);
        out[record_off + 4..record_off + 8].copy_from_slice(&checksum.to_be_bytes());
        out[record_off + 8..record_off + 12].copy_from_slice(&offsets[i].to_be_bytes());
        let length = data.len() as u32;
        out[record_off + 12..record_off + 16].copy_from_slice(&length.to_be_bytes());

        let data_off = offsets[i] as usize;
        out[data_off..data_off + data.len()].copy_from_slice(data);
    }

    Ok(out)
}

// searchRange/entrySelector/rangeShift are binary-search hints in the sfnt header.
// Parsers (including ours) iterate linearly, so zeroing these on overflow is safe.
fn sfnt_search_params(num_tables: u16) -> (u16, u16, u16) {
    if num_tables == 0 {
        return (0, 0, 0);
    }
    let n = num_tables as u32;
    let mut search_range = 1u32;
    let mut entry_selector = 0u16;
    while search_range * 2 <= n {
        search_range *= 2;
        entry_selector += 1;
    }
    let Some(sr) = search_range.checked_mul(16) else { return (0, 0, 0) };
    let Some(rs) = n.checked_mul(16).and_then(|v| v.checked_sub(sr)) else { return (0, 0, 0) };
    (sr as u16, entry_selector, rs as u16)
}

fn table_checksum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 3 < data.len() {
        sum = sum.wrapping_add(u32::from_be_bytes([data[i], data[i+1], data[i+2], data[i+3]]));
        i += 4;
    }
    if i < data.len() {
        let mut last = [0u8; 4];
        last[..data.len() - i].copy_from_slice(&data[i..]);
        sum = sum.wrapping_add(u32::from_be_bytes(last));
    }
    sum
}

// ── WOFF1 decoder ─────────────────────────────────────────────────────────
// WOFF1 uses zlib/deflate. Phase 0: use miniz_oxide (already a dep via wgpu).

/// Decode WOFF1 bytes into a raw sfnt byte vector.
/// WOFF1 spec: each table is zlib-compressed individually; uncompressed if
/// origLength == compLength.
pub fn decode_woff1(woff: &[u8]) -> Result<Vec<u8>, FontError> {
    if woff.len() < 44 {
        return Err(FontError::UnexpectedEof);
    }
    let signature = u32::from_be_bytes([woff[0], woff[1], woff[2], woff[3]]);
    if signature != WOFF1_MAGIC {
        return Err(FontError::InvalidData("woff1: bad magic"));
    }
    let flavor     = u32::from_be_bytes([woff[4], woff[5], woff[6], woff[7]]);
    let num_tables = u16::from_be_bytes([woff[12], woff[13]]) as usize;

    // Table directory at offset 44; each entry is 20 bytes
    let dir_size = num_tables * 20;
    if 44 + dir_size > woff.len() {
        return Err(FontError::UnexpectedEof);
    }

    let mut entries: Vec<([u8; 4], u32, u32, u32)> = Vec::with_capacity(num_tables); // tag, offset, compLen, origLen
    for i in 0..num_tables {
        let base = 44 + i * 20;
        let tag = [woff[base], woff[base+1], woff[base+2], woff[base+3]];
        let offset   = u32::from_be_bytes([woff[base+4],  woff[base+5],  woff[base+6],  woff[base+7]]);
        let comp_len = u32::from_be_bytes([woff[base+8],  woff[base+9],  woff[base+10], woff[base+11]]);
        let orig_len = u32::from_be_bytes([woff[base+12], woff[base+13], woff[base+14], woff[base+15]]);
        entries.push((tag, offset, comp_len, orig_len));
    }

    // Decompress each table
    let mut table_data: Vec<Vec<u8>> = Vec::with_capacity(num_tables);
    for (_, offset, comp_len, orig_len) in &entries {
        let src = woff.get(*offset as usize..*offset as usize + *comp_len as usize)
            .ok_or(FontError::UnexpectedEof)?;
        let data = if comp_len == orig_len {
            src.to_vec()
        } else {
            // WOFF1 tables are zlib-compressed (RFC 1950):
            // 2-byte CMF+FLG header + deflate stream + 4-byte Adler32 trailer.
            if src.len() < 6 {
                return Err(FontError::UnexpectedEof);
            }
            let deflate_data = &src[2..src.len() - 4];
            zune_inflate::DeflateDecoder::new(deflate_data)
                .decode_deflate()
                .map_err(|_| FontError::InvalidData("woff1: deflate decompress failed"))?
        };
        table_data.push(data);
    }

    // Build sfnt
    let sfnt_entries: Vec<W2TableEntry> = entries
        .iter()
        .zip(table_data.iter())
        .map(|((tag, _, _, orig_len), data)| W2TableEntry {
            tag: *tag,
            orig_length: *orig_len,
            xform_length: if data.len() != *orig_len as usize { Some(data.len() as u32) } else { None },
        })
        .collect();
    build_sfnt(flavor, &sfnt_entries, &table_data)
}

// ── Auto-detect and decode any supported font format ─────────────────────

/// If `data` is WOFF2 or WOFF1, decode it and return the raw sfnt bytes.
/// If `data` is already sfnt (TTF/OTF), return `None` (caller uses as-is).
pub fn maybe_decode_font(data: &[u8]) -> Result<Option<Vec<u8>>, FontError> {
    if is_woff2(data) {
        Ok(Some(decode_woff2(data)?))
    } else if is_woff1(data) {
        Ok(Some(decode_woff1(data)?))
    } else {
        Ok(None)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_woff2_detects_magic() {
        let mut data = vec![0u8; 48];
        data[0] = 0x77; data[1] = 0x4F; data[2] = 0x46; data[3] = 0x32;
        assert!(is_woff2(&data));
    }

    #[test]
    fn is_woff1_detects_magic() {
        let mut data = vec![0u8; 48];
        data[0] = 0x77; data[1] = 0x4F; data[2] = 0x46; data[3] = 0x46;
        assert!(is_woff1(&data));
    }

    #[test]
    fn is_woff2_false_for_ttf() {
        // TTF magic = 0x00010000
        let data = [0x00, 0x01, 0x00, 0x00];
        assert!(!is_woff2(&data));
        assert!(!is_woff1(&data));
    }

    #[test]
    fn maybe_decode_none_for_raw_sfnt() {
        // Raw TTF/OTF: magic = 0x00010000 or 0x4F54544F
        let data = [0x00u8, 0x01, 0x00, 0x00, 0, 0, 0, 0];
        assert!(maybe_decode_font(&data).unwrap().is_none());
    }

    #[test]
    fn decode_woff2_rejects_bad_magic() {
        let data = vec![0u8; 48]; // all-zero
        assert!(matches!(decode_woff2(&data), Err(FontError::InvalidData(_))));
    }

    #[test]
    fn decode_woff2_rejects_truncated() {
        let mut data = vec![0u8; 30]; // shorter than header
        data[0] = 0x77; data[1] = 0x4F; data[2] = 0x46; data[3] = 0x32;
        assert!(matches!(decode_woff2(&data), Err(FontError::UnexpectedEof)));
    }

    #[test]
    fn read_base128_single_byte() {
        let data = [0x05];
        let mut pos = 0;
        assert_eq!(read_base128(&data, &mut pos).unwrap(), 5);
        assert_eq!(pos, 1);
    }

    #[test]
    fn read_base128_multi_byte() {
        // 0x80 | 0x40 = 0xC0, then 0x00 → value = 0x40 << 7 | 0 = 8192
        let data = [0xC0, 0x00];
        let mut pos = 0;
        assert_eq!(read_base128(&data, &mut pos).unwrap(), 8192);
        assert_eq!(pos, 2);
    }

    #[test]
    fn table_checksum_empty() {
        assert_eq!(table_checksum(&[]), 0);
    }

    fn with_sign_helper(flag: u8, base: i32) -> i32 {
        with_sign(flag, base)
    }

    #[test]
    fn with_sign_follows_low_bit() {
        // Odd flag → positive, even flag → negative (WOFF2 spec §5.2).
        assert_eq!(with_sign_helper(1, 10), 10);
        assert_eq!(with_sign_helper(0, 10), -10);
        assert_eq!(with_sign_helper(21, 5), 5);
        assert_eq!(with_sign_helper(20, 5), -5);
    }

    // Build a minimal transformed glyf data block for use in unit tests.
    // Glyph 0 is a simple glyph with `points_per_contour.len()` contours; per-point
    // flag bytes go in the flag stream and coordinate bytes in the glyph stream
    // (followed by a single 0 byte = instructionLength 0). All other glyphs are
    // empty (nContours == 0).
    fn make_glyf_transform(
        num_glyphs: u16,
        points_per_contour: &[u8], // one 255UInt16 value per contour (all < 253 → 1 byte)
        flag_bytes: &[u8],         // one flag byte per total point (flag stream)
        coord_bytes: &[u8],        // coordinate triplet bytes (glyph stream)
    ) -> Vec<u8> {
        let n_contours = points_per_contour.len() as i16;
        let mut ncontour_stream: Vec<u8> = Vec::new();
        ncontour_stream.extend_from_slice(&n_contours.to_be_bytes());
        for _ in 1..num_glyphs {
            ncontour_stream.extend_from_slice(&0i16.to_be_bytes());
        }

        let npoints_stream: Vec<u8> = points_per_contour.to_vec();
        let flag_stream: Vec<u8> = flag_bytes.to_vec();

        // glyph stream: coordinate triplets, then instructionLength (255UInt16 = 0).
        let mut glyph_stream: Vec<u8> = coord_bytes.to_vec();
        glyph_stream.push(0u8);

        let bbox_bitmap_size = num_glyphs.div_ceil(8).max(1) as usize;
        let bbox_stream: Vec<u8> = vec![0u8; bbox_bitmap_size]; // all bits 0 → no explicit bbox
        let composite_stream: Vec<u8> = Vec::new();
        let instruction_stream: Vec<u8> = Vec::new();

        let mut data: Vec<u8> = Vec::new();
        // Header (36 bytes)
        data.extend_from_slice(&0u16.to_be_bytes()); // reserved
        data.extend_from_slice(&0u16.to_be_bytes()); // optionFlags
        data.extend_from_slice(&num_glyphs.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes()); // indexFormat
        data.extend_from_slice(&(ncontour_stream.len() as u32).to_be_bytes());
        data.extend_from_slice(&(npoints_stream.len() as u32).to_be_bytes());
        data.extend_from_slice(&(flag_stream.len() as u32).to_be_bytes());
        data.extend_from_slice(&(glyph_stream.len() as u32).to_be_bytes());
        data.extend_from_slice(&(composite_stream.len() as u32).to_be_bytes());
        data.extend_from_slice(&(bbox_stream.len() as u32).to_be_bytes());
        data.extend_from_slice(&(instruction_stream.len() as u32).to_be_bytes());
        // Streams
        data.extend_from_slice(&ncontour_stream);
        data.extend_from_slice(&npoints_stream);
        data.extend_from_slice(&flag_stream);
        data.extend_from_slice(&glyph_stream);
        data.extend_from_slice(&composite_stream);
        data.extend_from_slice(&bbox_stream);
        data.extend_from_slice(&instruction_stream);
        data
    }

    #[test]
    fn glyf_transform_zero_point_contour_skipped_gracefully() {
        // BUG-059: a glyph with 1 contour having 0 points must not return an error.
        let data = make_glyf_transform(1, &[0u8], &[], &[]);
        let mut loca = Vec::new();
        let result = super::decode_transformed_glyf(&data, &mut loca);
        assert!(result.is_ok(), "zero-point contour should be accepted: {:?}", result.err());
        // Empty glyph → loca[0] == loca[1].
        assert_eq!(loca, vec![0, 0]);
    }

    #[test]
    fn glyf_transform_normal_glyph_decoded() {
        // Simple glyph: 1 contour with 1 on-curve point.
        // Flag byte 0x00 → on-curve, masked flag 0, n_data_bytes 1.
        // Coordinate byte 0x00 → dx=0, dy=with_sign(0,0)=0.
        let data = make_glyf_transform(1, &[1u8], &[0x00u8], &[0x00u8]);
        let mut loca = Vec::new();
        let result = super::decode_transformed_glyf(&data, &mut loca);
        assert!(result.is_ok(), "normal 1-point glyph should decode: {:?}", result.err());
        let glyf = result.unwrap();
        assert!(!glyf.is_empty(), "decoded glyf must not be empty");
        // First i16 = numberOfContours = 1.
        assert_eq!(i16::from_be_bytes([glyf[0], glyf[1]]), 1);
        // loca[1] points past the glyph, loca[0] == 0.
        assert_eq!(loca.len(), 2);
        assert_eq!(loca[0], 0);
        assert_eq!(loca[1] as usize, glyf.len());
    }

    #[test]
    fn glyf_transform_triplet_coords_reconstructed() {
        // 1 contour, 2 points. Both on-curve (flag high bit 0).
        // Point 0: masked flag 1 (range 0..10, n_data_bytes 1): dx=0,
        //   dy = with_sign(1, ((1&14)<<7) + coord) = +(0 + 5) = 5.
        // Point 1: masked flag 11 (range 10..20, n_data_bytes 1): dy=0,
        //   dx = with_sign(11, (((11-10)&14)<<7) + coord) = +(0 + 7) = 7.
        let data = make_glyf_transform(1, &[2u8], &[0x01u8, 0x0Bu8], &[5u8, 7u8]);
        let mut loca = Vec::new();
        let glyf = super::decode_transformed_glyf(&data, &mut loca).unwrap();

        // Parse the produced glyph with the real glyf parser and check points.
        let glyph = crate::glyf::Glyph::parse(&glyf[loca[0] as usize..loca[1] as usize]).unwrap();
        let crate::glyf::Outline::Simple(contours) = &glyph.outline else {
            panic!("expected simple outline");
        };
        assert_eq!(contours.len(), 1);
        let pts = &contours[0].points;
        assert_eq!(pts.len(), 2);
        // Absolute coords: p0 = (0, 5), p1 = (7, 5).
        assert_eq!((pts[0].x, pts[0].y), (0, 5));
        assert_eq!((pts[1].x, pts[1].y), (7, 5));
        assert!(pts[0].on_curve && pts[1].on_curve);
    }

    #[test]
    fn glyf_transform_empty_glyph_produces_no_output() {
        // num_glyphs 1 but glyph 0 has nContours 0 → empty. We force this by
        // passing zero contours via an empty points list.
        let data = make_glyf_transform(1, &[], &[], &[]);
        let mut loca = Vec::new();
        let result = super::decode_transformed_glyf(&data, &mut loca);
        assert!(result.is_ok(), "empty glyph must decode OK: {:?}", result.err());
        assert!(result.unwrap().is_empty(), "empty glyph must emit no bytes");
    }

    #[test]
    fn table_checksum_aligned() {
        // 0x00_01_02_03 → 0x00010203
        let data = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(table_checksum(&data), 0x0001_0203);
    }

    #[test]
    fn sfnt_search_params_4_tables() {
        let (sr, es, rs) = sfnt_search_params(4);
        assert_eq!(sr, 4 * 16);
        assert_eq!(es, 2);
        assert_eq!(rs, 0);
    }

    #[test]
    fn build_sfnt_returns_correct_magic() {
        // Build minimal 0-table sfnt
        let data = build_sfnt(0x0001_0000, &[], &[]).unwrap();
        assert_eq!(data.len(), 12);
        let v = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        assert_eq!(v, 0x0001_0000);
    }
}
