//! WOFF 2.0 container: Brotli stream decoding, transform reversal
//! (glyf/loca/hmtx), and SFNT reconstruction.
//!
//! Structure (spec: WOFF File Format 2.0, W3C Recommendation): 48-byte
//! header, compact table directory (`flags [tag] origLength
//! [transformLength]` with UIntBase128 sizes), then one Brotli-compressed
//! block holding every table back to back. `glyf` (transform version 0) is
//! re-encoded as seven substreams; `loca` (version 0, `transformLength`
//! always 0) is rebuilt from the glyf decode; `hmtx` (version 1) may omit
//! side-bearing arrays for reconstruction from glyph bboxes. Reconstruction
//! recomputes table checksums and `head.checkSumAdjustment`, and sets
//! `head.flags` bit 11 (lossless-transform marker) per spec.
//!
//! Memory discipline: the decompressed stream is capped and the directory's
//! declared sizes are validated against it before any per-table slicing, so
//! hostile `origLength` values cannot drive allocation.

use crate::error::{ParseError, RawTag};
use crate::reader::{be_u16_at, be_u32_at, Reader};
use alloc::vec::Vec;
use std::io::Read;

const TAG: RawTag = *b"wOF2";
const WOFF2_SIGNATURE: u32 = 0x774F_4632; // 'wOF2'
const TTC_FLAVOR: u32 = 0x7474_6366; // 'ttcf'

/// Header size.
const HEADER: usize = 48;

/// Cap on the decompressed table stream (memory-bomb guard; real fonts are
/// well under 64 MiB).
const MAX_STREAM: usize = 512 << 20;

/// The 63 known table tags (spec §4.1), indexed by the flags byte's low six
/// bits; 63 means an explicit tag follows.
const KNOWN_TAGS: [[u8; 4]; 63] = [
    *b"cmap", *b"head", *b"hhea", *b"hmtx", *b"maxp", *b"name", *b"OS/2", *b"post", *b"cvt ",
    *b"fpgm", *b"glyf", *b"loca", *b"prep", *b"CFF ", *b"VORG", *b"EBDT", *b"EBLC", *b"gasp",
    *b"hdmx", *b"kern", *b"LTSH", *b"PCLT", *b"VDMX", *b"vhea", *b"vmtx", *b"BASE", *b"GDEF",
    *b"GPOS", *b"GSUB", *b"EBSC", *b"JSTF", *b"MATH", *b"CBDT", *b"CBLC", *b"COLR", *b"CPAL",
    *b"SVG ", *b"sbix", *b"acnt", *b"avar", *b"bdat", *b"bloc", *b"bsln", *b"cvar", *b"fdsc",
    *b"feat", *b"fmtx", *b"fvar", *b"gvar", *b"hsty", *b"just", *b"lcar", *b"mort", *b"morx",
    *b"opbd", *b"prop", *b"trak", *b"Zapf", *b"Silf", *b"Glat", *b"Gloc", *b"Feat", *b"Sill",
];

fn tag_is(a: &[u8; 4], b: &[u8; 4]) -> bool {
    a == b
}

/// One decoded table-directory entry.
struct DirEntry {
    tag: [u8; 4],
    orig_length: usize,
    transform_length: usize,
    transformed: bool,
}

impl DirEntry {
    /// Length of this entry's data in the decompressed stream.
    fn stream_len(&self) -> usize {
        if self.transformed {
            self.transform_length
        } else {
            self.orig_length
        }
    }
}

