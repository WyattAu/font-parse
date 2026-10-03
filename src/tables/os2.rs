//! `OS/2` — Windows metrics: weight/width classes, Unicode ranges, metrics.

use crate::error::{ParseError, RawTag};
use crate::reader::{be_i16_at, be_u16_at, be_u32_at};

pub(crate) const TAG: RawTag = *b"OS/2";

/// Weight class constants (usWeightClass).
pub mod weight {
    /// Thin (100).
    pub const THIN: u16 = 100;
    /// ExtraLight (200).
    pub const EXTRA_LIGHT: u16 = 200;
    /// Light (300).
    pub const LIGHT: u16 = 300;
    /// Regular (400).
    pub const REGULAR: u16 = 400;
    /// Medium (500).
    pub const MEDIUM: u16 = 500;
    /// SemiBold (600).
    pub const SEMI_BOLD: u16 = 600;
    /// Bold (700).
    pub const BOLD: u16 = 700;
    /// ExtraBold (800).
    pub const EXTRA_BOLD: u16 = 800;
    /// Black (900).
    pub const BLACK: u16 = 900;
}

/// Width class constants (usWidthClass).
pub mod width {
    /// Ultra-condensed (1).
    pub const ULTRA_CONDENSED: u16 = 1;
    /// Extra-condensed (2).
    pub const EXTRA_CONDENSED: u16 = 2;
    /// Condensed (3).
    pub const CONDENSED: u16 = 3;
    /// Semi-condensed (4).
    pub const SEMI_CONDENSED: u16 = 4;
    /// Medium/normal (5).
    pub const MEDIUM: u16 = 5;
    /// Semi-expanded (6).
    pub const SEMI_EXPANDED: u16 = 6;
    /// Expanded (7).
    pub const EXPANDED: u16 = 7;
    /// Extra-expanded (8).
    pub const EXTRA_EXPANDED: u16 = 8;
    /// Ultra-expanded (9).
    pub const ULTRA_EXPANDED: u16 = 9;
}

/// Zero-copy view over the `OS/2` table.
///
/// Field availability is length-driven (v0 = 78 bytes, v1 = 86, v2 = 96,
/// v5 = 100), so partial tables parse instead of failing wholesale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Os2Table<'a> {
    data: &'a [u8],
}

