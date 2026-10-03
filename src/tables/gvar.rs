//! `gvar` — glyph variation data: per-glyph delta tuples for variable fonts.
//!
//! Packed encodings (per the OpenType spec and the reference `fontTools`
//! decompiler): point-number runs accumulate relative increments into
//! absolute indices; delta runs are control-byte framed
//! (`bits 6-7`: size — 1-byte, 2-byte, zeros, 4-byte; `bits 0-5`: run count
//! minus 1) and are **not** cumulative.

use crate::error::{ParseError, RawTag};
use crate::reader::{be_u16_at, be_u32_at, Reader};
use alloc::vec::Vec;
use font_types::F2Dot14;

pub(crate) const TAG: RawTag = *b"gvar";

const EMBEDDED_PEAK: u16 = 0x8000;
const INTERMEDIATE_REGION: u16 = 0x4000;
const PRIVATE_POINTS: u16 = 0x2000;
const SHARED_POINTS: u16 = 0x8000;
const LONG_OFFSETS: u16 = 0x0001;

const DELTAS_SIZE_MASK: u8 = 0xC0;
const DELTAS_ONE_BYTE: u8 = 0x00;
const DELTAS_ARE_WORDS: u8 = 0x40;
const DELTAS_ARE_ZERO: u8 = 0x80;
const DELTAS_ARE_LONGS: u8 = 0xC0;
const DELTA_RUN_COUNT_MASK: u8 = 0x3F;

const POINTS_ARE_WORDS: u8 = 0x80;
const POINT_RUN_COUNT_MASK: u8 = 0x7F;

/// One variation tuple: peak coordinates plus resolved x/y deltas.
///
/// Deltas apply to the outline points listed in `point_numbers`, or to
/// **all** points of the glyph when `point_numbers` is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TupleVariation {
    /// Peak coordinates per axis (embedded, shared-indexed, or empty).
    pub peak: Vec<F2Dot14>,
    /// Index into the shared-tuples array when the peak is not embedded.
    pub shared_peak_index: u16,
    /// Intermediate region start coordinates, when declared.
    pub start: Option<Vec<F2Dot14>>,
    /// Intermediate region end coordinates, when declared.
    pub end: Option<Vec<F2Dot14>>,
    /// Point indices the deltas apply to (`None` = all points).
    pub point_numbers: Option<Vec<u16>>,
    /// Per-point x deltas, run-decoded (not cumulative).
    pub x_deltas: Vec<i32>,
    /// Per-point y deltas.
    pub y_deltas: Vec<i32>,
}

impl TupleVariation {
    /// True when the tuple carries its own packed point numbers.
    #[must_use]
    pub const fn has_private_points(&self) -> bool {
        self.point_numbers.is_some()
    }
}

/// Zero-copy view over the `gvar` table.
#[derive(Debug, Clone, Copy)]
pub struct GvarTable<'a> {
    data: &'a [u8],
    axis_count: u16,
    glyph_count: u16,
    long_offsets: bool,
    glyph_data_array_offset: u32,
    offsets_array_at: usize,
    shared_tuples_offset: u32,
    shared_tuple_count: u16,
}

