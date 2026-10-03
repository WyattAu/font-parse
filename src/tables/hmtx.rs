//! `hmtx` — horizontal metrics: advance widths and left side bearings.

use crate::error::{ParseError, RawTag};
use crate::reader::{be_i16_at, be_u16_at};

pub(crate) const TAG: RawTag = *b"hmtx";

/// Zero-copy view over the `hmtx` table.
///
/// Layout: `numberOfHMetrics` pairs of (`advanceWidth: u16`, `lsb: i16`)
/// followed by `numGlyphs - numberOfHMetrics` bare `lsb: i16` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HmtxTable<'a> {
    data: &'a [u8],
    num_h_metrics: u16,
    num_glyphs: u16,
}

impl<'a> HmtxTable<'a> {
    /// Parse and validate the `hmtx` table bytes.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the table cannot hold the declared
    /// arrays; [`ParseError::InvalidGlyphFormat`] when `numberOfHMetrics`
    /// exceeds `numGlyphs`.
    pub fn parse(data: &'a [u8], num_h_metrics: u16, num_glyphs: u16) -> Result<Self, ParseError> {
        if num_h_metrics > num_glyphs {
            return Err(ParseError::InvalidGlyphFormat);
        }
        let longs = usize::from(num_h_metrics);
        let bare = usize::from(num_glyphs - num_h_metrics);
        // 4 bytes per (advance, lsb) pair + 2 per trailing lsb.
        let expected = longs
            .checked_mul(4)
            .and_then(|v| v.checked_add(bare.checked_mul(2)?))
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if data.len() < expected {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected,
                got: data.len(),
            });
        }
        Ok(Self {
            data,
            num_h_metrics,
            num_glyphs,
        })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Declared number of (advance, lsb) entries.
    #[must_use]
    pub const fn num_h_metrics(&self) -> u16 {
        self.num_h_metrics
    }

    /// Advance width for `gid` (`gid < numGlyphs`).
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on internal inconsistency (cannot
    /// happen for a table validated by [`parse`](Self::parse)).
    pub fn advance(&self, gid: u16) -> Result<u16, ParseError> {
        let idx = usize::from(self.metric_index(gid));
        let at = idx.checked_mul(4).ok_or_else(truncated)?;
        be_u16_at(self.data, at, TAG)
    }

    /// Left side bearing for `gid`, from the long or bare array as
    /// appropriate.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on internal inconsistency.
    pub fn left_side_bearing(&self, gid: u16) -> Result<i16, ParseError> {
        let gid = usize::from(gid);
        let num_h = usize::from(self.num_h_metrics);
        let at = if gid < num_h {
            // Long array: (advance u16, lsb i16) pairs.
            gid.checked_mul(4)
                .and_then(|v| v.checked_add(2))
                .ok_or_else(truncated)?
        } else {
            // Bare lsb array starts after the long entries.
            num_h
                .checked_mul(4)
                .and_then(|v| v.checked_add((gid - num_h).checked_mul(2)?))
                .ok_or_else(truncated)?
        };
        be_i16_at(self.data, at, TAG)
    }

    /// Index of the governing (advance, lsb) entry: the glyph's own entry,
    /// or the last one beyond `numberOfHMetrics`.
    fn metric_index(&self, gid: u16) -> u16 {
        if gid < self.num_h_metrics {
            gid
        } else if gid < self.num_glyphs {
            self.num_h_metrics.saturating_sub(1)
        } else {
            // Out-of-range ids pin the last declared entry; callers wanting
            // strictness bound `gid` by `maxp.numGlyphs` themselves.
            self.num_glyphs.checked_sub(1).unwrap_or(self.num_h_metrics)
        }
    }
}

/// Pre-built truncation error for internal consistency paths.
fn truncated() -> ParseError {
    ParseError::TruncatedTable {
        tag: TAG,
        expected: usize::MAX,
        got: 0,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::HmtxTable;
    use crate::error::ParseError;
    use alloc::vec::Vec;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    /// 3 long entries + 2 bare lsbs = 12 + 4 = 16 bytes.
    fn sample() -> Vec<u8> {
        let mut d = Vec::new();
        for (adv, lsb) in [(500i16, 40i16), (600, 50), (550, 30)] {
            d.extend_from_slice(&adv.to_be_bytes());
            d.extend_from_slice(&lsb.to_be_bytes());
        }
        d.extend_from_slice(&70i16.to_be_bytes());
        d.extend_from_slice(&80i16.to_be_bytes());
        d
    }

    #[test]
    fn long_and_bear_entries() {
        let d = sample();
        let h = HmtxTable::parse(&d, 3, 5).unwrap();
        assert_eq!(h.advance(0).unwrap(), 500);
        assert_eq!(h.advance(2).unwrap(), 550);
        // Bare glyphs pin the last advance.
        assert_eq!(h.advance(3).unwrap(), 550);
        assert_eq!(h.advance(4).unwrap(), 550);
        assert_eq!(h.left_side_bearing(0).unwrap(), 40);
        assert_eq!(h.left_side_bearing(2).unwrap(), 30);
        assert_eq!(h.left_side_bearing(3).unwrap(), 70);
        assert_eq!(h.left_side_bearing(4).unwrap(), 80);
        assert_eq!(h.num_h_metrics(), 3);
    }

    #[test]
    fn single_entry_font() {
        let d = sample();
        let h = HmtxTable::parse(&d, 1, 1).unwrap();
        assert_eq!(h.advance(0).unwrap(), 500);
        assert_eq!(h.left_side_bearing(0).unwrap(), 40);
    }

    #[test]
    fn errors() {
        let d = sample();
        // numberOfHMetrics > numGlyphs.
        assert_eq!(
            HmtxTable::parse(&d, 4, 3),
            Err(ParseError::InvalidGlyphFormat)
        );
        // Table too short for declared arrays.
        assert_eq!(
            HmtxTable::parse(&d, 5, 5),
            Err(ParseError::TruncatedTable {
                tag: *b"hmtx",
                expected: 20,
                got: 16
            })
        );
    }
}