impl<'a> Os2Table<'a> {
    /// Parse and validate the `OS/2` table bytes.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when shorter than the version-0 size
    /// (78 bytes); [`ParseError::InvalidSfnt`] for an unknown version.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 78 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 78,
                got: data.len(),
            });
        }
        match be_u16_at(data, 0, TAG)? {
            0..=5 => {}
            _ => return Err(ParseError::InvalidSfnt),
        }
        Ok(Self { data })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Table version (0-5).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn version(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 0, TAG)
    }

    /// Average character width.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn avg_char_width(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 2, TAG)
    }

    /// Weight class (100-900; see [`weight`]).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn weight_class(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 4, TAG)
    }

    /// Width class (1-9; see [`width`]).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn width_class(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 6, TAG)
    }

    /// Embedding permissions (`fsType`).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn fs_type(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 8, TAG)
    }

    /// Classification id (`sFamilyClass`).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn family_class(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 30, TAG)
    }

    /// PANOSE classification bytes (10 bytes at offset 32).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn panose(&self) -> Result<&'a [u8], ParseError> {
        crate::reader::slice_at(self.data, 32, 10, TAG)
    }

    /// Unicode range bits (`ulUnicodeRange1..4`) as four raw `u32` words;
    /// bit `n` of word `w` marks range `w*32 + n`.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn unicode_ranges(&self) -> Result<[u32; 4], ParseError> {
        Ok([
            be_u32_at(self.data, 42, TAG)?,
            be_u32_at(self.data, 46, TAG)?,
            be_u32_at(self.data, 50, TAG)?,
            be_u32_at(self.data, 54, TAG)?,
        ])
    }

    /// Four-character vendor id.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn vendor_id(&self) -> Result<&'a [u8], ParseError> {
        crate::reader::slice_at(self.data, 58, 4, TAG)
    }

    /// `fsSelection` — italic/bold/regular/… selection bits.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn fs_selection(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 62, TAG)
    }

    /// Typographic ascender (v0+).
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a table shorter than 78 bytes
    /// (impossible post-[`parse`](Self::parse)).
    pub fn typo_ascender(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 68, TAG)
    }

    /// Typographic descender (v0+).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn typo_descender(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 70, TAG)
    }

    /// Typographic line gap (v0+).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn typo_line_gap(&self) -> Result<i16, ParseError> {
        be_i16_at(self.data, 72, TAG)
    }

    /// Windows ascent (v0+).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn win_ascent(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 74, TAG)
    }

    /// Windows descent (v0+).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn win_descent(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 76, TAG)
    }

    /// Code page range bits (`ulCodePageRange1..2`, v1+).
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a pre-v1 (78-byte) table.
    pub fn code_page_ranges(&self) -> Result<[u32; 2], ParseError> {
        if self.data.len() < 86 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 86,
                got: self.data.len(),
            });
        }
        Ok([
            be_u32_at(self.data, 78, TAG)?,
            be_u32_at(self.data, 82, TAG)?,
        ])
    }

    /// x-height (`sxHeight`, v2+).
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a pre-v2 table.
    pub fn x_height(&self) -> Result<i16, ParseError> {
        if self.data.len() < 96 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 96,
                got: self.data.len(),
            });
        }
        be_i16_at(self.data, 86, TAG)
    }

    /// Cap height (`sCapHeight`, v2+).
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] on a pre-v2 table.
    pub fn cap_height(&self) -> Result<i16, ParseError> {
        if self.data.len() < 96 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 96,
                got: self.data.len(),
            });
        }
        be_i16_at(self.data, 88, TAG)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::Os2Table;
    use crate::error::ParseError;
    use alloc::vec;
    use alloc::vec::Vec;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    fn sample(len: usize) -> Vec<u8> {
        let mut d = vec![0u8; len];
        d[0..2].copy_from_slice(&2u16.to_be_bytes()); // v2
        d[2..4].copy_from_slice(&450i16.to_be_bytes());
        d[4..6].copy_from_slice(&700u16.to_be_bytes());
        d[6..8].copy_from_slice(&5u16.to_be_bytes());
        d[8..10].copy_from_slice(&8u16.to_be_bytes()); // installable
        d[30..32].copy_from_slice(&0x0401i16.to_be_bytes());
        d[32..42].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        d[42..46].copy_from_slice(&0b11u32.to_be_bytes()); // ranges bit 0/1
        d[58..62].copy_from_slice(b"TEST");
        d[62..64].copy_from_slice(&0x40u16.to_be_bytes()); // bold
        d[68..70].copy_from_slice(&800i16.to_be_bytes());
        d[70..72].copy_from_slice(&(-200i16).to_be_bytes());
        d[72..74].copy_from_slice(&50i16.to_be_bytes());
        d[74..76].copy_from_slice(&850u16.to_be_bytes());
        d[76..78].copy_from_slice(&250u16.to_be_bytes());
        if len >= 86 {
            d[78..82].copy_from_slice(&0x1u32.to_be_bytes());
            d[82..86].copy_from_slice(&0x2u32.to_be_bytes());
        }
        if len >= 96 {
            d[86..88].copy_from_slice(&500i16.to_be_bytes());
            d[88..90].copy_from_slice(&700i16.to_be_bytes());
        }
        d
    }

    #[test]
    fn v2_fields() {
        let d = sample(96);
        let t = Os2Table::parse(&d).unwrap();
        assert_eq!(t.version().unwrap(), 2);
        assert_eq!(t.avg_char_width().unwrap(), 450);
        assert_eq!(t.weight_class().unwrap(), super::weight::BOLD);
        assert_eq!(t.width_class().unwrap(), super::width::MEDIUM);
        assert_eq!(t.fs_type().unwrap(), 8);
        assert_eq!(t.family_class().unwrap(), 0x0401);
        assert_eq!(t.panose().unwrap(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(t.unicode_ranges().unwrap()[0], 0b11);
        assert_eq!(t.vendor_id().unwrap(), b"TEST");
        assert_eq!(t.fs_selection().unwrap(), 0x40);
        assert_eq!(t.typo_ascender().unwrap(), 800);
        assert_eq!(t.typo_descender().unwrap(), -200);
        assert_eq!(t.typo_line_gap().unwrap(), 50);
        assert_eq!(t.win_ascent().unwrap(), 850);
        assert_eq!(t.win_descent().unwrap(), 250);
        assert_eq!(t.code_page_ranges().unwrap(), [1, 2]);
        assert_eq!(t.x_height().unwrap(), 500);
        assert_eq!(t.cap_height().unwrap(), 700);
    }

    #[test]
    fn version_gating() {
        // v0-sized table: codepage and x-height unavailable.
        let d = sample(78);
        let t = Os2Table::parse(&d).unwrap();
        assert!(t.code_page_ranges().is_err());
        assert!(t.x_height().is_err());
        assert_eq!(t.typo_ascender().unwrap(), 800);
        // v1-sized table.
        let d = sample(86);
        let t = Os2Table::parse(&d).unwrap();
        assert_eq!(t.code_page_ranges().unwrap(), [1, 2]);
        assert!(t.x_height().is_err());
    }

    #[test]
    fn errors() {
        assert_eq!(
            Os2Table::parse(&[0u8; 77]),
            Err(ParseError::TruncatedTable {
                tag: *b"OS/2",
                expected: 78,
                got: 77
            })
        );
        let mut d = sample(96);
        d[0..2].copy_from_slice(&9u16.to_be_bytes());
        assert_eq!(Os2Table::parse(&d), Err(ParseError::InvalidSfnt));
    }
}
