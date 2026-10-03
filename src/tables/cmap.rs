//! `cmap` — character-to-glyph mapping (formats 0, 4, 6, 12).

use crate::error::{ParseError, RawTag};
use alloc::vec::Vec;
use font_types::{BigEndian, GlyphId};

pub(crate) const TAG: RawTag = *b"cmap";

/// Subtable header size.
const SUB_HEADER: usize = 4;
/// Encoding record size.
const RECORD: usize = 8;

/// A `cmap` encoding record: platform/encoding id plus subtable offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingRecord {
    /// Platform id (0 Unicode, 1 Macintosh, 3 Microsoft, …).
    pub platform_id: u16,
    /// Encoding id (platform-specific; e.g. 1 = BMP, 10 = UCS-4).
    pub encoding_id: u16,
    /// Offset of the subtable from the `cmap` table start.
    pub offset: u32,
}

impl EncodingRecord {
    /// True for encodings that carry a Unicode character mapping.
    #[must_use]
    pub const fn is_unicode(self) -> bool {
        self.platform_id == 0
            || (self.platform_id == 3 && (self.encoding_id == 1 || self.encoding_id == 10))
    }
}

/// Zero-copy view over the `cmap` table.
///
/// The header and encoding records are validated eagerly; subtables are
/// decoded lazily. Lookups via [`codepoint`](Self::codepoint) are total:
/// unsupported or malformed subtables are skipped in favor of the next
/// candidate record (typed access is available through
/// [`subtable`](Self::subtable)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cmap<'a> {
    data: &'a [u8],
    records: &'a [u8],
}

impl<'a> Cmap<'a> {
    /// Parse and validate the `cmap` header + encoding records.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when header or records are truncated;
    /// [`ParseError::InvalidGlyphFormat`] when the record count is
    /// inconsistent with the table length.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < SUB_HEADER {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: SUB_HEADER,
                got: data.len(),
            });
        }
        let num_tables = crate::reader::be_u16_at(data, 2, TAG)?;
        let end = 4 + usize::from(num_tables)
            .checked_mul(RECORD)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: end,
                got: data.len(),
            });
        }
        // Every record's offset must at least point inside the table.
        for i in 0..usize::from(num_tables) {
            let at = 4 + i * RECORD;
            let offset = crate::reader::be_u32_at(data, at + 4, TAG)?;
            let inside = usize::try_from(offset)
                .map(|o| o.checked_add(SUB_HEADER).is_some_and(|e| e <= data.len()))
                .unwrap_or(false);
            if !inside {
                return Err(ParseError::InvalidGlyphFormat);
            }
        }
        Ok(Self {
            data,
            records: crate::reader::slice_at(data, 4, end - 4, TAG)?,
        })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// The encoding records, in table order.
    pub fn records(&self) -> Vec<EncodingRecord> {
        let mut out = Vec::new();
        for chunk in self.records.chunks_exact(RECORD) {
            let platform_id = chunk
                .get(0..2)
                .and_then(BigEndian::<u16>::from_slice)
                .map_or(0, |b| b.get());
            let encoding_id = chunk
                .get(2..4)
                .and_then(BigEndian::<u16>::from_slice)
                .map_or(0, |b| b.get());
            let offset = chunk
                .get(4..8)
                .and_then(BigEndian::<u32>::from_slice)
                .map_or(0, |b| b.get());
            out.push(EncodingRecord {
                platform_id,
                encoding_id,
                offset,
            });
        }
        out
    }

    /// Decode the subtable referenced by record `index`.
    ///
    /// # Errors
    /// [`ParseError::InvalidCmapFormat`] for formats this parser does not
    /// implement (2, 8, 10, 13, 14) or invalid format numbers;
    /// [`ParseError::TruncatedTable`] / [`ParseError::InvalidGlyphFormat`]
    /// for malformed subtables.
    pub fn subtable(&self, index: usize) -> Result<CmapSubtable<'a>, ParseError> {
        let record = *self
            .records()
            .get(index)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        let at = usize::try_from(record.offset).map_err(|_| ParseError::InvalidGlyphFormat)?;
        let data = self.data.get(at..).ok_or(ParseError::InvalidGlyphFormat)?;
        CmapSubtable::parse(data)
    }

    /// Map a Unicode scalar to a glyph id.
    ///
    /// Candidates are scanned in spec-preference order (UCS-4 subtables
    /// before BMP before platform-0 catch-alls); the first subtable that
    /// both decodes and maps the codepoint wins. Returns `None` when no
    /// Unicode subtable maps the character. Never panics: malformed or
    /// unsupported subtables are skipped.
    #[must_use]
    pub fn codepoint(&self, c: char) -> Option<GlyphId> {
        self.codepoint_u32(u32::from(c))
    }

    /// [`codepoint`](Self::codepoint) for a raw codepoint value.
    #[must_use]
    pub fn codepoint_u32(&self, cp: u32) -> Option<GlyphId> {
        // Candidates in spec-preference order (UCS-4 subtables before BMP
        // before platform-0 catch-alls); stable sort keeps table order for
        // ties. Malformed or unsupported subtables are skipped.
        let mut candidates: Vec<(u8, usize)> = self
            .records()
            .iter()
            .enumerate()
            .filter(|(_, r)| r.is_unicode())
            .map(|(i, r)| (preference(r.platform_id, r.encoding_id), i))
            .collect();
        candidates.sort_by_key(|c| core::cmp::Reverse(c.0));
        for (_, i) in candidates {
            if let Ok(sub) = self.subtable(i) {
                if let Some(gid) = sub.lookup(cp) {
                    return Some(GlyphId::new(u32::from(gid)));
                }
            }
        }
        None
    }
}