/// Decode a WOFF 2.0 container into an owned, validated SFNT.
///
/// # Errors
/// [`ParseError::InvalidSfnt`] for container-level violations;
/// [`ParseError::UnsupportedWoff2Transform`] for undefined transform
/// versions; [`ParseError::InvalidGlyphFormat`] for glyph-transform
/// inconsistencies.
pub fn decode(data: &[u8]) -> Result<crate::OwnedFont, ParseError> {
    if data.len() < HEADER {
        return Err(ParseError::TruncatedTable {
            tag: TAG,
            expected: HEADER,
            got: data.len(),
        });
    }
    if be_u32_at(data, 0, TAG)? != WOFF2_SIGNATURE {
        return Err(ParseError::InvalidSfnt);
    }
    let flavor = be_u32_at(data, 4, TAG)?;
    if flavor == TTC_FLAVOR {
        // Collection directories are out of scope for v1 (documented).
        return Err(ParseError::InvalidSfnt);
    }
    let num_tables = usize::from(be_u16_at(data, 12, TAG)?);
    let total_compressed = be_u32_at(data, 20, TAG)?;
    if num_tables == 0 {
        return Err(ParseError::InvalidSfnt);
    }
    // Table directory: variable-size entries, parsed as a stream.
    let mut r = Reader::within(data, HEADER, data.len(), TAG);
    let mut entries = Vec::with_capacity(num_tables);
    for _ in 0..num_tables {
        let flags = r.u8()?;
        let tag_idx = usize::from(flags & 0x3F);
        let version = (flags >> 6) & 0x03;
        let tag = if tag_idx == 63 {
            r.tag_bytes()?
        } else {
            KNOWN_TAGS
                .get(tag_idx)
                .copied()
                .ok_or(ParseError::InvalidSfnt)?
        };
        let orig_length = r.base128()?;
        let (transformed, transform_length) = if tag_is(&tag, b"glyf") || tag_is(&tag, b"loca") {
            match version {
                0 => (true, r.base128()?),
                3 => (false, 0),
                _ => return Err(ParseError::UnsupportedWoff2Transform),
            }
        } else if tag_is(&tag, b"hmtx") {
            match version {
                0 => (false, 0),
                1 => (true, r.base128()?),
                _ => return Err(ParseError::UnsupportedWoff2Transform),
            }
        } else if version != 0 {
            return Err(ParseError::UnsupportedWoff2Transform);
        } else {
            (false, 0)
        };
        entries.push(DirEntry {
            tag,
            orig_length: usize::try_from(orig_length).map_err(|_| ParseError::InvalidSfnt)?,
            transform_length: usize::try_from(transform_length)
                .map_err(|_| ParseError::InvalidSfnt)?,
            transformed,
        });
    }
    let dir_end = HEADER + r.pos();
    let comp_len = usize::try_from(total_compressed).map_err(|_| ParseError::InvalidSfnt)?;
    let comp_end = dir_end
        .checked_add(comp_len)
        .ok_or(ParseError::InvalidSfnt)?;
    if comp_end > data.len() {
        return Err(ParseError::TruncatedTable {
            tag: TAG,
            expected: comp_end,
            got: data.len(),
        });
    }
    let stream = data
        .get(dir_end..comp_end)
        .ok_or(ParseError::TruncatedTable {
            tag: TAG,
            expected: comp_end,
            got: data.len(),
        })?;
    let stream = decompress(stream)?;
    // Sum of declared sizes must equal the stream exactly (spec MUST).
    let mut expected_total = 0usize;
    for e in &entries {
        expected_total = expected_total
            .checked_add(e.stream_len())
            .ok_or(ParseError::InvalidSfnt)?;
    }
    if expected_total != stream.len() {
        return Err(ParseError::InvalidSfnt);
    }
    // Slice the stream per entry, in directory order.
    let mut pos = 0usize;
    let mut raw_tables: Vec<(&DirEntry, &[u8])> = Vec::with_capacity(entries.len());
    for e in &entries {
        let bytes = crate::reader::slice_at(&stream, pos, e.stream_len(), TAG)?;
        pos += e.stream_len();
        raw_tables.push((e, bytes));
    }
    // hhea.numberOfHMetrics is needed by the hmtx transform; hhea is never
    // transformed, so its raw bytes are in the stream.
    let num_h_metrics = raw_tables
        .iter()
        .find(|(e, _)| tag_is(&e.tag, b"hhea"))
        .and_then(|(_, b)| be_u16_at(b, 34, TAG).ok());
    // Per-glyph xMin values and the reconstructed glyf/loca pair, from the
    // transformed glyf entry (needed by the hmtx transform, which may
    // appear before glyf in directory order). Null-transform glyf keeps its
    // lsbs inline.
    let mut x_mins: Vec<i16> = Vec::new();
    let mut num_glyphs: Option<u16> = None;
    let mut reconstructed: Option<(Vec<u8>, Vec<u8>)> = None;
    let mut had_transformed_glyf = false;
    for (entry, bytes) in &raw_tables {
        if entry.transformed && tag_is(&entry.tag, b"glyf") {
            let index_format = be_u16_at(bytes, 6, TAG)?;
            let r = reconstruct_glyf(bytes, index_format)?;
            x_mins = r.x_mins;
            num_glyphs = Some(r.num_glyphs);
            reconstructed = Some((r.glyf, r.loca));
            had_transformed_glyf = true;
        }
    }
    // Reconstruct tables (everything except glyf/loca flows through).
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(entries.len());
    let mut had_transformed_loca_slot = false;
    let mut had_loca_entry = false;
    for (entry, bytes) in &raw_tables {
        if tag_is(&entry.tag, b"loca") {
            had_loca_entry = true;
        }
        if entry.transformed && tag_is(&entry.tag, b"loca") {
            // Zero-byte placeholder; the real loca comes from the glyf decode.
            had_transformed_loca_slot = true;
            continue;
        }
        if entry.transformed && tag_is(&entry.tag, b"glyf") {
            if let Some((glyf, loca)) = reconstructed.take() {
                tables.push((*b"glyf", glyf));
                tables.push((*b"loca", loca));
            }
            continue;
        }
        if entry.transformed && tag_is(&entry.tag, b"hmtx") {
            let hmtx = reconstruct_hmtx(bytes, &x_mins, num_glyphs.unwrap_or(0), num_h_metrics)?;
            tables.push((*b"hmtx", hmtx));
            continue;
        }
        tables.push((entry.tag, bytes.to_vec()));
    }
    // Transformed glyf requires a paired loca entry (spec MUST).
    if had_transformed_glyf && !had_loca_entry {
        return Err(ParseError::InvalidGlyphFormat);
    }
    if !had_transformed_glyf && had_transformed_loca_slot {
        return Err(ParseError::InvalidGlyphFormat);
    }
    // WOFF2 reconstruction sets head.flags bit 11 (lossless transform).
    for (tag, head) in tables.iter_mut() {
        if tag_is(tag, b"head") && head.len() >= 18 {
            let flags = head
                .get(16..18)
                .and_then(|s| be_u16_at(s, 0, TAG).ok())
                .unwrap_or(0);
            let flags = flags | 0x0800;
            if let Some(slot) = head.get_mut(16..18) {
                slot.copy_from_slice(&flags.to_be_bytes());
            }
        }
    }
    let sfnt = crate::font::build_sfnt(flavor, &mut tables)?;
    crate::OwnedFont::from_sfnt(sfnt)
}