impl<'a> GvarTable<'a> {
    /// Parse and validate the `gvar` header and offsets array.
    ///
    /// `glyph_count` must come from `maxp`.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] / [`ParseError::InvalidGlyphFormat`]
    /// for short or inconsistent tables.
    pub fn parse(data: &'a [u8], glyph_count: u16) -> Result<Self, ParseError> {
        if data.len() < 20 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 20,
                got: data.len(),
            });
        }
        let axis_count = be_u16_at(data, 4, TAG)?;
        let shared_tuple_count = be_u16_at(data, 6, TAG)?;
        let shared_tuples_offset = be_u32_at(data, 8, TAG)?;
        let glyph_data_array_offset = be_u32_at(data, 16, TAG)?;
        let flags = be_u16_at(data, 14, TAG)?;
        let long_offsets = flags & LONG_OFFSETS != 0;
        if axis_count == 0 {
            return Err(ParseError::InvalidGlyphFormat);
        }
        let offsets_array_at = 20usize;
        let offsets_len = (usize::from(glyph_count) + 1)
            .checked_mul(if long_offsets { 4 } else { 2 })
            .ok_or(ParseError::InvalidGlyphFormat)?;
        let needed = offsets_array_at
            .checked_add(offsets_len)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if data.len() < needed {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: needed,
                got: data.len(),
            });
        }
        Ok(Self {
            data,
            axis_count,
            glyph_count,
            long_offsets,
            glyph_data_array_offset,
            offsets_array_at,
            shared_tuples_offset,
            shared_tuple_count,
        })
    }

    /// Raw offset entry `i` from the glyph-variation-data offsets array.
    /// Short offsets are stored divided by two (per the FreeType-derived
    /// convention fontTools documents); long offsets are raw.
    fn glyph_offset(&self, i: u16) -> Result<usize, ParseError> {
        if self.long_offsets {
            let v = be_u32_at(self.data, self.offsets_array_at + usize::from(i) * 4, TAG)?;
            usize::try_from(v).map_err(|_| ParseError::InvalidGlyphFormat)
        } else {
            let v = be_u16_at(self.data, self.offsets_array_at + usize::from(i) * 2, TAG)?;
            usize::from(v)
                .checked_mul(2)
                .ok_or(ParseError::InvalidGlyphFormat)
        }
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Axis count shared by every tuple.
    #[must_use]
    pub const fn axis_count(&self) -> u16 {
        self.axis_count
    }

    /// Shared tuple coordinates, in declared order.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the array is short.
    pub fn shared_tuples(&self) -> Result<Vec<Vec<F2Dot14>>, ParseError> {
        let mut out = Vec::with_capacity(usize::from(self.shared_tuple_count));
        let mut r = Reader::within(
            self.data,
            usize::try_from(self.shared_tuples_offset)
                .map_err(|_| ParseError::InvalidGlyphFormat)?,
            self.data.len(),
            TAG,
        );
        for _ in 0..self.shared_tuple_count {
            let mut tuple = Vec::with_capacity(usize::from(self.axis_count));
            for _ in 0..self.axis_count {
                tuple.push(F2Dot14::from_bits(r.i16()?));
            }
            out.push(tuple);
        }
        Ok(out)
    }

    /// Byte range of glyph `gid`'s variation data (`None` = no variation
    /// data / id out of range).
    fn glyph_data_range(&self, gid: u16) -> Result<Option<core::ops::Range<usize>>, ParseError> {
        if gid >= self.glyph_count {
            return Ok(None);
        }
        let base = usize::try_from(self.glyph_data_array_offset)
            .map_err(|_| ParseError::InvalidGlyphFormat)?;
        let next = gid.checked_add(1).ok_or(ParseError::InvalidGlyphFormat)?;
        let start = base
            .checked_add(self.glyph_offset(gid)?)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        let end = base
            .checked_add(self.glyph_offset(next)?)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end >= start && end <= self.data.len() {
            if start == end {
                Ok(None)
            } else {
                Ok(Some(start..end))
            }
        } else {
            Err(ParseError::InvalidGlyphFormat)
        }
    }

    /// All variation tuples for `gid`.
    ///
    /// `point_count` is the glyph's variation point count — outline points
    /// (components for composites) **plus the 4 phantom points** — which
    /// sizes "apply to all points" tuples whose delta count the table does
    /// not store.
    ///
    /// # Errors
    /// [`ParseError::InvalidGlyphFormat`] / [`ParseError::TruncatedTable`]
    /// for malformed tuple data.
    pub fn glyph_deltas(
        &self,
        gid: u16,
        point_count: u16,
    ) -> Result<Vec<TupleVariation>, ParseError> {
        let mut tuples = Vec::new();
        let Some(range) = self.glyph_data_range(gid)? else {
            return Ok(tuples);
        };
        let data = crate::reader::slice_at(self.data, range.start, range.end - range.start, TAG)?;
        if data.len() < 4 {
            return Err(ParseError::InvalidGlyphFormat);
        }
        let tuple_var_count = be_u16_at(data, 0, TAG)?;
        let count = usize::from(tuple_var_count & 0x0FFF);
        let has_shared_points = tuple_var_count & SHARED_POINTS != 0;
        let data_offset = usize::from(be_u16_at(data, 2, TAG)?);
        if data_offset > data.len() {
            return Err(ParseError::InvalidGlyphFormat);
        }
        // Headers live in [4, data_offset); serialized tuples follow.
        let mut hr = Reader::within(data, 4, data_offset, TAG);
        let mut sr_pos = data_offset;
        // Shared packed points precede the tuple data when flagged.
        let mut shared_points: Option<Vec<u16>> = None;
        if has_shared_points {
            let mut pr = Reader::within(data, sr_pos, data.len(), TAG);
            let pts = read_packed_points(&mut pr, usize::from(point_count))?;
            sr_pos = sr_pos.saturating_add(pr.pos());
            shared_points = Some(pts);
        }
        let shared_tuples = self.shared_tuples()?;
        for _ in 0..count {
            // Header: variationDataSize + tupleIndex [+ peak [+ region]].
            let variation_data_size = usize::from(hr.u16()?);
            let tuple_index = hr.u16()?;
            let axis = usize::from(self.axis_count);
            let (peak, shared_peak_index) = if tuple_index & EMBEDDED_PEAK != 0 {
                let mut peak = Vec::with_capacity(axis);
                for _ in 0..axis {
                    peak.push(F2Dot14::from_bits(hr.i16()?));
                }
                (peak, u16::MAX)
            } else {
                let idx = tuple_index & 0x0FFF;
                let peak = shared_tuples
                    .get(usize::from(idx))
                    .cloned()
                    .unwrap_or_default();
                (peak, idx)
            };
            let (start, end) = if tuple_index & INTERMEDIATE_REGION != 0 {
                let mut s = Vec::with_capacity(axis);
                for _ in 0..axis {
                    s.push(F2Dot14::from_bits(hr.i16()?));
                }
                let mut e = Vec::with_capacity(axis);
                for _ in 0..axis {
                    e.push(F2Dot14::from_bits(hr.i16()?));
                }
                (Some(s), Some(e))
            } else {
                (None, None)
            };
            // Serialized window for this tuple.
            let window = crate::reader::slice_at(data, sr_pos, variation_data_size, TAG)?;
            sr_pos += variation_data_size;
            let mut tr = Reader::new(window, TAG);
            // Private point numbers, else shared, else all points.
            let point_numbers = if tuple_index & PRIVATE_POINTS != 0 {
                Some(read_packed_points(&mut tr, usize::from(point_count))?)
            } else {
                shared_points.clone()
            };
            let n = point_numbers
                .as_ref()
                .map_or(usize::from(point_count), Vec::len);
            let x = read_packed_deltas(&mut tr, n)?;
            let y = read_packed_deltas(&mut tr, n)?;
            tuples.push(TupleVariation {
                peak,
                shared_peak_index,
                start,
                end,
                point_numbers,
                x_deltas: x,
                y_deltas: y,
            });
        }
        Ok(tuples)
    }
}

