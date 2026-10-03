//! Bounds-checked big-endian reads over `&[u8]` — the total-read substrate
//! every parser in the crate is built on.
//!
//! Every method validates the read against the buffer and returns a typed
//! [`ParseError::TruncatedTable`] on shortfall (never panics, never wraps).
//! Integer conversion is exclusively `from_be_bytes`-style via
//! [`font_types::BigEndian`] — byteorder-free by the estate convention.

use crate::error::{ParseError, RawTag};
use font_types::BigEndian;

/// A cursor over a byte slice with an error-context tag.
///
/// `tag` is the table (or container) being read; it rides along purely so
/// truncation errors identify their source.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    tag: RawTag,
}

impl<'a> Reader<'a> {
    /// A reader positioned at the start of `data`; reads report `tag`.
    pub(crate) const fn new(data: &'a [u8], tag: RawTag) -> Self {
        Self { data, pos: 0, tag }
    }

    /// A reader over `data[..end]` positioned at `start`.
    pub(crate) fn within(data: &'a [u8], start: usize, end: usize, tag: RawTag) -> Self {
        let end = end.min(data.len());
        let start = start.min(end);
        Self {
            data: data.get(start..end).unwrap_or(&[]),
            pos: 0,
            tag,
        }
    }

    fn err(&self, need: usize) -> ParseError {
        ParseError::TruncatedTable {
            tag: self.tag,
            expected: need,
            got: self.data.len(),
        }
    }

    /// Bytes consumed so far (relative to the reader's start).
    pub(crate) const fn pos(&self) -> usize {
        self.pos
    }

    /// Move to an absolute position (relative to the reader's start).
    pub(crate) fn seek(&mut self, pos: usize) -> Result<(), ParseError> {
        if pos <= self.data.len() {
            self.pos = pos;
            Ok(())
        } else {
            Err(self.err(pos))
        }
    }