/// Brotli-decompress the table stream with a hard output cap.
fn decompress(input: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut out = Vec::new();
    let mut reader = brotli_decompressor::Decompressor::new(input, 4096);
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if out.len().saturating_add(n) > MAX_STREAM {
                    return Err(ParseError::InvalidSfnt);
                }
                out.extend_from_slice(chunk.get(..n).unwrap_or(&chunk));
            }
            Err(_) => return Err(ParseError::InvalidSfnt),
        }
    }
    Ok(out)
}

/// Transformed glyf header (36 bytes of substream sizes).
struct GlyfStreams {
    num_glyphs: u16,
    index_format: u16,
    overlap_bitmap: Option<core::ops::Range<usize>>,
    n_contour: core::ops::Range<usize>,
    n_points: core::ops::Range<usize>,
    flag: core::ops::Range<usize>,
    glyph: core::ops::Range<usize>,
    composite: core::ops::Range<usize>,
    bbox_bitmap: core::ops::Range<usize>,
    bbox_values: core::ops::Range<usize>,
    instruction: core::ops::Range<usize>,
}

/// Lay out the transformed-glyf substreams and validate them against the
/// table length.
fn glyf_streams(transformed: &[u8]) -> Result<GlyfStreams, ParseError> {
    if transformed.len() < 36 {
        return Err(ParseError::TruncatedTable {
            tag: *b"glyf",
            expected: 36,
            got: transformed.len(),
        });
    }
    let index_format = be_u16_at(transformed, 6, TAG)?;
    if index_format > 1 {
        return Err(ParseError::InvalidGlyphFormat);
    }
    let option_flags = be_u16_at(transformed, 2, TAG)?;
    let num_glyphs = be_u16_at(transformed, 4, TAG)?;
    let size = |at: usize| -> Result<usize, ParseError> {
        usize::try_from(be_u32_at(transformed, at, TAG)?).map_err(|_| ParseError::InvalidSfnt)
    };
    let n_contour_size = size(8)?;
    let n_points_size = size(12)?;
    let flag_size = size(16)?;
    let glyph_size = size(20)?;
    let composite_size = size(24)?;
    let bbox_size = size(28)?;
    let instruction_size = size(32)?;
    // Stream layout after the 36-byte header, in spec order.
    let mut base = 36usize;
    let mut take = |size: usize| -> Result<core::ops::Range<usize>, ParseError> {
        let end = base
            .checked_add(size)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end > transformed.len() {
            return Err(ParseError::TruncatedTable {
                tag: *b"glyf",
                expected: end,
                got: transformed.len(),
            });
        }
        let range = base..end;
        base = end;
        Ok(range)
    };
    let n_contour = take(n_contour_size)?;
    let n_points = take(n_points_size)?;
    let flag = take(flag_size)?;
    let glyph = take(glyph_size)?;
    let composite = take(composite_size)?;
    // bboxStream is the combined bboxBitmap + Int16 stream.
    let bitmap_len = 4 * usize::from(num_glyphs).div_ceil(32);
    if bbox_size < bitmap_len {
        return Err(ParseError::InvalidGlyphFormat);
    }
    let bbox = take(bbox_size)?;
    let bbox_bitmap = bbox.start..bbox.start + bitmap_len;
    let bbox_values = bbox.start + bitmap_len..bbox.end;
    let instruction = take(instruction_size)?;
    let overlap_bitmap = if option_flags & 0x0001 != 0 {
        Some(take(usize::from(num_glyphs).div_ceil(8))?)
    } else {
        None
    };
    Ok(GlyfStreams {
        num_glyphs,
        index_format,
        overlap_bitmap,
        n_contour,
        n_points,
        flag,
        glyph,
        composite,
        bbox_bitmap,
        bbox_values,
        instruction,
    })
}

/// Reconstructed TrueType tables from a transformed glyf table.
struct GlyfReconstruction {
    /// Rebuilt `glyf` table.
    glyf: Vec<u8>,
    /// Rebuilt `loca` table (matches the declared index format).
    loca: Vec<u8>,
    /// Per-glyph `xMin` (feeds the hmtx transform reversal).
    x_mins: Vec<i16>,
    /// Glyph count from the transformed header.
    num_glyphs: u16,
}

