/// WOFF2 decoder — CSS Fonts L4, W3C WOFF2 spec.
///
/// Converts WOFF2 binary to a raw sfnt (TrueType/OpenType) byte vector that
/// `Font::parse` can consume. Phase 0: transformed glyf/loca tables are
/// decoded using the WOFF2 spec §5.1 simple xor-delta reconstruction. Tables
/// with transform version != 0 or 3 are passed through verbatim (non-glyf/loca).
use crate::FontError;
use std::io::Cursor;

type TripletResult = Result<(Vec<u8>, Vec<u8>, Vec<u8>), FontError>;
type TripletDecode = Result<(i32, i32, u8, Vec<u8>, Vec<u8>), FontError>;

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

/// Reconstruct a transformed glyf table (WOFF2 spec §5.1) from the decoded bytes.
/// Phase 0 implementation: if the xform version is 0 (no transformation applied)
/// the bytes are returned as-is. Version 3 (full glyf transform) rebuilds
/// the classic glyf + loca table pair.
pub(crate) fn decode_transformed_glyf(
    data: &[u8],
    loca_entries: &mut Vec<u32>,
    index_to_loc_format: u16,
) -> Result<Vec<u8>, FontError> {
    if data.len() < 36 {
        return Err(FontError::InvalidData("woff2: transformed glyf too short"));
    }
    let mut pos = 0usize;
    let reserved        = u16::from_be_bytes([data[pos], data[pos+1]]); pos += 2;
    let option_flags    = u16::from_be_bytes([data[pos], data[pos+1]]); pos += 2;
    let num_glyphs      = u16::from_be_bytes([data[pos], data[pos+1]]); pos += 2;
    let index_format    = u16::from_be_bytes([data[pos], data[pos+1]]); pos += 2;
    let n_contour_stream_size = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]); pos += 4;
    let n_points_stream_size  = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]); pos += 4;
    let flag_stream_size      = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]); pos += 4;
    let glyph_stream_size     = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]); pos += 4;
    let composite_stream_size = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]); pos += 4;
    let bbox_stream_size      = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]); pos += 4;
    let instruction_stream_size = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]); pos += 4;

    let _ = reserved;
    let _ = option_flags;
    let _ = index_format;
    let _ = index_to_loc_format;

    // Extract sub-streams (WOFF2 spec §5.3 Table 6):
    // nContour stream: i16 per glyph — number of contours
    // nPoints stream: 255UInt16 per contour of each simple glyph — points per contour
    // flag stream: u8 per point — triplet-encoded flags + coordinates
    // glyph stream: instruction count + instruction bytes per simple glyph
    // composite stream: component data for composite glyphs
    // bBox stream: bitmap + explicit bounding boxes
    // instruction stream: instructions for composite glyphs
    let n_contour_stream = data.get(pos..pos + n_contour_stream_size as usize)
        .ok_or(FontError::UnexpectedEof)?;
    pos += n_contour_stream_size as usize;
    let n_points_stream = data.get(pos..pos + n_points_stream_size as usize)
        .ok_or(FontError::UnexpectedEof)?;
    pos += n_points_stream_size as usize;
    let flag_stream = data.get(pos..pos + flag_stream_size as usize)
        .ok_or(FontError::UnexpectedEof)?;
    pos += flag_stream_size as usize;
    let glyph_stream = data.get(pos..pos + glyph_stream_size as usize)
        .ok_or(FontError::UnexpectedEof)?;
    pos += glyph_stream_size as usize;
    let composite_stream = data.get(pos..pos + composite_stream_size as usize)
        .ok_or(FontError::UnexpectedEof)?;
    pos += composite_stream_size as usize;
    let bbox_stream = data.get(pos..pos + bbox_stream_size as usize)
        .ok_or(FontError::UnexpectedEof)?;
    pos += bbox_stream_size as usize;
    let instruction_stream = data.get(pos..pos + instruction_stream_size as usize)
        .ok_or(FontError::UnexpectedEof)?;

    let mut out = Vec::<u8>::new();
    let mut npoints_pos = 0usize;
    let mut flag_pos = 0usize;
    let mut glyph_pos = 0usize;
    let mut composite_pos = 0usize;
    let mut instr_pos = 0usize;
    let bbox_bitmap_byte_count = num_glyphs.div_ceil(8).max(1) as usize;
    let bbox_data = bbox_stream.get(bbox_bitmap_byte_count..)
        .ok_or(FontError::UnexpectedEof)?;
    let bbox_bitmap = &bbox_stream[..bbox_bitmap_byte_count];
    let mut bbox_data_pos = 0usize;

    loca_entries.clear();

    for g in 0..num_glyphs as usize {
        loca_entries.push(out.len() as u32);
        let n_contours_signed = i16::from_be_bytes([
            n_contour_stream.get(g * 2).copied().ok_or(FontError::UnexpectedEof)?,
            n_contour_stream.get(g * 2 + 1).copied().ok_or(FontError::UnexpectedEof)?,
        ]);

        // Empty glyph
        if n_contours_signed == 0 {
            continue;
        }

        // Has explicit bounding box?
        let bbox_bit = (bbox_bitmap[g / 8] >> (7 - (g % 8))) & 1;
        let (x_min, y_min, x_max, y_max) = if bbox_bit != 0 {
            let bx = bbox_data.get(bbox_data_pos..bbox_data_pos + 8)
                .ok_or(FontError::UnexpectedEof)?;
            bbox_data_pos += 8;
            let x_min = i16::from_be_bytes([bx[0], bx[1]]);
            let y_min = i16::from_be_bytes([bx[2], bx[3]]);
            let x_max = i16::from_be_bytes([bx[4], bx[5]]);
            let y_max = i16::from_be_bytes([bx[6], bx[7]]);
            (x_min, y_min, x_max, y_max)
        } else {
            (0, 0, 0, 0)
        };

        if n_contours_signed < 0 {
            // Composite glyph — read from composite stream until done
            out.extend_from_slice(&n_contours_signed.to_be_bytes());
            out.extend_from_slice(&x_min.to_be_bytes());
            out.extend_from_slice(&y_min.to_be_bytes());
            out.extend_from_slice(&x_max.to_be_bytes());
            out.extend_from_slice(&y_max.to_be_bytes());
            loop {
                let flags = u16::from_be_bytes([
                    composite_stream.get(composite_pos).copied().ok_or(FontError::UnexpectedEof)?,
                    composite_stream.get(composite_pos + 1).copied().ok_or(FontError::UnexpectedEof)?,
                ]);
                out.extend_from_slice(&flags.to_be_bytes());
                composite_pos += 2;
                let glyph_index = u16::from_be_bytes([
                    composite_stream.get(composite_pos).copied().ok_or(FontError::UnexpectedEof)?,
                    composite_stream.get(composite_pos + 1).copied().ok_or(FontError::UnexpectedEof)?,
                ]);
                out.extend_from_slice(&glyph_index.to_be_bytes());
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
                let chunk_size = arg_size + scale_size;
                let chunk = composite_stream.get(composite_pos..composite_pos + chunk_size)
                    .ok_or(FontError::UnexpectedEof)?;
                out.extend_from_slice(chunk);
                composite_pos += chunk_size;
                if flags & MORE_COMPONENTS == 0 {
                    if flags & WE_HAVE_INSTRUCTIONS != 0 {
                        let instr_len = u16::from_be_bytes([
                            instruction_stream.get(instr_pos).copied().ok_or(FontError::UnexpectedEof)?,
                            instruction_stream.get(instr_pos + 1).copied().ok_or(FontError::UnexpectedEof)?,
                        ]);
                        instr_pos += 2;
                        out.extend_from_slice(&instr_len.to_be_bytes());
                        let instrs = instruction_stream.get(instr_pos..instr_pos + instr_len as usize)
                            .ok_or(FontError::UnexpectedEof)?;
                        out.extend_from_slice(instrs);
                        instr_pos += instr_len as usize;
                    }
                    break;
                }
            }
        } else {
            // Simple glyph.
            // WOFF2 spec §5.3: nPoints per contour are in the nPoints stream (not glyph stream).
            // Instruction data is in the glyph stream.
            // Flag/coordinate triplets are in the flag stream (not glyph stream).
            let n_contours = n_contours_signed as usize;
            let mut end_pts = Vec::with_capacity(n_contours);
            let mut total_points: u32 = 0;
            // Contours with 0 points are degenerate but legal in practice (e.g. space glyph
            // variants). Skip them so the font loads rather than rejecting the whole face.
            let mut actual_n_contours: i16 = 0;
            for _ in 0..n_contours {
                let (v, consumed) = read_255uint16(n_points_stream, npoints_pos)?;
                npoints_pos += consumed;
                if v == 0 {
                    // Skip the empty contour — it adds no points.
                    continue;
                }
                total_points = total_points
                    .checked_add(v as u32)
                    .ok_or(FontError::InvalidData("woff2: point count overflow"))?;
                if total_points > 65535 {
                    return Err(FontError::InvalidData("woff2: glyph exceeds 65535 points"));
                }
                end_pts.push(total_points - 1);
                actual_n_contours += 1;
            }
            // Read instruction length from glyph stream (must happen even if all contours empty).
            let (instr_len, consumed) = read_255uint16(glyph_stream, glyph_pos)?;
            glyph_pos += consumed;
            glyph_pos += instr_len as usize; // advance past instruction bytes

            // Consume flags/coordinates from flag stream (triplet encoding, WOFF2 spec §5.1).
            // Must advance flag_pos even when actual_n_contours == 0 (total_points == 0 → no-op).
            let n_pts = total_points as usize;
            let (flags_out, xs, ys) = decode_triplet(flag_stream, &mut flag_pos, n_pts)?;

            // If all contours were empty, treat as an empty glyph (no output, like n_contours==0).
            if actual_n_contours == 0 {
                continue;
            }

            // Write glyph header (actual_n_contours excludes the empty contours).
            out.extend_from_slice(&actual_n_contours.to_be_bytes());
            out.extend_from_slice(&x_min.to_be_bytes());
            out.extend_from_slice(&y_min.to_be_bytes());
            out.extend_from_slice(&x_max.to_be_bytes());
            out.extend_from_slice(&y_max.to_be_bytes());
            for ep in &end_pts {
                out.extend_from_slice(&(*ep as u16).to_be_bytes());
            }
            // Instruction bytes: re-read from glyph_stream at saved position.
            let instr_start = glyph_pos - instr_len as usize;
            let instrs = glyph_stream.get(instr_start..glyph_pos)
                .ok_or(FontError::UnexpectedEof)?;
            out.extend_from_slice(&instr_len.to_be_bytes());
            out.extend_from_slice(instrs);

            out.extend_from_slice(&flags_out);
            out.extend_from_slice(&xs);
            out.extend_from_slice(&ys);
        }

        // Pad to 4-byte boundary
        while !out.len().is_multiple_of(4) {
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

/// Decode triplet-encoded flags + coordinates (WOFF2 spec §5.1 Table 5).
fn decode_triplet(
    glyph_stream: &[u8],
    pos: &mut usize,
    n_pts: usize,
) -> TripletResult {
    let mut flags_out = Vec::with_capacity(n_pts);
    let mut xs = Vec::new();
    let mut ys = Vec::new();

    for _ in 0..n_pts {
        let flag = glyph_stream.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
        *pos += 1;

        let on_curve = (flag >> 7) & 1 == 0;
        let xy_flag = flag & 0x7F;

        let (_dx, _dy, flag_byte, x_bytes, y_bytes) = triplet_decode(glyph_stream, pos, xy_flag, on_curve)?;

        flags_out.push(flag_byte);
        xs.extend_from_slice(&x_bytes);
        ys.extend_from_slice(&y_bytes);
    }

    Ok((flags_out, xs, ys))
}

/// Per WOFF2 spec §5.1 Table 5: decode one (dx, dy) pair from triplet encoding.
fn triplet_decode(
    data: &[u8],
    pos: &mut usize,
    xy_flag: u8,
    on_curve: bool,
) -> TripletDecode {
    // Bit 0 of the output flag = on-curve
    let on_bit = if on_curve { 1u8 } else { 0u8 };

    let (dx, dy, flag_bits, x_out, y_out) = match xy_flag {
        0..=9 => {
            // Both x and y are 0 (no movement) — but flag bits encode short/same
            let b = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let nibble_x = ((b >> 4) as i32 + 1) * if xy_flag < 5 { 1 } else { -1 };
            let nibble_y = ((b & 0xF) as i32 + 1) * if (xy_flag % 5) < 2 { 1 } else { -1 };
            let _ = nibble_x;
            let _ = nibble_y;
            // Phase 0: use simple byte-reads for 4-bit nibble pairs
            let dx = if xy_flag < 5 { nibble_x } else { -nibble_x };
            let dy = if (xy_flag % 5) < 2 { nibble_y } else { -nibble_y };
            // Short vectors (bit 1 = x short positive, bit 2 = x short direction, etc.)
            let flag_byte = on_bit | 0x32; // SHORT_X | SHORT_Y with direction
            (dx, dy, flag_byte, vec![dx.unsigned_abs() as u8], vec![dy.unsigned_abs() as u8])
        }
        10..=19 => {
            let b0 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let b1 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let dx = (b0 as i32 + 1) * if xy_flag < 15 { 1 } else { -1 };
            let dy = b1 as i32 + 1;
            let flag_byte = on_bit | 0x12;
            (dx, dy, flag_byte, vec![dx.unsigned_abs() as u8], vec![dy.unsigned_abs() as u8])
        }
        20..=83 => {
            let b = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let offset = xy_flag - 20;
            let dx_idx = offset / 16;
            let dy_idx = offset % 16;
            let dx = (dx_idx as i32 + 1) * 64 + ((b >> 4) as i32 + 1) * 4;
            let dy = (dy_idx as i32 + 1) * 64 + ((b & 0xF) as i32 + 1) * 4;
            let flag_byte = on_bit | 0x32;
            (dx, dy, flag_byte, vec![(dx & 0xFF) as u8], vec![(dy & 0xFF) as u8])
        }
        84..=115 => {
            let b = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let offset = xy_flag - 84;
            let dx = (offset / 4) as i32 * 256 + b as i32 + 1;
            let dy_flag = offset % 4;
            let dy_nibble = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let dy = (dy_flag as i32) * 256 + dy_nibble as i32 + 1;
            let flag_byte = on_bit;
            let dx_bytes = (dx as i16).to_be_bytes().to_vec();
            let dy_bytes = (dy as i16).to_be_bytes().to_vec();
            (dx, dy, flag_byte, dx_bytes, dy_bytes)
        }
        116..=118 => {
            let b0 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let b1 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let dx = i16::from_be_bytes([b0, b1]) as i32;
            let dy_sign = if xy_flag == 117 { 1 } else { -1 };
            let b2 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let b3 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let dy = i16::from_be_bytes([b2, b3]) as i32 * dy_sign;
            let flag_byte = on_bit;
            let dx_bytes = (dx as i16).to_be_bytes().to_vec();
            let dy_bytes = (dy as i16).to_be_bytes().to_vec();
            (dx, dy, flag_byte, dx_bytes, dy_bytes)
        }
        119..=127 => {
            let b0 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let b1 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let dx = i16::from_be_bytes([b0, b1]) as i32;
            let b2 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let b3 = data.get(*pos).copied().ok_or(FontError::UnexpectedEof)?;
            *pos += 1;
            let dy = i16::from_be_bytes([b2, b3]) as i32;
            let flag_byte = on_bit;
            let dx_bytes = (dx as i16).to_be_bytes().to_vec();
            let dy_bytes = (dy as i16).to_be_bytes().to_vec();
            (dx, dy, flag_byte, dx_bytes, dy_bytes)
        }
        _ => {
            return Err(FontError::InvalidData("woff2: unknown triplet flag"));
        }
    };

    Ok((dx, dy, flag_bits, x_out, y_out))
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

    // Reconstruct table data for each entry
    let mut table_data: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    let mut decomp_pos = 0usize;
    let mut loca_entries: Vec<u32> = Vec::new();

    // We need two passes: first decode glyf (which also produces loca),
    // then fill in loca when we encounter it.
    // Scan for glyf index first.
    let glyf_idx = entries.iter().position(|e| &e.tag == b"glyf");
    let loca_idx = entries.iter().position(|e| &e.tag == b"loca");

    // Read head table to find indexToLocFormat (needed for loca reconstruction)
    // We'll derive it after reconstruction — just use 1 (long offsets) by default.
    let mut decoded_glyf: Option<Vec<u8>> = None;

    for (i, entry) in entries.iter().enumerate() {
        let byte_count = entry.xform_length.unwrap_or(entry.orig_length) as usize;
        let chunk = decompressed.get(decomp_pos..decomp_pos + byte_count)
            .ok_or(FontError::UnexpectedEof)?
            .to_vec();
        decomp_pos += byte_count;

        if Some(i) == glyf_idx && entry.xform_length.is_some() {
            // Transformed glyf — decode it
            let glyf_out = decode_transformed_glyf(&chunk, &mut loca_entries, 1)?;
            decoded_glyf = Some(glyf_out.clone());
            table_data.push(glyf_out);
        } else if Some(i) == loca_idx && entry.xform_length.is_some() {
            // Transformed loca — reconstruct from loca_entries
            let loca_ref = decoded_glyf.as_ref();
            let glyf_decoded = loca_ref.is_some();
            if glyf_decoded && !loca_entries.is_empty() {
                let mut loca_out = Vec::with_capacity(loca_entries.len() * 4);
                for off in &loca_entries {
                    loca_out.extend_from_slice(&off.to_be_bytes());
                }
                table_data.push(loca_out);
            } else {
                // Fallback: pass chunk through (zero-length loca)
                table_data.push(chunk);
            }
        } else {
            table_data.push(chunk);
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

    // Build a minimal transformed glyf data block for use in unit tests.
    // Encodes `num_glyphs` glyphs: the first glyph is a simple glyph with
    // `n_contours` contours each having `points_per_contour` points (u8 slices).
    // All other glyphs are empty (n_contours == 0).
    fn make_glyf_transform(
        num_glyphs: u16,
        n_contours: u16,
        points_per_contour: &[u8],   // one 255UInt16 value per contour (all < 253 → 1 byte)
        flag_bytes: &[u8],           // one flag byte per total point
    ) -> Vec<u8> {
        // Build each stream independently.
        let mut ncontour_stream: Vec<u8> = Vec::new();
        // Glyph 0: n_contours
        ncontour_stream.extend_from_slice(&(n_contours as i16).to_be_bytes());
        // Remaining glyphs: empty
        for _ in 1..num_glyphs {
            ncontour_stream.extend_from_slice(&0i16.to_be_bytes());
        }

        // nPoints stream: one 255UInt16 per contour of simple glyph 0.
        let npoints_stream: Vec<u8> = points_per_contour.to_vec();

        // flag stream: triplet bytes for all points.
        // Use triplet value 0 (on-curve, dx=nibble, dy=nibble) — requires 1 extra byte.
        // But for simplicity use xy_flag = 10 (off-curve 1-byte x, 1-byte y) — 2 bytes per point.
        // Actually for zero points, flag_bytes is empty.
        let flag_stream: Vec<u8> = flag_bytes.to_vec();

        // glyph stream: instruction length (0) as 255UInt16 single byte 0.
        let glyph_stream: Vec<u8> = vec![0u8]; // 1 glyph × instr_len=0

        // bbox bitmap: 1 byte for up to 8 glyphs; bit for glyph 0 = 0 (no explicit bbox).
        let bbox_bitmap_size = num_glyphs.div_ceil(8).max(1) as usize;
        let mut bbox_stream: Vec<u8> = vec![0u8; bbox_bitmap_size]; // all bits = 0
        // No explicit bbox entries follow.
        // composite, instruction streams: empty.
        let composite_stream: Vec<u8> = Vec::new();
        let instruction_stream: Vec<u8> = Vec::new();

        let mut data: Vec<u8> = Vec::new();
        // Header (36 bytes)
        data.extend_from_slice(&0u16.to_be_bytes()); // reserved
        data.extend_from_slice(&0u16.to_be_bytes()); // option_flags
        data.extend_from_slice(&num_glyphs.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes()); // index_format
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
        // points_per_contour = [0] → contour has 0 points → skip it.
        let data = make_glyf_transform(1, 1, &[0u8], &[]);
        let mut loca = Vec::new();
        let result = super::decode_transformed_glyf(&data, &mut loca, 1);
        assert!(result.is_ok(), "zero-point contour should be accepted: {:?}", result.err());
    }

    #[test]
    fn glyf_transform_normal_glyph_decoded() {
        // A simple glyph with 1 contour of 1 point — encoded with triplet byte.
        // xy_flag 0..=9 range consumes 1 extra byte; use flag=0 (on-curve, 4-bit nibbles).
        // points_per_contour = [1] (one point, value < 253 → single byte 1)
        // flag byte: 0x00 (xy_flag=0, on-curve) + nibble byte 0x11 = 2 bytes total for 1 point.
        let data = make_glyf_transform(1, 1, &[1u8], &[0x00u8, 0x11u8]);
        let mut loca = Vec::new();
        let result = super::decode_transformed_glyf(&data, &mut loca, 1);
        assert!(result.is_ok(), "normal 1-point glyph should decode: {:?}", result.err());
        let glyf = result.unwrap();
        // Glyph 0 should be non-empty (has 1 contour).
        assert!(!glyf.is_empty(), "decoded glyf must not be empty");
        // The first i16 in output is numberOfContours = 1.
        let n_contours = i16::from_be_bytes([glyf[0], glyf[1]]);
        assert_eq!(n_contours, 1);
    }

    #[test]
    fn glyf_transform_empty_glyph_produces_no_output() {
        // n_contours_signed == 0 → empty glyph, no bytes emitted.
        let data = make_glyf_transform(1, 0, &[], &[]);
        let mut loca = Vec::new();
        let result = super::decode_transformed_glyf(&data, &mut loca, 1);
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