/// Decode gvar packed point numbers: relative increments accumulated into
/// absolute indices. An initial zero byte means "all points".
fn read_packed_points(r: &mut Reader<'_>, outline_points: usize) -> Result<Vec<u16>, ParseError> {
    let first = r.u8()?;
    if first == 0 {
        let n = outline_points.min(usize::from(u16::MAX));
        return (0..n)
            .map(u16::try_from)
            .collect::<Result<Vec<u16>, _>>()
            .map_err(|_| ParseError::InvalidGlyphFormat);
    }
    let count = if first & POINTS_ARE_WORDS != 0 {
        let hi = usize::from(first & POINT_RUN_COUNT_MASK);
        (hi << 8 | usize::from(r.u8()?)) + 1
    } else {
        usize::from(first) + 1
    };
    if count > outline_points {
        return Err(ParseError::InvalidGlyphFormat);
    }
    let mut pts = Vec::with_capacity(count);
    let mut cur: u32 = 0;
    while pts.len() < count {
        let control = r.u8()?;
        let run_len = usize::from(control & POINT_RUN_COUNT_MASK) + 1;
        for _ in 0..run_len {
            if pts.len() >= count {
                return Err(ParseError::InvalidGlyphFormat);
            }
            let inc = if control & POINTS_ARE_WORDS != 0 {
                u32::from(r.u16()?)
            } else {
                u32::from(r.u8()?)
            };
            cur = cur.saturating_add(inc);
            let idx = u16::try_from(cur).map_err(|_| ParseError::InvalidGlyphFormat)?;
            pts.push(idx);
        }
    }
    Ok(pts)
}