/// Reconstruct the classic `glyf` + `loca` pair from a transformed glyf
/// table (spec §5.1).
fn reconstruct_glyf(
    transformed: &[u8],
    index_format: u16,
) -> Result<GlyfReconstruction, ParseError> {
    let s = glyf_streams(transformed)?;
    if s.index_format != index_format {
        return Err(ParseError::InvalidGlyphFormat);
    }
    let bitmap =
        crate::reader::slice_at(transformed, s.bbox_bitmap.start, s.bbox_bitmap.len(), TAG)?;
    let mut n_contour = Reader::within(transformed, s.n_contour.start, s.n_contour.end, TAG);
    let mut n_points = Reader::within(transformed, s.n_points.start, s.n_points.end, TAG);
    let mut flags = Reader::within(transformed, s.flag.start, s.flag.end, TAG);
    let mut glyph = Reader::within(transformed, s.glyph.start, s.glyph.end, TAG);
    let mut composite = Reader::within(transformed, s.composite.start, s.composite.end, TAG);
    let mut bboxes = Reader::within(transformed, s.bbox_values.start, s.bbox_values.end, TAG);
    let mut instructions = Reader::within(transformed, s.instruction.start, s.instruction.end, TAG);

    let mut loca: Vec<u32> = Vec::with_capacity(usize::from(s.num_glyphs) + 1);
    let mut glyf_out: Vec<u8> = Vec::new();
    let mut x_mins: Vec<i16> = Vec::with_capacity(usize::from(s.num_glyphs));
    loca.push(0);

    for gid in 0..usize::from(s.num_glyphs) {
        let has_bbox = bitmap
            .get(gid / 8)
            .is_some_and(|b| b & (0x80 >> (gid % 8)) != 0);
        let overlap = match &s.overlap_bitmap {
            Some(range) => crate::reader::slice_at(transformed, range.start + gid / 8, 1, TAG)?
                .first()
                .is_some_and(|b| b & (0x80 >> (gid % 8)) != 0),
            None => false,
        };
        let n_contours = n_contour.i16()?;
        match n_contours {
            0 => {
                // Empty glyph: no bytes, explicit bbox forbidden.
                if has_bbox {
                    return Err(ParseError::InvalidGlyphFormat);
                }
                x_mins.push(0);
            }
            c if c > 0 => {
                // Simple glyph.
                let mut ends: Vec<u16> = Vec::with_capacity(usize::try_from(c).unwrap_or(0));
                let mut total = 0u32;
                for _ in 0..c {
                    let n = u32::from(n_points.u255()?);
                    if n == 0 {
                        return Err(ParseError::InvalidGlyphFormat);
                    }
                    total = total.checked_add(n).ok_or(ParseError::InvalidGlyphFormat)?;
                    // nPoints are per-contour counts; endPts are cumulative
                    // sums minus one (spec §5.1).
                    ends.push(
                        u16::try_from(total - 1).map_err(|_| ParseError::InvalidGlyphFormat)?,
                    );
                }
                // endPts are inclusive indices: total points = last + 1.
                let total_points = usize::from(ends.last().copied().unwrap_or(0)) + 1;
                let mut triplets: Vec<(u8, [u8; 4])> = Vec::with_capacity(total_points);
                for _ in 0..total_points {
                    let flag = flags.u8()?;
                    let n_coord = coord_bytes_for_flag(flag);
                    let bytes = glyph.bytes(n_coord)?;
                    let mut quad = [0u8; 4];
                    if let Some(slot) = quad.get_mut(..n_coord) {
                        slot.copy_from_slice(bytes);
                    }
                    triplets.push((flag, quad));
                }
                let instruction_len = glyph.u255()?;
                let instr = instructions.bytes(usize::from(instruction_len))?;
                // Decode points (delta coordinates) and compute the bbox.
                let mut points: Vec<(i32, i32, bool)> = Vec::with_capacity(total_points);
                let (mut x, mut y) = (0i64, 0i64);
                for (flag, quad) in &triplets {
                    let (dx, dy, on_curve) = decode_triplet(*flag, quad);
                    x += i64::from(dx);
                    y += i64::from(dy);
                    let (x, y) = (coord_i16(x)?, coord_i16(y)?);
                    points.push((i32::from(x), i32::from(y), on_curve));
                }
                let (x_min, y_min, x_max, y_max) = if has_bbox {
                    (bboxes.i16()?, bboxes.i16()?, bboxes.i16()?, bboxes.i16()?)
                } else {
                    bbox_of(&points)?
                };
                x_mins.push(x_min);
                // Encode: bbox, endPts, instructions, flags, coords.
                write_i16(&mut glyf_out, n_contours);
                write_i16(&mut glyf_out, x_min);
                write_i16(&mut glyf_out, y_min);
                write_i16(&mut glyf_out, x_max);
                write_i16(&mut glyf_out, y_max);
                for &e in &ends {
                    write_u16(&mut glyf_out, e);
                }
                write_u16(&mut glyf_out, instruction_len);
                glyf_out.extend_from_slice(instr);
                // Flags array first (on-curve bit + overlap on the first
                // point), then all x deltas, then all y deltas — the classic
                // glyf layout.
                let mut flag_bytes = Vec::with_capacity(points.len());
                let mut x_bytes = Vec::with_capacity(points.len() * 2);
                let mut y_bytes = Vec::with_capacity(points.len() * 2);
                let mut prev_x = 0i32;
                let mut prev_y = 0i32;
                for (i, &(x, y, on_curve)) in points.iter().enumerate() {
                    let mut flag = if on_curve {
                        crate::tables::glyf::FLAG_ON_CURVE
                    } else {
                        0
                    };
                    if i == 0 && overlap {
                        flag |= crate::tables::glyf::FLAG_OVERLAP_SIMPLE;
                    }
                    flag_bytes.push(flag);
                    let dx = delta_i16(i64::from(x) - i64::from(prev_x))?;
                    let dy = delta_i16(i64::from(y) - i64::from(prev_y))?;
                    x_bytes.extend_from_slice(&dx.to_be_bytes());
                    y_bytes.extend_from_slice(&dy.to_be_bytes());
                    prev_x = x;
                    prev_y = y;
                }
                glyf_out.extend_from_slice(&flag_bytes);
                glyf_out.extend_from_slice(&x_bytes);
                glyf_out.extend_from_slice(&y_bytes);
            }
            -1 => {
                // Composite glyph: explicit bbox required (spec MUST).
                if !has_bbox {
                    return Err(ParseError::InvalidGlyphFormat);
                }
                let xm = bboxes.i16()?;
                let ym = bboxes.i16()?;
                let x_max = bboxes.i16()?;
                let y_max = bboxes.i16()?;
                x_mins.push(xm);
                write_i16(&mut glyf_out, -1);
                write_i16(&mut glyf_out, xm);
                write_i16(&mut glyf_out, ym);
                write_i16(&mut glyf_out, x_max);
                write_i16(&mut glyf_out, y_max);
                let mut have_instructions = false;
                loop {
                    let comp_flags = composite.u16()?;
                    let glyph_index = composite.u16()?;
                    write_u16(&mut glyf_out, comp_flags);
                    write_u16(&mut glyf_out, glyph_index);
                    if comp_flags & 0x0001 != 0 {
                        // ARGS_ARE_WORDS.
                        let a = composite.i16()?;
                        let b = composite.i16()?;
                        write_i16(&mut glyf_out, a);
                        write_i16(&mut glyf_out, b);
                    } else {
                        let a = composite.i8()?;
                        let b = composite.i8()?;
                        glyf_out.push(a.to_be_bytes()[0]);
                        glyf_out.push(b.to_be_bytes()[0]);
                    }
                    let scale_count = if comp_flags & 0x0008 != 0 {
                        1
                    } else if comp_flags & 0x0040 != 0 {
                        2
                    } else if comp_flags & 0x0080 != 0 {
                        4
                    } else {
                        0
                    };
                    for _ in 0..scale_count {
                        write_i16(&mut glyf_out, composite.i16()?);
                    }
                    if comp_flags & crate::tables::glyf::WE_HAVE_INSTRUCTIONS != 0 {
                        have_instructions = true;
                    }
                    if comp_flags & crate::tables::glyf::MORE_COMPONENTS == 0 {
                        break;
                    }
                }
                if have_instructions {
                    let instruction_len = glyph.u255()?;
                    write_u16(&mut glyf_out, instruction_len);
                    let instr = instructions.bytes(usize::from(instruction_len))?;
                    glyf_out.extend_from_slice(instr);
                }
            }
            _ => return Err(ParseError::InvalidGlyphFormat),
        }
        // Records are 2-byte aligned (short loca compatibility).
        if glyf_out.len() % 2 != 0 {
            glyf_out.push(0);
        }
        let len = u32::try_from(glyf_out.len()).map_err(|_| ParseError::InvalidGlyphFormat)?;
        loca.push(len);
    }
    // Encode loca in the format the header declares.
    let mut loca_out = Vec::with_capacity(loca.len() * if index_format == 0 { 2 } else { 4 });
    if index_format == 0 {
        for v in &loca {
            if v % 2 != 0 {
                return Err(ParseError::InvalidGlyphFormat);
            }
            let half = v / 2;
            let half = u16::try_from(half).map_err(|_| ParseError::InvalidGlyphFormat)?;
            write_u16(&mut loca_out, half);
        }
    } else {
        for v in &loca {
            write_u32(&mut loca_out, *v);
        }
    }
    Ok(GlyfReconstruction {
        glyf: glyf_out,
        loca: loca_out,
        x_mins,
        num_glyphs: s.num_glyphs,
    })
}