    /// Skip `n` bytes.
    pub(crate) fn skip(&mut self, n: usize) -> Result<(), ParseError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| self.err(usize::MAX))?;
        self.seek(end)
    }

    /// Read `n` raw bytes.
    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| self.err(usize::MAX))?;
        let out = self.data.get(self.pos..end).ok_or_else(|| self.err(end))?;
        self.pos = end;
        Ok(out)
    }

    fn scalar<T: font_types::Scalar<Raw = R>, R>(&mut self, n: usize) -> Result<T, ParseError> {
        let raw = self.bytes(n)?;
        // Scalar::Raw is a [u8; N] matching T's size; from_slice validates
        // the length again, so the mapping is infallible here.
        let be = BigEndian::<T>::from_slice(raw).ok_or_else(|| self.err(self.pos))?;
        Ok(be.get())
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ParseError> {
        self.scalar::<u8, [u8; 1]>(1)
    }

    pub(crate) fn i8(&mut self) -> Result<i8, ParseError> {
        self.scalar::<i8, [u8; 1]>(1)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, ParseError> {
        self.scalar::<u16, [u8; 2]>(2)
    }

    pub(crate) fn i16(&mut self) -> Result<i16, ParseError> {
        self.scalar::<i16, [u8; 2]>(2)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, ParseError> {
        self.scalar::<u32, [u8; 4]>(4)
    }

    pub(crate) fn i32(&mut self) -> Result<i32, ParseError> {
        self.scalar::<i32, [u8; 4]>(4)
    }

    /// A four-byte tag.
    #[cfg(feature = "woff2")]
    pub(crate) fn tag_bytes(&mut self) -> Result<RawTag, ParseError> {
        let b = self.bytes(4)?;
        let mut tag: RawTag = *b"\0\0\0\0";
        if let Ok(found) = <&[u8; 4]>::try_from(b) {
            tag = *found;
        }
        Ok(tag)
    }

    /// A 2.14 fixed-point value.
    pub(crate) fn f2dot14(&mut self) -> Result<font_types::F2Dot14, ParseError> {
        Ok(font_types::F2Dot14::from_bits(self.i16()?))
    }

    /// Read a WOFF2 `UIntBase128` value. Rejects leading zeros, sequences
    /// longer than five bytes, and values above `u32::MAX` (spec §3.1).
    #[cfg(feature = "woff2")]
    pub(crate) fn base128(&mut self) -> Result<u32, ParseError> {
        let mut accum: u32 = 0;
        for i in 0..5 {
            let byte = self.u8()?;
            if i == 0 && byte == 0x80 {
                return Err(ParseError::InvalidSfnt); // leading zero
            }
            if accum & 0xFE00_0000 != 0 {
                return Err(ParseError::InvalidSfnt); // would overflow u32
            }
            accum = (accum << 7) | u32::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                return Ok(accum);
            }
        }
        Err(ParseError::InvalidSfnt) // > 5 bytes
    }

    /// Read a WOFF2 `255UInt16` value (spec §3.1).
    #[cfg(feature = "woff2")]
    pub(crate) fn u255(&mut self) -> Result<u16, ParseError> {
        const WORD_CODE: u8 = 253;
        const ONE_MORE_1: u8 = 255;
        const ONE_MORE_2: u8 = 254;
        const LOWEST: u16 = 253;
        let code = self.u8()?;
        Ok(match code {
            WORD_CODE => {
                let hi = self.u8()?;
                let lo = self.u8()?;
                (u16::from(hi) << 8) | u16::from(lo)
            }
            ONE_MORE_1 => u16::from(self.u8()?).wrapping_add(LOWEST),
            ONE_MORE_2 => u16::from(self.u8()?).wrapping_add(LOWEST * 2),
            v => u16::from(v),
        })
    }
}

/// Read a big-endian `u16` at `at` within `data`, mapped to a `TruncatedTable`
/// error for `tag`.
pub(crate) fn be_u16_at(data: &[u8], at: usize, tag: RawTag) -> Result<u16, ParseError> {
    let end = at.checked_add(2).ok_or(ParseError::TruncatedTable {
        tag,
        expected: usize::MAX,
        got: data.len(),
    })?;
    let raw = data.get(at..end).ok_or(ParseError::TruncatedTable {
        tag,
        expected: end,
        got: data.len(),
    })?;
    BigEndian::<u16>::from_slice(raw)
        .map(|v| v.get())
        .ok_or(ParseError::TruncatedTable {
            tag,
            expected: end,
            got: data.len(),
        })
}

/// Read a big-endian `i16` at `at` within `data`.
pub(crate) fn be_i16_at(data: &[u8], at: usize, tag: RawTag) -> Result<i16, ParseError> {
    let end = at.checked_add(2).ok_or(ParseError::TruncatedTable {
        tag,
        expected: usize::MAX,
        got: data.len(),
    })?;
    let raw = data.get(at..end).ok_or(ParseError::TruncatedTable {
        tag,
        expected: end,
        got: data.len(),
    })?;
    BigEndian::<i16>::from_slice(raw)
        .map(|v| v.get())
        .ok_or(ParseError::TruncatedTable {
            tag,
            expected: end,
            got: data.len(),
        })
}

/// Read a big-endian `u32` at `at` within `data`.
pub(crate) fn be_u32_at(data: &[u8], at: usize, tag: RawTag) -> Result<u32, ParseError> {
    let end = at.checked_add(4).ok_or(ParseError::TruncatedTable {
        tag,
        expected: usize::MAX,
        got: data.len(),
    })?;
    let raw = data.get(at..end).ok_or(ParseError::TruncatedTable {
        tag,
        expected: end,
        got: data.len(),
    })?;
    BigEndian::<u32>::from_slice(raw)
        .map(|v| v.get())
        .ok_or(ParseError::TruncatedTable {
            tag,
            expected: end,
            got: data.len(),
        })
}

/// Read a big-endian `i32` at `at` within `data`.
pub(crate) fn be_i32_at(data: &[u8], at: usize, tag: RawTag) -> Result<i32, ParseError> {
    let end = at.checked_add(4).ok_or(ParseError::TruncatedTable {
        tag,
        expected: usize::MAX,
        got: data.len(),
    })?;
    let raw = data.get(at..end).ok_or(ParseError::TruncatedTable {
        tag,
        expected: end,
        got: data.len(),
    })?;
    BigEndian::<i32>::from_slice(raw)
        .map(|v| v.get())
        .ok_or(ParseError::TruncatedTable {
            tag,
            expected: end,
            got: data.len(),
        })
}

/// Slice `data[at..at+n]` with a truncation error carrying `tag`.
pub(crate) fn slice_at(data: &[u8], at: usize, n: usize, tag: RawTag) -> Result<&[u8], ParseError> {
    let end = at.checked_add(n).ok_or(ParseError::TruncatedTable {
        tag,
        expected: usize::MAX,
        got: data.len(),
    })?;
    data.get(at..end).ok_or(ParseError::TruncatedTable {
        tag,
        expected: end,
        got: data.len(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::*;

    // Test harness: assertions legitimately panic; the parse paths under test
    // (the lib target) remain lint-clean.

    const TAG: RawTag = *b"test";

    #[test]
    fn reads_are_bounds_checked() {
        let data = [0x01, 0x02, 0x03, 0x04];
        let mut r = Reader::new(&data, TAG);
        assert_eq!(r.u16().unwrap(), 0x0102);
        assert_eq!(r.pos(), 2);
        // Exact fit.
        assert_eq!(r.u16().unwrap(), 0x0304);
        // Truncated u16 at end of buffer.
        assert_eq!(
            r.u16(),
            Err(ParseError::TruncatedTable {
                tag: TAG,
                expected: 6,
                got: 4
            })
        );
        // A fresh reader at the last byte: u8 ok, u16 short.
        let mut r1 = Reader::within(&data, 3, 4, TAG);
        assert_eq!(r1.u8().unwrap(), 4);
        assert!(r1.u16().is_err());
    }

    #[test]
    fn absolute_reads_and_slices() {
        let data = [0u8, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(be_u16_at(&data, 0, TAG).unwrap(), 0x0001);
        assert_eq!(be_i16_at(&data, 6, TAG).unwrap(), 0x0607);
        assert_eq!(be_u32_at(&data, 4, TAG).unwrap(), 0x0405_0607);
        assert_eq!(slice_at(&data, 2, 4, TAG).unwrap(), &[2, 3, 4, 5]);
        assert!(be_u16_at(&data, 7, TAG).is_err());
        assert!(be_u32_at(&data, 5, TAG).is_err());
        assert!(slice_at(&data, 5, 4, TAG).is_err());
        assert!(be_u16_at(&data, usize::MAX, TAG).is_err());
    }

    #[cfg(feature = "woff2")]
    #[test]
    fn base128_roundtrip_and_rejections() {
        // Single byte.
        let mut r = Reader::new(&[0x3F], TAG);
        assert_eq!(r.base128().unwrap(), 63);
        // Two bytes: 0x81 0x00 -> 128.
        let mut r = Reader::new(&[0x81, 0x00], TAG);
        assert_eq!(r.base128().unwrap(), 128);
        // Leading zero is rejected.
        let mut r = Reader::new(&[0x80, 0x3F], TAG);
        assert_eq!(r.base128(), Err(ParseError::InvalidSfnt));
        // Longer than five bytes is rejected.
        let mut r = Reader::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01], TAG);
        assert_eq!(r.base128(), Err(ParseError::InvalidSfnt));
        // Value exceeding u32::MAX is rejected (accum overflow guard).
        let mut r = Reader::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x7F], TAG);
        assert_eq!(r.base128(), Err(ParseError::InvalidSfnt));
        // Truncation surfaces as TruncatedTable.
        let mut r = Reader::new(&[0x81], TAG);
        assert!(matches!(
            r.base128(),
            Err(ParseError::TruncatedTable { .. })
        ));
        // Known u32::MAX encoding: 0x8F 0xFF 0xFF 0xFF 0x7F.
        let mut r = Reader::new(&[0x8F, 0xFF, 0xFF, 0xFF, 0x7F], TAG);
        assert_eq!(r.base128().unwrap(), u32::MAX);
    }

    #[cfg(feature = "woff2")]
    #[test]
    fn u255_all_encodings() {
        // Single-byte small value.
        let mut r = Reader::new(&[252], TAG);
        assert_eq!(r.u255().unwrap(), 252);
        // 253 word code.
        let mut r = Reader::new(&[253, 1, 250], TAG);
        assert_eq!(r.u255().unwrap(), 506);
        // 255 code: value + 253.
        let mut r = Reader::new(&[255, 253], TAG);
        assert_eq!(r.u255().unwrap(), 506);
        // 254 code: value + 506.
        let mut r = Reader::new(&[254, 0], TAG);
        assert_eq!(r.u255().unwrap(), 506);
        // 65535: word code.
        let mut r = Reader::new(&[253, 0xFF, 0xFF], TAG);
        assert_eq!(r.u255().unwrap(), 65535);
        // Truncated.
        let mut r = Reader::new(&[253, 1], TAG);
        assert!(r.u255().is_err());
    }

    #[cfg(feature = "woff2")]
    #[test]
    fn signed_reads_and_tags() {
        let mut r = Reader::new(&[0xFF, 0xFE, 0x40, 0x00, 0x00, 0x00, 0x07], TAG);
        assert_eq!(r.i16().unwrap(), -2);
        assert_eq!(r.i32().unwrap(), 0x4000_0000);
        assert_eq!(r.i8().unwrap(), 7);
        let mut r = Reader::new(&[0xC0, 0x00, 0x00, 0x00], TAG);
        assert!((r.f2dot14().unwrap().to_f32() - -1.0).abs() < 1e-6);
        // tag_bytes.
        let mut r = Reader::new(b"cmap", TAG);
        assert_eq!(r.tag_bytes().unwrap(), *b"cmap");
    }

    #[test]
    fn within_and_seek() {
        let data = [0, 1, 2, 3, 4, 5];
        let mut r = Reader::within(&data, 2, 5, TAG);
        assert_eq!(r.pos(), 0);
        assert_eq!(r.u16().unwrap(), 0x0203);
        r.seek(0).unwrap();
        assert_eq!(r.u16().unwrap(), 0x0203);
        assert!(r.seek(4).is_err());
        r.skip(1).unwrap();
        assert!(r.skip(1).is_err());
        // Out-of-range window clamps.
        let mut r = Reader::within(&data, 0, 99, TAG);
        assert!(r.seek(6).is_ok());
        assert!(r.seek(7).is_err());
    }
}
