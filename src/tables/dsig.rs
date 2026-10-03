//! `DSIG` — digital signature table (header + signature records only).

use crate::error::{ParseError, RawTag};
use crate::reader::{be_u16_at, be_u32_at};
use alloc::vec::Vec;

pub(crate) const TAG: RawTag = *b"DSIG";

/// One signature record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureRecord {
    /// Signature format (1 = PKCS#7 blobs).
    pub format: u32,
    /// Signature length in bytes.
    pub length: u32,
    /// Signature offset from the start of the `DSIG` table.
    pub offset: u32,
}

/// Zero-copy view over the `DSIG` table (header + records; the PKCS#7
/// payloads are exposed as raw bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsigTable<'a> {
    data: &'a [u8],
}

impl<'a> DsigTable<'a> {
    /// Parse and validate the `DSIG` header.
    ///
    /// # Errors
    /// [`ParseError::TruncatedTable`] / [`ParseError::InvalidGlyphFormat`]
    /// for short or inconsistent tables.
    pub fn parse(data: &'a [u8]) -> Result<Self, ParseError> {
        if data.len() < 8 {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 8,
                got: data.len(),
            });
        }
        let count = usize::from(be_u16_at(data, 4, TAG)?);
        let end = 8usize
            .checked_add(count * 12)
            .ok_or(ParseError::InvalidGlyphFormat)?;
        if end > data.len() {
            return Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: end,
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

    /// Version (1).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn version(&self) -> Result<u32, ParseError> {
        be_u32_at(self.data, 0, TAG)
    }

    /// Permission flags (bit 0 = cannot be resigned).
    ///
    /// # Errors
    /// Truncation (impossible post-[`parse`](Self::parse)).
    pub fn flags(&self) -> Result<u16, ParseError> {
        be_u16_at(self.data, 2, TAG)
    }

    /// Signature records, in table order.
    ///
    /// # Errors
    /// [`ParseError::InvalidGlyphFormat`] when a record's byte range falls
    /// outside the table.
    pub fn signatures(&self) -> Result<Vec<(&'a [u8], SignatureRecord)>, ParseError> {
        let count = usize::from(be_u16_at(self.data, 4, TAG)?);
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let at = 8 + i * 12;
            let rec = SignatureRecord {
                format: be_u32_at(self.data, at, TAG)?,
                length: be_u32_at(self.data, at + 4, TAG)?,
                offset: be_u32_at(self.data, at + 8, TAG)?,
            };
            let len = usize::try_from(rec.length).map_err(|_| ParseError::InvalidGlyphFormat)?;
            let off = usize::try_from(rec.offset).map_err(|_| ParseError::InvalidGlyphFormat)?;
            out.push((crate::reader::slice_at(self.data, off, len, TAG)?, rec));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::DsigTable;
    use crate::error::ParseError;
    use alloc::vec::Vec;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    fn sample() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&1u32.to_be_bytes()); // version
        d.extend_from_slice(&1u16.to_be_bytes()); // flags
        d.extend_from_slice(&1u16.to_be_bytes()); // numSignatures
        d.extend_from_slice(&1u32.to_be_bytes()); // format
        d.extend_from_slice(&4u32.to_be_bytes()); // length
        d.extend_from_slice(&20u32.to_be_bytes()); // offset
        d.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        d
    }

    #[test]
    fn parses_header_and_records() {
        let d = sample();
        let t = DsigTable::parse(&d).unwrap();
        assert_eq!(t.version().unwrap(), 1);
        assert_eq!(t.flags().unwrap(), 1);
        let sigs = t.signatures().unwrap();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].1.format, 1);
        assert_eq!(sigs[0].0, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn errors() {
        assert_eq!(
            DsigTable::parse(&[0u8; 7]),
            Err(ParseError::TruncatedTable {
                tag: *b"DSIG",
                expected: 8,
                got: 7
            })
        );
        // Record array truncated.
        let mut d = sample();
        d.truncate(15);
        assert!(DsigTable::parse(&d).is_err());
        // Signature bytes outside the table.
        let mut d = sample();
        d.truncate(22);
        let t = DsigTable::parse(&d).unwrap();
        assert!(matches!(
            t.signatures(),
            Err(ParseError::TruncatedTable { .. })
        ));
    }
}