/// Coordinate byte count for a triplet flag (spec §5.1: 0-83 → 1, 84-119 →
/// 2, 120-123 → 3, 124-127 → 4).
fn coord_bytes_for_flag(flag: u8) -> usize {
    match flag & 0x7F {
        0..=83 => 1,
        84..=119 => 2,
        120..=123 => 3,
        _ => 4,
    }
}

/// Decode one WOFF2 triplet into (dx, dy, on_curve) — the spec's 128-entry
/// encoding (flag bit 7 = on-curve when clear).
fn decode_triplet(flag: u8, bytes: &[u8; 4]) -> (i32, i32, bool) {
    let on_curve = flag & 0x80 == 0;
    let f = usize::from(flag & 0x7F);
    let signed = |v: i32, positive: bool| if positive { v } else { -v };
    let (x, y) = match f {
        0..=9 => {
            // x = 0; y from 8 bits, biases 0/256/512/768/1024, alternating sign.
            let yv = i32::from(bytes[0]);
            let bias = 256 * i32::try_from(f / 2).unwrap_or(0);
            (0, signed(yv + bias, f % 2 == 1))
        }
        10..=19 => {
            // y = 0; x from 8 bits, same bias ladder.
            let g = f - 10;
            let xv = i32::from(bytes[0]);
            let bias = 256 * i32::try_from(g / 2).unwrap_or(0);
            (signed(xv + bias, g % 2 == 1), 0)
        }
        20..=83 => {
            // Nibbles: x hi, y lo; dx from {1,17,33,49} by group of 16,
            // dy from {1,17,33,49} by group of 4, signs cycling.
            let g = f - 20;
            let b = i32::from(bytes[0]);
            let (xv, yv) = (b >> 4, b & 0xF);
            let dx = 1 + 16 * i32::try_from(g / 16).unwrap_or(0);
            let dy = nibble_bias(g % 16 / 4);
            let (sx, sy) = sign_pair(g % 4);
            (signed(xv + dx, sx), signed(yv + dy, sy))
        }
        84..=119 => {
            // Bytes: x 8 bits, y 8 bits; biases {1,257,513}.
            let g = f - 84;
            let xv = i32::from(bytes[0]);
            let yv = i32::from(bytes[1]);
            let dx = 1 + 256 * i32::try_from(g / 16).unwrap_or(0);
            let dy = byte_bias(g % 16 / 4);
            let (sx, sy) = sign_pair(g % 4);
            (signed(xv + dx, sx), signed(yv + dy, sy))
        }
        120..=123 => {
            // 12 + 12 bits, no bias.
            let xv = (i32::from(bytes[0]) << 4) | i32::from(bytes[1] >> 4);
            let yv = (i32::from(bytes[1] & 0xF) << 8) | i32::from(bytes[2]);
            let (sx, sy) = sign_pair(f - 120);
            (signed(xv, sx), signed(yv, sy))
        }
        _ => {
            // 16 + 16 bits, no bias.
            let xv = (i32::from(bytes[0]) << 8) | i32::from(bytes[1]);
            let yv = (i32::from(bytes[2]) << 8) | i32::from(bytes[3]);
            let (sx, sy) = sign_pair(f - 124);
            (signed(xv, sx), signed(yv, sy))
        }
    };
    (x, y, on_curve)
}

