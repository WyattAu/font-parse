//! `name` — naming table: family, style, full name, PostScript name.

use crate::error::{ParseError, RawTag};
use alloc::string::String;
use alloc::vec::Vec;

pub(crate) const TAG: RawTag = *b"name";

/// Record header size.
const RECORD: usize = 12;

/// Well-known name ids (subset; raw ids pass through).
pub mod name_id {
    /// Copyright notice.
    pub const COPYRIGHT: u16 = 0;
    /// Font family name.
    pub const FAMILY: u16 = 1;
    /// Font subfamily (style) name.
    pub const SUBFAMILY: u16 = 2;
    /// Unique font identifier.
    pub const UNIQUE_ID: u16 = 3;
    /// Full font name.
    pub const FULL_NAME: u16 = 4;
    /// Version string.
    pub const VERSION: u16 = 5;
    /// PostScript name.
    pub const POSTSCRIPT: u16 = 6;
    /// Trademark.
    pub const TRADEMARK: u16 = 7;
    /// Manufacturer.
    pub const MANUFACTURER: u16 = 8;
    /// Designer.
    pub const DESIGNER: u16 = 9;
    /// Description.
    pub const DESCRIPTION: u16 = 10;
    /// Vendor URL.
    pub const VENDOR_URL: u16 = 11;
    /// Designer URL.
    pub const DESIGNER_URL: u16 = 12;
    /// License description.
    pub const LICENSE: u16 = 13;
    /// License URL.
    pub const LICENSE_URL: u16 = 14;
    /// Preferred family.
    pub const PREFERRED_FAMILY: u16 = 16;
    /// Preferred subfamily.
    pub const PREFERRED_SUBFAMILY: u16 = 17;
}

/// One naming record: metadata ids plus the borrowed string bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameRecord<'a> {
    /// Platform id (0 Unicode, 1 Macintosh, 3 Windows).
    pub platform_id: u16,
    /// Platform-specific encoding id.
    pub encoding_id: u16,
    /// Language id.
    pub language_id: u16,
    /// What the string names (see [`name_id`]).
    pub name_id: u16,
    /// Raw string bytes.
    pub bytes: &'a [u8],
}

impl NameRecord<'_> {
    /// True for Windows (platform 3) records.
    #[must_use]
    pub const fn is_windows(self) -> bool {
        self.platform_id == 3
    }

    /// The string as `&str` when the raw bytes are valid UTF-8 (typical for
    /// ASCII Macintosh strings); `None` otherwise (use
    /// [`decode`](Self::decode)).
    #[must_use]
    pub fn to_str(&self) -> Option<&str> {
        core::str::from_utf8(self.bytes).ok()
    }

    /// Decode the string: UTF-16BE for Windows (platform 3) records,
    /// lossy byte-per-char (Latin-1) otherwise.
    #[must_use]
    pub fn decode(&self) -> String {
        if self.is_windows() {
            let mut units = Vec::with_capacity(self.bytes.len() / 2);
            for c in self.bytes.chunks_exact(2) {
                if let Ok(pair) = <[u8; 2]>::try_from(c) {
                    units.push(u16::from_be_bytes(pair));
                }
            }
            String::from_utf16_lossy(&units)
        } else {
            self.bytes.iter().map(|&b| b as char).collect()
        }
    }
}

/// Zero-copy view over the `name` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameTable<'a> {
    data: &'a [u8],
    count: u16,
    string_offset: usize,
}