/// Lookup preference score for a Unicode-capable encoding record; higher
/// wins. (3, 10) UCS-4 outranks platform-0 outranks (3, 1) BMP.
fn preference(platform: u16, encoding: u16) -> u8 {
    match (platform, encoding) {
        (3, 10) => 6,
        (0, _) => 5,
        (3, 1) => 4,
        _ => 0,
    }
}

/// A decoded `cmap` subtable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmapSubtable<'a> {
    /// Byte encoding table (format 0).
    Format0(Format0<'a>),
    /// Segment mapping to delta values (format 4).
    Format4(Format4<'a>),
    /// Trimmed table mapping (format 6).
    Format6(Format6<'a>),
    /// Segmented coverage (format 12).
    Format12(Format12<'a>),
}

impl<'a> CmapSubtable<'a> {
    /// Decode a subtable by its format field.
    ///
    /// # Errors
    /// [`ParseError::InvalidCmapFormat`] for unsupported/invalid formats;
    /// format-specific parse errors otherwise.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 2 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 2,
                got: data.len(),
            });
        }
        match crate::reader::be_u16_at(data, 0, TAG)? {
            0 => Ok(Self::Format0(Format0::parse(data)?)),
            4 => Ok(Self::Format4(Format4::parse(data)?)),
            6 => Ok(Self::Format6(Format6::parse(data)?)),
            12 => Ok(Self::Format12(Format12::parse(data)?)),
            other => Err(ParseError::InvalidCmapFormat(other)),
        }
    }

    /// Map a codepoint to a raw glyph id (`None` = not mapped).
    #[must_use]
    pub fn lookup(&self, cp: u32) -> Option<u16> {
        match self {
            Self::Format0(t) => t.lookup(cp),
            Self::Format4(t) => t.lookup(cp),
            Self::Format6(t) => t.lookup(cp),
            Self::Format12(t) => t.lookup(cp),
        }
    }
}

/// Format 0 — byte encoding table: 256 direct glyph ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format0<'a> {
    glyph_array: &'a [u8],
}