/// Nibble-coordinate biases {1, 17, 33, 49} by group of four.
const fn nibble_bias(i: usize) -> i32 {
    match i & 3 {
        0 => 1,
        1 => 17,
        2 => 33,
        _ => 49,
    }
}

/// Byte-coordinate biases {1, 257, 513} by group of four.
const fn byte_bias(i: usize) -> i32 {
    match i & 3 {
        0 => 1,
        1 => 257,
        _ => 513,
    }
}

/// Sign pairs for nibble/byte triplets: 0:(-,-) 1:(+,-) 2:(-,+) 3:(+,+).
const fn sign_pair(i: usize) -> (bool, bool) {
    match i & 3 {
        0 => (false, false),
        1 => (true, false),
        2 => (false, true),
        _ => (true, true),
    }
}

/// Coordinates must fit i16 (TrueType coordinate range).
fn coord_i16(v: i64) -> Result<i16, ParseError> {
    i16::try_from(v).map_err(|_| ParseError::InvalidGlyphFormat)
}

/// Coordinate deltas must fit i16 (the classic encoding's delta range).
fn delta_i16(v: i64) -> Result<i16, ParseError> {
    i16::try_from(v).map_err(|_| ParseError::InvalidGlyphFormat)
}

/// Min/max over all points (the inferred bbox).
fn bbox_of(points: &[(i32, i32, bool)]) -> Result<(i16, i16, i16, i16), ParseError> {
    if points.is_empty() {
        return Ok((0, 0, 0, 0));
    }
    let mut x_min = i32::MAX;
    let mut y_min = i32::MAX;
    let mut x_max = i32::MIN;
    let mut y_max = i32::MIN;
    for &(x, y, _) in points {
        x_min = x_min.min(x);
        y_min = y_min.min(y);
        x_max = x_max.max(x);
        y_max = y_max.max(y);
    }
    Ok((
        coord_i16(i64::from(x_min))?,
        coord_i16(i64::from(y_min))?,
        coord_i16(i64::from(x_max))?,
        coord_i16(i64::from(y_max))?,
    ))
}

/// Reconstruct classic `hmtx` from a transformed table (spec §5.4): flags
/// byte, advance widths (u16), optional lsb/leftSideBearing arrays — the
/// missing ones rebuild from per-glyph xMin (0 for empty glyphs).
fn reconstruct_hmtx(
    transformed: &[u8],
    x_mins: &[i16],
    num_glyphs: u16,
    num_h_metrics: Option<u16>,
) -> Result<Vec<u8>, ParseError> {
    let Some(first) = transformed.first() else {
        return Err(ParseError::InvalidGlyphFormat);
    };
    let flags = *first;
    if flags & 0b1111_1100 != 0 {
        return Err(ParseError::InvalidGlyphFormat);
    }
    let lsb_missing = flags & 0x01 != 0;
    let bearing_missing = flags & 0x02 != 0;
    if !lsb_missing && !bearing_missing {
        // The transform exists only to drop arrays (spec: bit 0 or 1 MUST
        // be set).
        return Err(ParseError::InvalidGlyphFormat);
    }
    let num_h_metrics = match num_h_metrics {
        Some(h) if usize::from(h) <= usize::from(num_glyphs) => usize::from(h),
        _ => return Err(ParseError::InvalidGlyphFormat),
    };
    let n = usize::from(num_glyphs);
    let mut r = Reader::within(transformed, 1, transformed.len(), TAG);
    let mut advances = Vec::with_capacity(num_h_metrics);
    for _ in 0..num_h_metrics {
        advances.push(r.u16()?);
    }
    let mut lsbs: Vec<i16> = Vec::with_capacity(num_h_metrics);
    if !lsb_missing {
        for _ in 0..num_h_metrics {
            lsbs.push(r.i16()?);
        }
    } else {
        for g in 0..num_h_metrics {
            lsbs.push(*x_mins.get(g).ok_or(ParseError::InvalidGlyphFormat)?);
        }
    }
    let mut bearings = Vec::with_capacity(n - num_h_metrics);
    if !bearing_missing {
        for _ in 0..(n - num_h_metrics) {
            bearings.push(r.i16()?);
        }
    } else {
        for g in num_h_metrics..n {
            bearings.push(*x_mins.get(g).ok_or(ParseError::InvalidGlyphFormat)?);
        }
    }
    let mut out = Vec::with_capacity(4 * num_h_metrics + 2 * (n - num_h_metrics));
    for (adv, lsb) in advances.iter().zip(lsbs.iter()) {
        out.extend_from_slice(&adv.to_be_bytes());
        out.extend_from_slice(&lsb.to_be_bytes());
    }
    for b in &bearings {
        out.extend_from_slice(&b.to_be_bytes());
    }
    Ok(out)
}