impl<'a> NameTable<'a> {
    /// Parse and validate the `name` table header.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when header or records are truncated;
    /// [`ParseError::InvalidGlyphFormat`] when the string storage falls
    /// outside the table.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 6 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 6,
                got: data.len(),
            });
        }
        let count = crate::reader::be_u16_at(data, 2, TAG)?;
        let string_offset = usize::from(crate::reader::be_u16_at(data, 4, TAG)?);
        let end = 6usize
            .checked_add(
                usize::from(count)
                    .checked_mul(RECORD)
                    .ok_or(ParseError::InvalidGlyphFormat)?,
            )
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: end,
                got: data.len(),
            });
        }
        Ok(Self {
            data,
            count,
            string_offset,
        })
    }

    /// Raw table bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Format (0 or 1).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn format(&self) -> Result<u16, ParseError> {
        crate::reader::be_u16_at(self.data, 0, TAG)
    }

    /// Declared record count.
    #[must_use]
    pub const fn count(&self) -> u16 {
        self.count
    }

    /// All records, in table order.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] when a record's string range falls
    /// outside the storage area.
    pub fn records(&self) -> Result<Vec<NameRecord<'a>>, ParseError> {
        let mut out = Vec::with_capacity(usize::from(self.count));
        for i in 0..usize::from(self.count) {
            out.push(self.record_at(i)?);
        }
        Ok(out)
    }

    fn record_at(&self, i: usize) -> Result<NameRecord<'a>, ParseError> {
        let at = 6 + i * RECORD;
        let platform_id = crate::reader::be_u16_at(self.data, at, TAG)?;
        let encoding_id = crate::reader::be_u16_at(self.data, at + 2, TAG)?;
        let language_id = crate::reader::be_u16_at(self.data, at + 4, TAG)?;
        let name_id = crate::reader::be_u16_at(self.data, at + 6, TAG)?;
        let length = usize::from(crate::reader::be_u16_at(self.data, at + 8, TAG)?);
        let offset = usize::from(crate::reader::be_u16_at(self.data, at + 10, TAG)?);
        let base = self
            .string_offset
            .checked_add(offset)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        let bytes = crate::reader::slice_at(self.data, base, length, TAG)?;
        Ok(NameRecord {
            platform_id,
            encoding_id,
            language_id,
            name_id,
            bytes,
        })
    }

    /// First record matching `id`, preferring Windows (platform 3) then
    /// Unicode (platform 0) then Macintosh (platform 1) encodings.
    #[must_use]
    pub fn get(&self, id: u16) -> Option<NameRecord<'a>> {
        let mut best: Option<(u8, NameRecord<'a>)> = None;
        for i in 0..usize::from(self.count) {
            let rec = self.record_at(i).ok()?;
            if rec.name_id != id {
                continue;
            }
            let score = match rec.platform_id {
                3 => 3,
                0 => 2,
                1 => 1,
                _ => 0,
            };
            best = match best {
                None => Some((score, rec)),
                Some((s, _)) if score > s => Some((score, rec)),
                other => other,
            };
        }
        best.map(|(_, rec)| rec)
    }

    /// Family name (name id 1).
    #[must_use]
    pub fn family(&self) -> Option<String> {
        self.get(name_id::FAMILY).map(|r| r.decode())
    }

    /// Subfamily / style name (name id 2).
    #[must_use]
    pub fn subfamily(&self) -> Option<String> {
        self.get(name_id::SUBFAMILY).map(|r| r.decode())
    }

    /// Full font name (name id 4).
    #[must_use]
    pub fn full_name(&self) -> Option<String> {
        self.get(name_id::FULL_NAME).map(|r| r.decode())
    }

    /// PostScript name (name id 6).
    #[must_use]
    pub fn postscript_name(&self) -> Option<String> {
        self.get(name_id::POSTSCRIPT).map(|r| r.decode())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{name_id, NameTable};
    use alloc::vec::Vec;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    /// Two records: (3,1,0x409) family "Test" as UTF-16BE, (1,0,0) PS name
    /// "Test-Regular" as ASCII.
    fn sample() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&0u16.to_be_bytes()); // format
        d.extend_from_slice(&2u16.to_be_bytes()); // count
        let string_offset = 6u16 + 2 * 12;
        d.extend_from_slice(&string_offset.to_be_bytes());
        // Record 1: family, Windows.
        d.extend_from_slice(&3u16.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&0x0409u16.to_be_bytes());
        d.extend_from_slice(&name_id::FAMILY.to_be_bytes());
        d.extend_from_slice(&8u16.to_be_bytes()); // len
        d.extend_from_slice(&0u16.to_be_bytes()); // offset
                                                  // Record 2: PS name, Mac.
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&0u16.to_be_bytes());
        d.extend_from_slice(&name_id::POSTSCRIPT.to_be_bytes());
        d.extend_from_slice(&12u16.to_be_bytes());
        d.extend_from_slice(&8u16.to_be_bytes());
        // Strings.
        d.extend_from_slice(&[0x00, 0x54, 0x00, 0x65, 0x00, 0x73, 0x00, 0x74]); // "Test"
        d.extend_from_slice(b"Test-Regular");
        d
    }

    #[test]
    fn reads_records_and_helpers() {
        let d = sample();
        let n = NameTable::parse(&d).unwrap();
        assert_eq!(n.count(), 2);
        assert_eq!(n.format().unwrap(), 0);
        assert_eq!(n.family().as_deref(), Some("Test"));
        assert_eq!(n.postscript_name().as_deref(), Some("Test-Regular"));
        assert!(n.subfamily().is_none());
        let fam = n.get(name_id::FAMILY).unwrap();
        assert!(fam.is_windows());
        // Raw UTF-16BE bytes happen to be valid UTF-8 (all ASCII range) —
        // `to_str` passes them through; `decode` is the proper reader.
        assert_eq!(fam.decode(), "Test");
        let recs = n.records().unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[1].to_str(), Some("Test-Regular"));
    }

    #[test]
    fn errors() {
        assert_eq!(
            NameTable::parse(&[0u8; 5]),
            Err(crate::ParseError::TruncatedTable {
                tag: *b"name",
                expected: 6,
                got: 5
            })
        );
        // Records truncated.
        let mut d = sample();
        d.truncate(20);
        assert!(NameTable::parse(&d).is_err());
        // String storage outside the table: parse stays lazy (header +
        // records validated), but the records access surfaces the error.
        let mut d = sample();
        d.truncate(35);
        let n = NameTable::parse(&d).unwrap();
        assert!(n.records().is_err());
        assert!(n.get(super::name_id::FAMILY).is_none());
    }
}
