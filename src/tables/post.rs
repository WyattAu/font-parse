//! `post` — PostScript table: italic angle, underline metrics, glyph names.

use crate::error::{ParseError, RawTag};
use crate::reader::Reader;
use font_types::Fixed;

pub(crate) const TAG: RawTag = *b"post";

/// Header size (all versions).
const HEADER: usize = 32;
/// Number of standard Macintosh glyph names.
const STD_NAMES: usize = 258;

/// The 258 standard Macintosh glyph names (TrueType spec appendix), indexed
/// by their post v2 `glyphNameIndex` values 0..257.
#[rustfmt::skip]
pub(crate) const MAC_GLYPH_NAMES: [&str; STD_NAMES] = [
    ".notdef", ".null", "nonmarkingreturn", "space", "exclam", "quotedbl", "numbersign",
    "dollar", "percent", "ampersand", "quotesingle", "parenleft", "parenright", "asterisk",
    "plus", "comma", "hyphen", "period", "slash", "zero", "one", "two", "three", "four",
    "five", "six", "seven", "eight", "nine", "colon", "semicolon", "less", "equal", "greater",
    "question", "at", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N",
    "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "bracketleft", "backslash",
    "bracketright", "asciicircum", "underscore", "grave", "a", "b", "c", "d", "e", "f", "g",
    "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y",
    "z", "braceleft", "bar", "braceright", "asciitilde", "Adieresis", "Aring", "Ccedilla",
    "Eacute", "Ntilde", "Odieresis", "Udieresis", "aacute", "agrave", "acircumflex",
    "adieresis", "atilde", "aring", "ccedilla", "eacute", "egrave", "ecircumflex",
    "edieresis", "iacute", "igrave", "icircumflex", "idieresis", "ntilde", "oacute",
    "ograve", "ocircumflex", "odieresis", "otilde", "uacute", "ugrave", "ucircumflex",
    "udieresis", "dagger", "degree", "cent", "sterling", "section", "bullet", "paragraph",
    "germandbls", "registered", "copyright", "trademark", "acute", "dieresis", "notequal",
    "AE", "Oslash", "infinity", "plusminus", "lessequal", "greaterequal", "yen", "mu",
    "partialdiff", "summation", "product", "pi", "integral", "ordfeminine", "ordmasculine",
    "Omega", "ae", "oslash", "questiondown", "exclamdown", "logicalnot", "radical",
    "florin", "approxequal", "Delta", "guillemotleft", "guillemotright", "ellipsis",
    "nonbreakingspace", "Agrave", "Atilde", "Otilde", "OE", "oe", "endash", "emdash",
    "quotedblleft", "quotedblright", "quoteleft", "quoteright", "divide", "lozenge",
    "ydieresis", "Ydieresis", "fraction", "currency", "guilsinglleft", "guilsinglright",
    "fi", "fl", "daggerdbl", "periodcentered", "quotesinglbase", "quotedblbase",
    "perthousand", "Acircumflex", "Ecircumflex", "Aacute", "Edieresis", "Egrave", "Iacute",
    "Icircumflex", "Idieresis", "Igrave", "Oacute", "Ocircumflex", "apple", "Ograve",
    "Uacute", "Ucircumflex", "Ugrave", "dotlessi", "circumflex", "tilde", "macron",
    "breve", "dotaccent", "ring", "cedilla", "hungarumlaut", "ogonek", "caron", "Lslash",
    "lslash", "Scaron", "scaron", "Zcaron", "zcaron", "brokenbar", "Eth", "eth", "Yacute",
    "yacute", "Thorn", "thorn", "minus", "multiply", "onesuperior", "twosuperior",
    "threesuperior", "onehalf", "onequarter", "threequarters", "franc", "Gbreve",
    "gbreve", "Idotaccent", "Scedilla", "scedilla", "Cacute", "cacute", "Ccaron", "ccaron",
    "dcroat",
];

/// Zero-copy view over the `post` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostTable<'a> {
    data: &'a [u8],
}