fn write_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn write_i16(out: &mut Vec<u8>, v: i16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{decode, decode_triplet};

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean. Fixtures are generated by tests/fixtures/generate_fonts.py.

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/minimal.woff2");

    #[test]
    fn decodes_reference_fixture() {
        let font = decode(FIXTURE).unwrap();
        let f = font.as_font_ref().unwrap();
        assert_eq!(f.head().unwrap().units_per_em().unwrap(), 1000);
        assert_eq!(f.num_glyphs(), 4);
        let cmap = f.cmap().unwrap();
        assert_eq!(cmap.codepoint('A').map(crate::GlyphId::to_u32), Some(1));
        assert_eq!(cmap.codepoint('B').map(crate::GlyphId::to_u32), Some(2));
        // The composite glyph survives the transform.
        let outline = f.outline(crate::GlyphId::new(3)).unwrap().unwrap();
        assert!(!outline.is_empty());
        // hmtx reconstructed: advance for A.
        assert_eq!(f.advance_width(crate::GlyphId::new(1)).unwrap(), 600);
    }

    #[test]
    fn rejects_bad_containers() {
        assert!(decode(b"nope").is_err());
        assert!(decode(&FIXTURE[..40]).is_err());
        // Bad signature with sufficient length.
        let mut bad = FIXTURE.to_vec();
        bad[3] = b'X';
        assert!(decode(&bad).is_err());
    }

    #[test]
    fn triplet_encoding_vectors() {
        // Flag 0: x=0, y = -(8-bit + 0).
        assert_eq!(decode_triplet(0x80, &[10, 0, 0, 0]), (0, -10, false));
        // Flag 1: x=0, y = +(8-bit).
        assert_eq!(decode_triplet(0x81, &[10, 0, 0, 0]), (0, 10, false));
        // Flag 2: y = -(bits + 256).
        assert_eq!(decode_triplet(0x82, &[0, 0, 0, 0]), (0, -256, false));
        // Flag 3: y = +(bits + 256).
        assert_eq!(decode_triplet(0x83, &[1, 0, 0, 0]), (0, 257, false));
        // Flag 10: x negative 8-bit, y = 0.
        assert_eq!(decode_triplet(0x8A, &[10, 0, 0, 0]), (-10, 0, false));
        // Flag 12: x = -(bits + 256).
        assert_eq!(decode_triplet(0x8C, &[0, 0, 0, 0]), (-256, 0, false));
        // Flag 20: nibbles (2, 3), biases (1, 1), signs (-, -).
        assert_eq!(decode_triplet(0x94, &[0x23, 0, 0, 0]), (-3, -4, false));
        // Flag 21: signs (+, -).
        assert_eq!(decode_triplet(0x95, &[0x23, 0, 0, 0]), (3, -4, false));
        // Flag 36: dx = 17.
        assert_eq!(decode_triplet(0xA4, &[0x23, 0, 0, 0]), (-19, -4, false));
        // Flag 84: bytes (2, 3), biases (1, 1), signs (-, -).
        assert_eq!(decode_triplet(0xD4, &[2, 3, 0, 0]), (-3, -4, false));
        // Flag 120: 12-bit fields, no bias, signs (-, -).
        assert_eq!(
            decode_triplet(0xF8, &[0x12, 0x34, 0x00, 0]),
            (-0x123, -0x400, false)
        );
        // Flag 124: 16-bit fields, no bias, signs (-, -).
        assert_eq!(decode_triplet(0xFC, &[0, 7, 0, 9]), (-7, -9, false));
        // On-curve variants: flag bit 7 clear.
        assert_eq!(decode_triplet(0x7C, &[0, 7, 0, 9]), (-7, -9, true));
        assert_eq!(decode_triplet(0x7F, &[0, 7, 0, 9]), (7, 9, true));
        assert!(decode_triplet(0x20, &[0x23, 0, 0, 0]).2);
    }
}

