# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
Semantic Versioning.

## [0.1.0] — 2026-10-03

### Added
- `FontRef` — zero-copy SFNT/TrueType/OTF view: offset table + table
  directory validation, lifetime-bound raw table access (`table`,
  `table_by_tag`, `table_directory`), typed accessors (`head`, `hhea`,
  `hmtx`, `maxp`, `cmap`, `name`, `OS/2`, `post`, `stat`, `DSIG`, `layout`,
  `variable`), glyph access (`num_glyphs`, `glyph_data`, `glyph`, `outline`
  with recursive composite resolution, `advance_width`, `gvar_deltas`).
- `cmap` formats 0, 4, 6, 12 with binary-search segment/group lookup and
  spec-preference subtable selection.
- `glyf`/`loca`: simple glyph point decoding (all flag forms incl. repeat),
  composite components with 2x2 transforms, bounded outline resolution.
- `fvar`/`avar`/`gvar`: axes, named instances, segment maps with
  piecewise-linear mapping, packed point/delta decoding (shared + private
  point numbers, embedded/shared peaks, intermediate regions).
- `GSUB`/`GPOS` common structure: script/langsys/feature/lookup lists.
- `stat` design axes + axis-value formats 1–4; `DSIG` header + records.
- WOFF 1.0 container decoding (feature `woff`).
- WOFF 2.0 container decoding (feature `woff2`): Brotli stream, transformed
  glyf/loca/hmtx reversal, UIntBase128/255UInt16, checksum + checkSumAdjustment
  recomputation, head transform marker.
- TTC collections: shared-table `FontRef`s and standalone extraction.
- `ParseError` — typed, exhaustive; totality enforced by deny lints, a
  cargo-fuzz target, and 500-case proptests; coverage ≥ 90% on parse code.