/// Decode gvar packed deltas: control-byte framed runs; values are used
/// directly (never cumulative).
fn read_packed_deltas(r: &mut Reader<'_>, count: usize) -> Result<Vec<i32>, ParseError> {
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let control = r.u8()?;
        let run_len = usize::from(control & DELTA_RUN_COUNT_MASK) + 1;
        for _ in 0..run_len {
            if out.len() >= count {
                return Err(ParseError::InvalidGlyphFormat);
            }
            match control & DELTAS_SIZE_MASK {
                DELTAS_ONE_BYTE => out.push(i32::from(r.i8()?)),
                DELTAS_ARE_WORDS => out.push(i32::from(r.i16()?)),
                DELTAS_ARE_ZERO => out.push(0),
                DELTAS_ARE_LONGS => out.push(r.i32()?),
                _ => return Err(ParseError::InvalidGlyphFormat),
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{GvarTable, TupleVariation};
    use crate::error::ParseError;
    use alloc::vec;
    use alloc::vec::Vec;
    use font_types::F2Dot14;

    /// Hand-built gvar for 3 glyphs, 1 axis, 1 shared tuple (peak 1.0):
    /// gid 0: one tuple, shared points (all 6 points), mixed delta runs.
    /// gid 1: no data.
    /// gid 2: private points [0, 3] (of 6), embedded peak 0.5, intermediate
    /// region, x deltas [10, 20], zero y deltas.
    fn gvar() -> Vec<u8> {
        // Serialized data for gid 0: shared "all points" byte, x runs
        // ([50, 50] then zeros to 6), y runs ([5, 5] then zeros).
        let ser0: [u8; 9] = [
            0x00, // shared points: all points
            0x01, 50, 50,   // x: 1-byte deltas, count 2
            0x83, // x: zeros, count 4
            0x01, 5, 5,    // y: 1-byte deltas, count 2
            0x83, // y: zeros, count 4
        ];
        let gid0: [u8; 17] = [
            0x80, 0x01, // tupleVariationCount: shared points + 1 tuple
            0x00, 0x08, // dataOffset 8
            0x00, 0x09, // dataSize 9
            0x00, 0x00, // tupleIndex 0 -> shared tuple 0
            ser0[0], ser0[1], ser0[2], ser0[3], ser0[4], ser0[5], ser0[6], ser0[7], ser0[8],
        ];
        // Serialized data for gid 2: private points [0, 3], x [10, 20],
        // y zeros (2).
        let ser2: [u8; 9] = [
            0x01, // point count: (0x01 & 0x7F) + 1 = 2
            0x00, 0x00, // run of 1 one-byte increment: +0 -> point 0
            0x00, 0x03, // run of 1 one-byte increment: +3 -> point 3
            0x01, 10, 20,   // x: 1-byte deltas, count 2
            0x81, // y: zeros, count 2
        ];
        let mut gid2: Vec<u8> = Vec::new();
        gid2.extend_from_slice(&0x0001u16.to_be_bytes()); // 1 tuple, no shared points
                                                          // dataOffset: headers run [4, 14): dataSize, tupleIndex, peak,
                                                          // start, end (embedded peak + intermediate region extend the header).
        gid2.extend_from_slice(&14u16.to_be_bytes());
        gid2.extend_from_slice(&(ser2.len() as u16).to_be_bytes()); // dataSize
        gid2.extend_from_slice(&0xE000u16.to_be_bytes()); // embedded peak + intermediate + private points
        gid2.extend_from_slice(&0x2000i16.to_be_bytes()); // peak 0.5
        gid2.extend_from_slice(&0x1000i16.to_be_bytes()); // intermediate start 0.25
        gid2.extend_from_slice(&0x3000i16.to_be_bytes()); // intermediate end 0.75
        gid2.extend_from_slice(&ser2);
        debug_assert_eq!(gid2.len(), 14 + ser2.len());
        if gid2.len() % 2 != 0 {
            gid2.push(0);
        }
        // Layout: header 20, short offsets 8 (20..28), shared tuple @28,
        // glyph variation data array @32. Offsets are relative to the array.
        let mut d = Vec::new();
        d.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
        d.extend_from_slice(&1u16.to_be_bytes()); // axisCount
        d.extend_from_slice(&1u16.to_be_bytes()); // sharedTupleCount
        d.extend_from_slice(&28u32.to_be_bytes()); // sharedTuplesOffset
        d.extend_from_slice(&3u16.to_be_bytes()); // glyphCount
        d.extend_from_slice(&0u16.to_be_bytes()); // flags: short offsets
        d.extend_from_slice(&32u32.to_be_bytes()); // data array offset
                                                   // Offsets (raw, stored halved): [0, 20, 20, 20 + gid2.len()]
        let end = (17 + 1 + gid2.len()) as u16; // gid0 padded to even
        for v in [0u16, 9, 9, end / 2] {
            d.extend_from_slice(&v.to_be_bytes());
        }
        // Shared tuple @28: peak 1.0 (0x4000) + pad to the data array @32.
        d.extend_from_slice(&0x4000i16.to_be_bytes());
        d.extend_from_slice(&[0u8; 2]);
        d.extend_from_slice(&gid0);
        if d.len() % 2 != 0 {
            d.push(0); // gid0's even pad (accounted in the offsets)
        }
        d.extend_from_slice(&gid2);
        if d.len() % 2 != 0 {
            d.push(0);
        }
        d
    }

    #[test]
    fn header_offsets_and_shared_tuples() {
        let d = gvar();
        let t = GvarTable::parse(&d, 3).unwrap();
        assert_eq!(t.axis_count(), 1);
        let shared = t.shared_tuples().unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0][0].to_bits(), 0x4000);
    }

    #[test]
    fn shared_points_tuple() {
        let d = gvar();
        let t = GvarTable::parse(&d, 3).unwrap();
        let tuples: Vec<TupleVariation> = t.glyph_deltas(0, 6).unwrap();
        assert_eq!(tuples.len(), 1);
        let t0 = &tuples[0];
        // Shared point numbers materialize as a concrete list.
        assert_eq!(
            t0.point_numbers.as_deref(),
            Some(&[0u16, 1, 2, 3, 4, 5][..])
        );
        assert_eq!(t0.x_deltas, vec![50, 50, 0, 0, 0, 0]);
        assert_eq!(t0.y_deltas, vec![5, 5, 0, 0, 0, 0]);
        // Peak from the shared tuple.
        assert_eq!(t0.shared_peak_index, 0);
        assert_eq!(t0.peak, vec![F2Dot14::from_bits(0x4000)]);
        assert!(t0.start.is_none());
        // gid 1 has no data.
        assert!(t.glyph_deltas(1, 6).unwrap().is_empty());
    }

    #[test]
    fn private_points_embedded_peak_intermediate() {
        let d = gvar();
        let t = GvarTable::parse(&d, 3).unwrap();
        let tuples = t.glyph_deltas(2, 6).unwrap();
        assert_eq!(tuples.len(), 1);
        let t2 = &tuples[0];
        assert!(t2.has_private_points());
        assert_eq!(t2.point_numbers, Some(vec![0, 3]));
        assert_eq!(t2.x_deltas, vec![10, 20]);
        assert_eq!(t2.y_deltas, vec![0, 0]);
        assert_eq!(t2.shared_peak_index, u16::MAX);
        assert_eq!(t2.peak, vec![F2Dot14::from_bits(0x2000)]);
        assert_eq!(t2.start.as_ref().map(|s| s[0].to_bits()), Some(0x1000));
        assert_eq!(t2.end.as_ref().map(|s| s[0].to_bits()), Some(0x3000));
    }

    #[test]
    fn errors() {
        // Truncated header.
        assert!(matches!(
            GvarTable::parse(&[0u8; 19], 3),
            Err(ParseError::TruncatedTable { .. })
        ));
        // Zero axis count.
        let mut d = gvar();
        d[4..6].copy_from_slice(&0u16.to_be_bytes());
        assert_eq!(
            GvarTable::parse(&d, 3).unwrap_err(),
            ParseError::InvalidGlyphFormat
        );
        // Offsets array short for the declared glyph count.
        let d = gvar();
        assert!(matches!(
            GvarTable::parse(&d, 200),
            Err(ParseError::TruncatedTable { .. })
        ));
        // Out-of-range gid -> no data.
        let d = gvar();
        let t = GvarTable::parse(&d, 3).unwrap();
        assert!(t.glyph_deltas(9, 6).unwrap().is_empty());
        // gid 1 genuinely has no data (empty offset range).
        // Glyph range beyond the table -> invalid.
        let mut d = gvar();
        let n = d.len();
        d.truncate(n - 5);
        let t = GvarTable::parse(&d, 3).unwrap();
        assert_eq!(
            t.glyph_deltas(2, 6).unwrap_err(),
            ParseError::InvalidGlyphFormat
        );
    }
}