impl<'a> Format0<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        // 6-byte header + 256 bytes.
        if data.len() < 6 + 256 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 6 + 256,
                got: data.len(),
            });
        }
        Ok(Self {
            glyph_array: crate::reader::slice_at(data, 6, 256, TAG)?,
        })
    }

    fn lookup(&self, cp: u32) -> Option<u16> {
        if cp < 256 {
            self.glyph_array.get(cp as usize).map(|&g| u16::from(g))
        } else {
            None
        }
    }
}

/// Format 4 — segment mapping to delta values (BMP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format4<'a> {
    data: &'a [u8],
    seg_count: usize,
    /// Offset of the `endCode` array.
    ends: usize,
    /// Offset of the `startCode` array.
    starts: usize,
    /// Offset of the `idDelta` array.
    deltas: usize,
    /// Offset of the `idRangeOffset` array.
    ranges: usize,
}

impl<'a> Format4<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 14 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 14,
                got: data.len(),
            });
        }
        let seg_count_x2 = crate::reader::be_u16_at(data, 6, TAG)?;
        if seg_count_x2 == 0 || seg_count_x2 % 2 != 0 {
            return Err(ParseError::InvalidGlyphFormat);
        }
        let seg_count = usize::from(seg_count_x2 / 2);
        let ends = 14;
        let starts = ends + seg_count * 2 + 2; // reservedPad u16 in between
        let deltas = starts + seg_count * 2;
        let ranges = deltas + seg_count * 2;
        // The glyphIndexArray may be empty; require the four arrays to fit.
        let needed = ranges + seg_count * 2;
        if data.len() < needed {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: needed,
                got: data.len(),
            });
        }
        Ok(Self {
            data,
            seg_count,
            ends,
            starts,
            deltas,
            ranges,
        })
    }

    fn lookup(&self, cp: u32) -> Option<u16> {
        if cp > 0xFFFF {
            return None;
        }
        let cp16 = cp as u16;
        // endCodes are sorted ascending; find the first segment whose end
        // code is >= cp, then confirm cp >= startCode.
        let seg = self.segment_for(cp16)?;
        let start =
            crate::reader::be_u16_at(self.data, self.starts + seg * 2, TAG).unwrap_or(u16::MAX);
        if cp16 < start {
            return None;
        }
        let range_offset =
            crate::reader::be_u16_at(self.data, self.ranges + seg * 2, TAG).unwrap_or(0);
        let glyph = if range_offset == 0 {
            let delta =
                crate::reader::be_u16_at(self.data, self.deltas + seg * 2, TAG).unwrap_or(0);
            cp16.wrapping_add(delta)
        } else {
            // glyphIndexArray address, relative to the idRangeOffset[seg]
            // slot, in u16 units (spec: idRangeOffset/2 + cp - start + seg -
            // segCount).
            let rel = (usize::from(range_offset) / 2)
                .checked_add(cp16.wrapping_sub(start) as usize)?
                .checked_add(seg)?
                .checked_sub(self.seg_count)?;
            let slot = self.ranges + rel * 2;
            crate::reader::be_u16_at(self.data, slot, TAG).unwrap_or(0)
        };
        (glyph != 0).then_some(glyph)
    }

    fn segment_for(&self, cp: u16) -> Option<usize> {
        // Binary search: last segment with endCode >= cp.
        let mut lo = 0;
        let mut hi = self.seg_count;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let end = crate::reader::be_u16_at(self.data, self.ends + mid * 2, TAG).ok()?;
            if end < cp {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        (lo < self.seg_count).then_some(lo)
    }
}

/// Format 6 — trimmed table mapping: a contiguous firstCode window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format6<'a> {
    first_code: u16,
    glyph_array: &'a [u8],
}