impl<'a> PostTable<'a> {
    /// Parse and validate the `post` table header.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when shorter than 32 bytes.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < HEADER {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: HEADER,
                got: data.len(),
            });
        }
        Ok(Self { data })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Raw version (`0x00010000`, `0x00020000`, `0x00030000`).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn version(&self) -> Result<u32, ParseError> {
        crate::reader::be_u32_at(self.data, 0, TAG)
    }

    /// Italic angle in degrees counter-clockwise from vertical.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn italic_angle(&self) -> Result<f32, ParseError> {
        Ok(Fixed::from_bits(crate::reader::be_i32_at(self.data, 4, TAG)?).to_f32())
    }

    /// Underline position.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn underline_position(&self) -> Result<i16, ParseError> {
        crate::reader::be_i16_at(self.data, 8, TAG)
    }

    /// Underline thickness.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn underline_thickness(&self) -> Result<i16, ParseError> {
        crate::reader::be_i16_at(self.data, 10, TAG)
    }

    /// Non-zero for monospaced fonts.
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn is_fixed_pitch(&self) -> Result<u32, ParseError> {
        crate::reader::be_u32_at(self.data, 12, TAG)
    }

    /// Glyph name for `gid` (version 2 tables).
    ///
    /// Returns `None` for glyphs with no name (index 0/`NULL` handling per
    /// spec: unmapped indices yield no name) or non-v2 tables.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when the v2 arrays are truncated;
    /// [`ParseError::InvalidGlyphFormat`] when `glyphNameIndex` values run
    /// past the custom-name storage.
    pub fn glyph_name(&self, gid: u16) -> Result<Option<&'a str>, ParseError> {
        if self.version()? != 0x0002_0000 {
            return Ok(None);
        }
        if self.data.len() < 34 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 34,
                got: self.data.len(),
            });
        }
        let n = usize::from(crate::reader::be_u16_at(self.data, 32, TAG)?);
        if usize::from(gid) >= n {
            return Ok(None);
        }
        let index = crate::reader::be_u16_at(self.data, 34 + usize::from(gid) * 2, TAG)?;
        if let Some(name) = MAC_GLYPH_NAMES.get(usize::from(index)) {
            return Ok(Some(*name));
        }
        // Custom name number `index - 258`: the Nth Pascal string after the
        // glyphNameIndex array.
        let custom = usize::from(index) - STD_NAMES;
        let names_start = 34 + n * 2;
        let mut r = Reader::within(self.data, names_start, self.data.len(), TAG);
        for _ in 0..custom {
            let len = usize::from(r.u8()?);
            r.skip(len)?;
        }
        let len = usize::from(r.u8()?);
        let bytes = r.bytes(len)?;
        Ok(core::str::from_utf8(bytes).ok())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::PostTable;
    use crate::error::ParseError;
    use alloc::vec::Vec;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    fn v2() -> Vec<u8> {
        // Header + numberOfGlyphs + 3 glyphNameIndex entries + 2 custom
        // Pascal strings ("customglyph", "uni0041alt").
        let mut d = Vec::new();
        d.extend_from_slice(&0x0002_0000u32.to_be_bytes());
        d.extend_from_slice(&(-1200i32).to_be_bytes()); // italic angle
        d.extend_from_slice(&(-50i16).to_be_bytes()); // underline position
        d.extend_from_slice(&10i16.to_be_bytes());
        d.extend_from_slice(&1u32.to_be_bytes()); // fixed pitch
        d.extend_from_slice(&0u32.to_be_bytes()); // 4 memory fields
        d.extend_from_slice(&0u32.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes());
        d.extend_from_slice(&3u16.to_be_bytes()); // numberOfGlyphs
                                                  // Indexes: 0 -> ".notdef", 3 -> "space", 258 -> custom[0].
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&3u16.to_be_bytes());
        d.extend_from_slice(&258u16.to_be_bytes());
        d.extend_from_slice(&[11u8]);
        d.extend_from_slice(b"customglyph");
        d.extend_from_slice(&[10u8]);
        d.extend_from_slice(b"uni0041alt");
        d
    }

    #[test]
    fn header_fields() {
        let d = v2();
        let p = PostTable::parse(&d).unwrap();
        assert_eq!(p.version().unwrap(), 0x0002_0000);
        assert!((p.italic_angle().unwrap() - -0.018_310_547).abs() < 1e-6);
        assert_eq!(p.underline_position().unwrap(), -50);
        assert_eq!(p.underline_thickness().unwrap(), 10);
        assert_eq!(p.is_fixed_pitch().unwrap(), 1);
    }

    #[test]
    fn glyph_names_std_and_custom() {
        let d = v2();
        let p = PostTable::parse(&d).unwrap();
        assert_eq!(p.glyph_name(0).unwrap(), Some(".notdef"));
        assert_eq!(p.glyph_name(1).unwrap(), Some("space"));
        assert_eq!(p.glyph_name(2).unwrap(), Some("customglyph"));
        assert_eq!(p.glyph_name(3).unwrap(), None); // beyond numberOfGlyphs
    }

    #[test]
    fn v3_has_no_names() {
        let mut d = [0u8; 32];
        d[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
        let p = PostTable::parse(&d).unwrap();
        assert_eq!(p.glyph_name(0).unwrap(), None);
        assert_eq!(p.italic_angle().unwrap(), 0.0);
    }

    #[test]
    fn truncated_and_bad_custom_index() {
        assert_eq!(
            PostTable::parse(&[0u8; 31]),
            Err(ParseError::TruncatedTable {
                tag: *b"post",
                expected: 32,
                got: 31
            })
        );
        // A 32-byte v2 table parses but has no room for the glyph arrays.
        let d = v2();
        let p = PostTable::parse(&d[..32]).unwrap();
        assert!(p.glyph_name(0).is_err());
        // Custom index beyond the string storage -> error, not panic.
        let mut d = v2();
        let at = 34 + 2 * 2;
        d[at..at + 2].copy_from_slice(&9999u16.to_be_bytes());
        let p = PostTable::parse(&d).unwrap();
        assert!(p.glyph_name(2).is_err());
    }
}
