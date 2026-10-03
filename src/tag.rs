//! Known SFNT table tags and the [`TableTag`] lookup enum.

use crate::error::{ParseError, RawTag};
use core::fmt;

/// Re-exported from `font-types`: a validated four-byte font tag.
pub use font_types::Tag;

/// The subset of SFNT table tags this parser knows by name.
///
/// Typed accessors on [`crate::FontRef`] accept this enum; arbitrary
/// (unknown) tags are reachable through [`crate::FontRef::table_by_tag`] with
/// a raw [`Tag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[doc(alias = "tag")]
pub enum TableTag {
    /// Character-to-glyph mapping.
    Cmap,
    /// Font header.
    Head,
    /// Horizontal header.
    Hhea,
    /// Horizontal metrics.
    Hmtx,
    /// Maximum profile.
    Maxp,
    /// Naming table.
    Name,
    /// OS/2 — Windows metrics.
    Os2,
    /// PostScript table.
    Post,
    /// Vertical header.
    Vhea,
    /// Vertical metrics.
    Vmtx,
    /// Control value table.
    Cvt,
    /// Font program.
    Fpgm,
    /// CVT program.
    Prep,
    /// Glyph data (TrueType outlines).
    Glyf,
    /// Index to location.
    Loca,
    /// Compact Font Format (CFF) outlines.
    Cff,
    /// Font variations — axis data.
    Fvar,
    /// Font variations — axis segment maps.
    Avar,
    /// Font variations — glyph deltas.
    Gvar,
    /// Glyph substitution.
    Gsub,
    /// Glyph positioning.
    Gpos,
    /// Glyph definition.
    Gdef,
    /// Style attributes.
    Stat,
    /// Digital signature.
    Dsig,
    /// Baseline table.
    Base,
    /// Kerning.
    Kern,
}

impl TableTag {
    /// Every tag this parser names, in declaration order.
    pub const ALL: [TableTag; 26] = [
        TableTag::Cmap,
        TableTag::Head,
        TableTag::Hhea,
        TableTag::Hmtx,
        TableTag::Maxp,
        TableTag::Name,
        TableTag::Os2,
        TableTag::Post,
        TableTag::Vhea,
        TableTag::Vmtx,
        TableTag::Cvt,
        TableTag::Fpgm,
        TableTag::Prep,
        TableTag::Glyf,
        TableTag::Loca,
        TableTag::Cff,
        TableTag::Fvar,
        TableTag::Avar,
        TableTag::Gvar,
        TableTag::Gsub,
        TableTag::Gpos,
        TableTag::Gdef,
        TableTag::Stat,
        TableTag::Dsig,
        TableTag::Base,
        TableTag::Kern,
    ];

    /// The raw four-byte representation.
    #[must_use]
    pub const fn to_bytes(self) -> RawTag {
        match self {
            TableTag::Cmap => *b"cmap",
            TableTag::Head => *b"head",
            TableTag::Hhea => *b"hhea",
            TableTag::Hmtx => *b"hmtx",
            TableTag::Maxp => *b"maxp",
            TableTag::Name => *b"name",
            TableTag::Os2 => *b"OS/2",
            TableTag::Post => *b"post",
            TableTag::Vhea => *b"vhea",
            TableTag::Vmtx => *b"vmtx",
            TableTag::Cvt => *b"cvt ",
            TableTag::Fpgm => *b"fpgm",
            TableTag::Prep => *b"prep",
            TableTag::Glyf => *b"glyf",
            TableTag::Loca => *b"loca",
            TableTag::Cff => *b"CFF ",
            TableTag::Fvar => *b"fvar",
            TableTag::Avar => *b"avar",
            TableTag::Gvar => *b"gvar",
            TableTag::Gsub => *b"GSUB",
            TableTag::Gpos => *b"GPOS",
            TableTag::Gdef => *b"GDEF",
            TableTag::Stat => *b"stat",
            TableTag::Dsig => *b"DSIG",
            TableTag::Base => *b"BASE",
            TableTag::Kern => *b"kern",
        }
    }

    /// Look up a known tag by its raw bytes.
    ///
    /// # Errors
    /// [`ParseError::UnknownTableTag`] when the bytes do not name a tag in
    /// [`TableTag::ALL`].
    pub fn from_bytes(tag: &RawTag) -> Result<Self, ParseError> {
        Self::ALL
            .iter()
            .find(|t| t.to_bytes() == *tag)
            .copied()
            .ok_or(ParseError::UnknownTableTag(*tag))
    }

    /// The [`font_types::Tag`] for this table.
    #[must_use]
    pub const fn to_tag(self) -> Tag {
        Tag::new(&self.to_bytes())
    }
}

impl From<TableTag> for Tag {
    fn from(t: TableTag) -> Tag {
        t.to_tag()
    }
}

impl fmt::Display for TableTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", crate::error::tag_display(&self.to_bytes()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::TableTag;
    use alloc::string::ToString;

    // Test harness: assertions legitimately panic; the lib target stays
    // lint-clean.

    #[test]
    fn round_trip_all_tags() {
        for tag in TableTag::ALL {
            let bytes = tag.to_bytes();
            assert_eq!(TableTag::from_bytes(&bytes), Ok(tag), "{tag}");
            // Every known tag is four printable-or-space bytes.
            assert!(bytes.iter().all(|&b| b.is_ascii_graphic() || b == b' '));
        }
        assert_eq!(
            TableTag::from_bytes(b"nope"),
            Err(crate::ParseError::UnknownTableTag(*b"nope"))
        );
        assert_eq!(TableTag::Cmap.to_string(), "'cmap'");
        assert_eq!(TableTag::Os2.to_string(), "'OS/2'");
        assert_eq!(
            font_types::Tag::from(TableTag::Stat).to_be_bytes(),
            *b"stat"
        );
    }
}