impl<'a> Format6<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 10 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 10,
                got: data.len(),
            });
        }
        let first_code = crate::reader::be_u16_at(data, 6, TAG)?;
        let entry_count = crate::reader::be_u16_at(data, 8, TAG)?;
        let needed = 10 + usize::from(entry_count) * 2;
        if data.len() < needed {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: needed,
                got: data.len(),
            });
        }
        Ok(Self {
            first_code,
            glyph_array: crate::reader::slice_at(data, 10, usize::from(entry_count) * 2, TAG)?,
        })
    }

    fn lookup(&self, cp: u32) -> Option<u16> {
        let cp = u16::try_from(cp).ok()?;
        let idx = cp.checked_sub(self.first_code)?;
        let idx = usize::from(idx);
        if idx < self.glyph_array.len() / 2 {
            crate::reader::be_u16_at(self.glyph_array, idx * 2, TAG).ok()
        } else {
            None
        }
    }
}

/// Format 12 — segmented coverage: sorted (start, end, startGlyphId) groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format12<'a> {
    groups: &'a [u8],
}

impl<'a> Format12<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 16 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 16,
                got: data.len(),
            });
        }
        let n_groups = crate::reader::be_u32_at(data, 12, TAG)?;
        let n = usize::try_from(n_groups).map_err(|_| ParseError::InvalidGlyphFormat)?;
        // Overflow guard first, then slice_at validates the group array
        // against the buffer.
        n.checked_mul(12)
            .and_then(|v| v.checked_add(16))
            .ok_or(ParseError::InvalidGlyphFormat)?;
        Ok(Self {
            groups: crate::reader::slice_at(data, 16, n * 12, TAG)?,
        })
    }

    fn lookup(&self, cp: u32) -> Option<u16> {
        // Binary search over sorted group starts: last group with
        // startCharCode <= cp, then check the end.
        let n = self.groups.len() / 12;
        let mut lo = 0;
        let mut hi = n;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let start = crate::reader::be_u32_at(self.groups, mid * 12, TAG).ok()?;
            if start <= cp {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let g = lo - 1;
        let start = crate::reader::be_u32_at(self.groups, g * 12, TAG).ok()?;
        let end = crate::reader::be_u32_at(self.groups, g * 12 + 4, TAG).ok()?;
        if cp < start || cp > end {
            return None;
        }
        let start_gid = crate::reader::be_u32_at(self.groups, g * 12 + 8, TAG).ok()?;
        let gid = start_gid.checked_add(cp - start)?;
        u16::try_from(gid).ok()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::{Cmap, CmapSubtable};
    use crate::error::ParseError;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    use alloc::vec;
    use alloc::vec::Vec;

    /// A minimal format 4 subtable with three segments:
    /// U+0041..U+0041 -> +2 (gid 67), U+0061..U+007A -> direct array,
    /// U+FFFF terminator (notdef). Array layout per spec.
    fn format4() -> Vec<u8> {
        let seg_count = 3u16;
        let mut d = Vec::new();
        d.extend_from_slice(&4u16.to_be_bytes()); // format
        d.extend_from_slice(&32u16.to_be_bytes()); // length: 14 + 3*2*4 + 2
        d.extend_from_slice(&0u16.to_be_bytes()); // language
        d.extend_from_slice(&(seg_count * 2).to_be_bytes()); // segCountX2
        d.extend_from_slice(&4u16.to_be_bytes()); // searchRange
        d.extend_from_slice(&1u16.to_be_bytes()); // entrySelector
        d.extend_from_slice(&2u16.to_be_bytes()); // rangeShift
                                                  // endCode[]
        d.extend_from_slice(&0x0041u16.to_be_bytes());
        d.extend_from_slice(&0x007Au16.to_be_bytes());
        d.extend_from_slice(&0xFFFFu16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
                                                  // startCode[]
        d.extend_from_slice(&0x0041u16.to_be_bytes());
        d.extend_from_slice(&0x0061u16.to_be_bytes());
        d.extend_from_slice(&0xFFFFu16.to_be_bytes());
        // idDelta[]
        d.extend_from_slice(&2u16.to_be_bytes()); // 0x41 + 2 = 0x43
        d.extend_from_slice(&0u16.to_be_bytes()); // via array
        d.extend_from_slice(&1u16.to_be_bytes()); // 0xFFFF + 1 = 0 -> notdef
                                                  // idRangeOffset[]
        d.extend_from_slice(&0u16.to_be_bytes());
        // Segment 1 (a..z) points into glyphIndexArray: rel = ro/2 + (cp -
        // start) + seg - segCount must land at array[cp - 0x61]; with the
        // array starting 6 bytes past the ranges slot for seg 1, ro = 10.
        d.extend_from_slice(&10u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        // glyphIndexArray for segment 1: a..z -> gid 1 + (c - a)
        d.extend_from_slice(&[0, 1, 0, 2, 0, 3]);
        d
    }

    #[test]
    fn format4_delta_and_array() {
        let d = format4();
        let sub = CmapSubtable::parse(&d).unwrap();
        assert!(matches!(sub, CmapSubtable::Format4(_)));
        assert_eq!(sub.lookup(0x41), Some(0x43)); // delta path
        assert_eq!(sub.lookup(0x61), Some(1)); // array path
        assert_eq!(sub.lookup(0x63), Some(3));
        assert_eq!(sub.lookup(0x40), None); // before first segment
        assert_eq!(sub.lookup(0x5A), None); // between segments (Z < a)
        assert_eq!(sub.lookup(0x1_0000), None); // above BMP
    }

    fn format0() -> Vec<u8> {
        let mut d = vec![0u8; 262];
        d[0..2].copy_from_slice(&0u16.to_be_bytes());
        d[6 + 65] = 7; // 'A' -> gid 7
        d[6 + 0x7F] = 9;
        d
    }

    #[test]
    fn format0_lookup() {
        let d = format0();
        let sub = CmapSubtable::parse(&d).unwrap();
        assert_eq!(sub.lookup(65), Some(7));
        assert_eq!(sub.lookup(0x7F), Some(9));
        assert_eq!(sub.lookup(66), Some(0));
        assert_eq!(sub.lookup(256), None);
        assert_eq!(sub.lookup(u32::MAX), None);
    }

    fn format6() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&6u16.to_be_bytes());
        d.extend_from_slice(&16u16.to_be_bytes()); // length
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&0x0300u16.to_be_bytes()); // firstCode
        d.extend_from_slice(&3u16.to_be_bytes()); // entryCount
        d.extend_from_slice(&10u16.to_be_bytes());
        d.extend_from_slice(&11u16.to_be_bytes());
        d.extend_from_slice(&12u16.to_be_bytes());
        d
    }

    #[test]
    fn format6_lookup() {
        let d = format6();
        let sub = CmapSubtable::parse(&d).unwrap();
        assert_eq!(sub.lookup(0x300), Some(10));
        assert_eq!(sub.lookup(0x302), Some(12));
        assert_eq!(sub.lookup(0x2FF), None);
        assert_eq!(sub.lookup(0x303), None);
        assert_eq!(sub.lookup(0x10300), None);
    }

    fn format12() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&12u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes()); // reserved
        d.extend_from_slice(&28u32.to_be_bytes()); // length
        d.extend_from_slice(&0u32.to_be_bytes()); // language
        d.extend_from_slice(&2u32.to_be_bytes()); // nGroups
        d.extend_from_slice(&0x1_0000u32.to_be_bytes()); // start
        d.extend_from_slice(&0x1_00FFu32.to_be_bytes()); // end
        d.extend_from_slice(&100u32.to_be_bytes()); // startGlyphId
        d.extend_from_slice(&0x2_0000u32.to_be_bytes());
        d.extend_from_slice(&0x2_0000u32.to_be_bytes());
        d.extend_from_slice(&200u32.to_be_bytes());
        d
    }

    #[test]
    fn format12_groups() {
        let d = format12();
        let sub = CmapSubtable::parse(&d).unwrap();
        assert_eq!(sub.lookup(0x1_0000), Some(100));
        assert_eq!(sub.lookup(0x1_00FF), Some(100 + 0xFF));
        assert_eq!(sub.lookup(0x2_0000), Some(200));
        assert_eq!(sub.lookup(0xFFFF), None);
        assert_eq!(sub.lookup(0x1_0100), None);
        assert_eq!(sub.lookup(0x3_0000), None);
    }

    #[test]
    fn unsupported_format() {
        let mut d = vec![0u8; 32];
        d[0..2].copy_from_slice(&14u16.to_be_bytes());
        assert_eq!(
            CmapSubtable::parse(&d),
            Err(ParseError::InvalidCmapFormat(14))
        );
        // A format-0 subtable shorter than the 262-byte layout.
        assert_eq!(
            CmapSubtable::parse(&[0, 0]),
            Err(ParseError::TruncatedTable {
                tag: *b"cmap",
                expected: 262,
                got: 2
            })
        );
        // One-byte table: truncation before the format field.
        assert_eq!(
            CmapSubtable::parse(&[0]),
            Err(ParseError::TruncatedTable {
                tag: *b"cmap",
                expected: 2,
                got: 1
            })
        );
        // format 4 with odd segCountX2.
        let mut d = format4();
        d[6..8].copy_from_slice(&3u16.to_be_bytes());
        assert_eq!(CmapSubtable::parse(&d), Err(ParseError::InvalidGlyphFormat));
    }

    #[test]
    fn cmap_header_and_records() {
        let sub = format4();
        let mut d = Vec::new();
        d.extend_from_slice(&0u16.to_be_bytes()); // version
        d.extend_from_slice(&2u16.to_be_bytes()); // numTables
                                                  // Record 0: (0, 3) Unicode BMP -> format 4 at offset 4 + 16 = 20.
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&3u16.to_be_bytes());
        d.extend_from_slice(&20u32.to_be_bytes());
        // Record 1: (3, 1) -> same subtable.
        d.extend_from_slice(&3u16.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&20u32.to_be_bytes());
        let base = d.len();
        d.extend_from_slice(&sub);
        assert_eq!(base, 20);
        let cmap = Cmap::parse(&d).unwrap();
        let recs = cmap.records();
        assert_eq!(recs.len(), 2);
        assert_eq!(
            recs[0],
            super::EncodingRecord {
                platform_id: 0,
                encoding_id: 3,
                offset: 20
            }
        );
        assert!(recs[1].is_unicode());
        assert_eq!(
            cmap.codepoint('A').map(font_types::GlyphId::to_u32),
            Some(0x43)
        );
        assert_eq!(
            cmap.codepoint('a').map(font_types::GlyphId::to_u32),
            Some(1)
        );
        assert_eq!(cmap.codepoint('\u{1F600}'), None);
        let sub0 = cmap.subtable(0).unwrap();
        assert!(matches!(sub0, CmapSubtable::Format4(_)));
    }

    #[test]
    fn cmap_header_errors() {
        assert_eq!(
            Cmap::parse(&[0, 0, 0]),
            Err(ParseError::TruncatedTable {
                tag: *b"cmap",
                expected: 4,
                got: 3
            })
        );
        // numTables beyond the buffer.
        let mut d = vec![0u8; 12];
        d[2..4].copy_from_slice(&2u16.to_be_bytes());
        assert_eq!(
            Cmap::parse(&d),
            Err(ParseError::TruncatedTable {
                tag: *b"cmap",
                expected: 20,
                got: 12
            })
        );
        // Record offset past the table.
        let mut d = vec![0u8; 20];
        d[2..4].copy_from_slice(&1u16.to_be_bytes());
        d[8..12].copy_from_slice(&500u32.to_be_bytes());
        assert_eq!(Cmap::parse(&d), Err(ParseError::InvalidGlyphFormat));
        // subtable out of range index.
        let cmap = Cmap::parse(&d);
        assert!(cmap.is_err() || cmap.unwrap().subtable(9).is_err());
    }
}