#[cfg(test)]
mod internal_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{coord_bytes_for_flag, decode, glyf_streams, reconstruct_hmtx};

    /// Transformed glyf with a long index format and an overlap bitmap, for
    /// `glyf_streams` layout validation.
    fn transformed_glyf(index_format: u16, option_flags: u16, num_glyphs: u16) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&0u16.to_be_bytes()); // reserved
        d.extend_from_slice(&option_flags.to_be_bytes());
        d.extend_from_slice(&num_glyphs.to_be_bytes());
        d.extend_from_slice(&index_format.to_be_bytes());
        for size in [2u32, 2, 3, 4, 0, 12, 0] {
            d.extend_from_slice(&size.to_be_bytes());
        }
        // nContour: 1 glyph, 1 contour.
        d.extend_from_slice(&1i16.to_be_bytes());
        // nPoints: 3.
        d.extend_from_slice(&3u16.to_be_bytes());
        // flag stream: 3 flags.
        d.extend_from_slice(&[0x37, 0x37, 0x17]);
        // glyph stream: 3 triplets (1 byte each) + instruction length 0.
        d.extend_from_slice(&[100, 0, 100, 0]);
        // composite stream: empty.
        // bbox stream: bitmap 4 bytes + 4 i16 values.
        d.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        d.extend_from_slice(&0i16.to_be_bytes());
        d.extend_from_slice(&0i16.to_be_bytes());
        d.extend_from_slice(&300i16.to_be_bytes());
        d.extend_from_slice(&300i16.to_be_bytes());
        // instruction stream: empty.
        // overlap bitmap (optionFlags bit 0): 1 byte.
        if option_flags & 1 != 0 {
            d.extend_from_slice(&[0x80]);
        }
        d
    }

    #[test]
    fn streams_layout_and_lengths() {
        let t = transformed_glyf(0, 0, 1);
        let s = glyf_streams(&t).unwrap();
        assert_eq!(s.num_glyphs, 1);
        assert_eq!(s.index_format, 0);
        assert_eq!(s.bbox_bitmap.len(), 4);
        assert_eq!(s.n_contour.len(), 2);
        assert!(s.overlap_bitmap.is_none());
        // Long index format parses the same way.
        let t = transformed_glyf(1, 0, 1);
        assert_eq!(glyf_streams(&t).unwrap().index_format, 1);
        // Overlap bitmap consumes the trailing byte.
        let t = transformed_glyf(0, 1, 1);
        let s = glyf_streams(&t).unwrap();
        assert_eq!(s.overlap_bitmap.unwrap().len(), 1);
        // Bad index format is rejected.
        let t = transformed_glyf(7, 0, 1);
        assert!(glyf_streams(&t).is_err());
        // Truncated header.
        assert!(glyf_streams(&[0u8; 10]).is_err());
        // Substream beyond the table.
        let t = transformed_glyf(0, 0, 1);
        assert!(glyf_streams(&t[..50]).is_err());
        // bbox stream smaller than the bitmap.
        let mut t = transformed_glyf(0, 0, 40);
        let n = t.len();
        t.truncate(n - 20);
        assert!(glyf_streams(&t).is_err());
    }

    #[test]
    fn coord_byte_table() {
        assert_eq!(coord_bytes_for_flag(0), 1);
        assert_eq!(coord_bytes_for_flag(83), 1);
        assert_eq!(coord_bytes_for_flag(84), 2);
        assert_eq!(coord_bytes_for_flag(119), 2);
        assert_eq!(coord_bytes_for_flag(120), 3);
        assert_eq!(coord_bytes_for_flag(123), 3);
        assert_eq!(coord_bytes_for_flag(124), 4);
        assert_eq!(coord_bytes_for_flag(127), 4);
        assert_eq!(coord_bytes_for_flag(0xFF), 4);
    }

    /// Reconstructed hmtx from a transformed table with both arrays dropped
    /// (lsbs from glyph xMin values).
    #[test]
    fn hmtx_transform_both_arrays_dropped() {
        // flags 0x03: lsb and leftSideBearing both reconstructed.
        let transformed = [0x03u8, /* advances */ 0x01, 0xF4, 0x02, 0x58];
        let x_mins = [40i16, 30, 70];
        let out = reconstruct_hmtx(&transformed, &x_mins, 3, Some(2)).unwrap();
        // 2 long entries + 1 bare lsb.
        assert_eq!(out.len(), 2 * 4 + 2);
        assert_eq!(&out[..4], &[0x01, 0xF4, 0x00, 0x28]); // adv 500, lsb 40
        assert_eq!(&out[4..8], &[0x02, 0x58, 0x00, 0x1E]); // adv 600, lsb 30
                                                           // Bare lsb: glyph 2 xMin 70.
        assert_eq!(&out[8..10], &[0x00, 0x46]);
    }

    #[test]
    fn hmtx_transform_with_inline_lsb() {
        // flags 0x02: proportional lsb present inline; bearings dropped.
        let mut transformed = vec![0x02u8];
        transformed.extend_from_slice(&500u16.to_be_bytes());
        transformed.extend_from_slice(&(-10i16).to_be_bytes());
        transformed.extend_from_slice(&600u16.to_be_bytes());
        transformed.extend_from_slice(&20i16.to_be_bytes());
        let x_mins = [0i16, 0, 5];
        let out = reconstruct_hmtx(&transformed, &x_mins, 3, Some(2)).unwrap();
        assert_eq!(out.len(), 2 * 4 + 2);
        // Bearings reconstructed from xMin (glyph 2).
        assert_eq!(&out[8..10], &[0, 5]);
    }

    #[test]
    fn hmtx_transform_rejections() {
        // No bits set: a no-op transform is invalid.
        assert!(reconstruct_hmtx(&[0x00], &[], 1, Some(1)).is_err());
        // Reserved bits set.
        assert!(reconstruct_hmtx(&[0x04], &[], 1, Some(1)).is_err());
        // Empty table.
        assert!(reconstruct_hmtx(&[], &[], 1, Some(1)).is_err());
        // numberOfHMetrics beyond numGlyphs.
        assert!(reconstruct_hmtx(&[0x02, 0, 2], &[], 1, Some(1)).is_err());
        // xMin table too short for reconstruction.
        assert!(reconstruct_hmtx(&[0x02, 0, 2], &[], 3, Some(1)).is_err());
        // Missing hhea.
        assert!(reconstruct_hmtx(&[0x02, 0, 2], &[0], 1, None).is_err());
    }

    #[test]
    fn decode_rejections() {
        // Truncated header.
        assert!(decode(&[0u8; 47]).is_err());
        // Collection flavor.
        let mut d = vec![0u8; 64];
        d[0..4].copy_from_slice(b"wOF2");
        d[4..8].copy_from_slice(b"ttcf");
        assert!(decode(&d).is_err());
        // Zero tables.
        let mut d = vec![0u8; 64];
        d[0..4].copy_from_slice(b"wOF2");
        d[12..14].copy_from_slice(&0u16.to_be_bytes());
        assert!(decode(&d).is_err());
    }
}
