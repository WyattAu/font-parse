#!/usr/bin/env python3
"""Deterministic fixture generator for font-parse's known-good test suite.

Regenerate with:
    python3 tests/fixtures/generate_fonts.py

Outputs (all under tests/fixtures/):
    minimal.ttf   — 4-glyph TrueType font (simple + composite outlines,
                    cmap formats 0/4/12, full name/OS-2/post tables)
    minimal.woff  — WOFF 1.0 packaging of minimal.ttf
    minimal.woff2 — WOFF 2.0 packaging (brotli + glyf/loca/hmtx transforms)
    collection.ttc — TTC with two fonts sharing glyph data
    variable.ttf  — variable font (fvar wght 400..900, avar map, gvar deltas)

The known-good test asserts specific glyph ids, advances, cmap lookups and
outline points against the values printed at the end of this script.
"""

import os

from fontTools.designspaceLib import DesignSpaceDocument
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTCollection, TTFont, newTable

HERE = os.path.dirname(os.path.abspath(__file__))

UPEM = 1000
FAMILY = "FontParse Test"
STYLE = "Regular"
PS_NAME = "FontParseTest-Regular"

# glyph order: gid 0=.notdef, 1=A, 2=B, 3=Ccomp (composite of A)
GLYPHS = [".notdef", "A", "B", "Ccomp"]
UNICODE_MAP = {0x41: "A", 0x42: "B", 0x43: "Ccomp"}

ADVANCES = {".notdef": 500, "A": 600, "B": 550, "Ccomp": 600}
LSBS = {".notdef": 50, "A": 40, "B": 30, "Ccomp": 20}


def box(x0, y0, x1, y1):
    pen = TTGlyphPen(None)
    pen.moveTo((x0, y0))
    pen.lineTo((x0, y1))
    pen.lineTo((x1, y1))
    pen.lineTo((x1, y0))
    pen.closePath()
    return pen.glyph()


def draw_glyphs(a_x0=100):
    glyphs = {
        ".notdef": box(50, 0, 450, 700),
        "A": box(a_x0, 100, 500, 800),
        "B": box(50, 0, 500, 750),
    }
    return glyphs


def set_common_tables(fb):
    head = fb.font["head"]
    head.macStyle = 0
    head.unitsPerEm = UPEM

    os2 = fb.font["OS/2"]
    os2.version = 2
    os2.usWeightClass = 700
    os2.usWidthClass = 5
    os2.fsSelection = 0x40  # bold
    os2.sTypoAscender = 800
    os2.sTypoDescender = -200
    os2.sTypoLineGap = 50
    os2.usWinAscent = 850
    os2.usWinDescent = 250
    os2.sxHeight = 500
    os2.sCapHeight = 700
    os2.sFamilyClass = 0x0401
    from fontTools.ttLib.tables.O_S_2f_2 import Panose
    panose = Panose()
    panose.bFamilyType, panose.bSerifStyle, panose.bWeight, panose.bProportion = 1, 2, 3, 4
    panose.bContrast, panose.bStrokeVariation, panose.bArmStyle, panose.bLetterForm = 5, 6, 7, 8
    panose.bMidline, panose.bXHeight = 9, 10
    os2.panose = panose
    os2.ulUnicodeRange1 = 0x00000003
    os2.ulCodePageRange1 = 0x00000001
    os2.achVendID = "TST1"

    post = fb.font["post"]
    post.formatType = 2.0
    post.italicAngle = -12.5
    post.underlinePosition = -50
    post.underlineThickness = 10
    post.isFixedPitch = 1

    hhea = fb.font["hhea"]
    hhea.ascender = 800
    hhea.descender = -200
    hhea.lineGap = 50
    hhea.numberOfHMetrics = 3


NAMES = {
    "familyName": FAMILY,
    "styleName": STYLE,
    "fullName": FAMILY + " " + STYLE,
    "psName": PS_NAME,
    "version": "Version 0.1",
    "copyright": "Copyright (c) 2026 WyattAu",
    "trademark": "font-parse is a trademark of nobody",
    "manufacturer": "WyattAu",
    "designer": "WyattAu",
    "description": "font-parse known-good fixture font",
    "vendorURL": "https://github.com/WyattAu/font-parse",
    
    "licenseDescription": "This font is in the public domain.",
    "licenseInfoURL": "https://opensource.org/licenses/MIT",
}

OS2_KW = dict(sTypoAscender=800, sTypoDescender=-200, sTypoLineGap=50,
              usWinAscent=850, usWinDescent=250)


def add_cmap_subtables(font):
    from fontTools.ttLib.tables._c_m_a_p import CmapSubtable
    tables = []

    def mk(fmt, platform, encoding):
        sub = CmapSubtable.getSubtableClass(fmt)(fmt)
        sub.platformID, sub.platEncID, sub.language = platform, encoding, 0
        return sub

    sub31 = mk(4, 3, 1)
    sub31.cmap = UNICODE_MAP
    sub03 = mk(4, 0, 3)
    sub03.cmap = UNICODE_MAP
    sub12 = mk(12, 3, 10)
    sub12.cmap = UNICODE_MAP
    subf0 = mk(0, 1, 0)
    subf0.cmap = dict(UNICODE_MAP)
    font["cmap"].tableVersion = 0
    font["cmap"].tables = [sub31, sub03, sub12, subf0]


def build_master(a_x0, name_ids=None):
    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder(GLYPHS)
    fb.setupCharacterMap(UNICODE_MAP)
    glyphs = draw_glyphs(a_x0)
    pen = TTGlyphPen({"A": glyphs["A"]})
    pen.addComponent("A", (0.5, 0, 0, 0.5, 40, 60))
    glyphs["Ccomp"] = pen.glyph()
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({n: (ADVANCES[n], LSBS[n]) for n in GLYPHS})
    fb.setupHorizontalHeader(ascent=800, descent=-200, lineGap=50)
    fb.setupNameTable(dict(NAMES))
    fb.setupOS2(**OS2_KW)
    fb.setupPost()
    set_common_tables(fb)
    add_cmap_subtables(fb.font)
    if name_ids:
        for nid, value in name_ids.items():
            fb.font["name"].setName(value, nid, 3, 1, 0x409)
    return fb


def main():
    ttf_path = os.path.join(HERE, "minimal.ttf")
    build_master(100).save(ttf_path)

    woff = TTFont(ttf_path)
    woff.flavor = "woff"
    woff.save(os.path.join(HERE, "minimal.woff"))

    woff2 = TTFont(ttf_path)
    woff2.flavor = "woff2"
    woff2.save(os.path.join(HERE, "minimal.woff2"))

    coll = TTCollection()
    coll.fonts = [TTFont(ttf_path), build_master(150, {1: "FontParse Alt", 6: "FontParseAlt-Regular"}).font]
    coll.save(os.path.join(HERE, "collection.ttc"))

    # Variable font from two masters via a designspace.
    from fontTools.designspaceLib import AxisDescriptor
    ds = DesignSpaceDocument()
    axis = AxisDescriptor()
    axis.name = "Weight"
    axis.tag = "wght"
    axis.minimum = 400
    axis.default = 400
    axis.maximum = 900
    ds.addAxis(axis)
    master_paths = []
    from fontTools.designspaceLib import SourceDescriptor, InstanceDescriptor
    for i, x0 in [(0, 100), (1, 150)]:
        p = os.path.join(HERE, f"_tmp_master{i}.ttf")
        build_master(x0).save(p)
        master_paths.append(p)
        src = SourceDescriptor()
        src.path = p
        src.name = f"master-{i}"
        src.location = {"Weight": 400 if i == 0 else 900}
        ds.addSource(src)
    inst = InstanceDescriptor()
    inst.name = "Medium"
    inst.styleName = "Medium"
    inst.familyName = FAMILY
    inst.postScriptFontName = "FontParseTest-Medium"
    inst.location = {"Weight": 600}
    ds.addInstance(inst)
    ds_path = os.path.join(HERE, "_tmp_designspace.designspace")
    ds.write(ds_path)

    from fontTools import varLib
    vf_path = os.path.join(HERE, "variable.ttf")
    vf_font, *_ = varLib.build(ds_path)
    vf_font.save(vf_path)

    vf = TTFont(vf_path)
    avar = newTable("avar")
    avar.segments = {"wght": {-1.0: -1.0, 0.0: 0.0, 0.5: 0.6, 1.0: 1.0}}
    vf["avar"] = avar
    vf.save(vf_path)

    for p in master_paths + [ds_path]:
        if os.path.exists(p):
            os.remove(p)

    print("fixtures written:")
    for f in ["minimal.ttf", "minimal.woff", "minimal.woff2", "collection.ttc", "variable.ttf"]:
        p = os.path.join(HERE, f)
        print(f"  {f}: {os.path.getsize(p)} bytes")

    # Reference data for the Rust assertions.
    f = TTFont(ttf_path)
    glyf = f["glyf"]
    a = glyf["A"]
    coords, ends, flags = a.getCoordinates(glyf)
    print("\nA endPtsOfContours:", list(a.getCoordinates(glyf)[1]))
    print("A flags:", list(flags))
    print("A coordinates:", [(int(x), int(y)) for x, y in coords])
    c = glyf["Ccomp"]
    print("Ccomp components:", [(com.glyphName, com.x, com.y, list(com.transform)) for com in c.components])
    print("hmtx:", {k: v for k, v in f["hmtx"].metrics.items()})
    print("post:", f["post"].italicAngle, f["post"].underlinePosition, f["post"].underlineThickness, f["post"].isFixedPitch)
    print("post format:", f["post"].formatType, "extraNames:", getattr(f["post"], "extraNames", None), "glyphOrder?", f["post"].glyphOrder if hasattr(f["post"], "glyphOrder") else None)
    print("OS/2 w/w:", f["OS/2"].usWeightClass, f["OS/2"].usWidthClass)
    print("maxp:", f["maxp"].numGlyphs)
    print("head:", f["head"].indexToLocFormat, f["head"].unitsPerEm, f["head"].macStyle)
    print("hhea numberOfHMetrics:", f["hhea"].numberOfHMetrics)
    print("name 1/2/4/6:", [f["name"].getDebugName(i) for i in (1, 2, 4, 6)])

    vf2 = TTFont(vf_path)
    print("fvar:", [(a.axisTag, a.minValue, a.defaultValue, a.maxValue) for a in vf2["fvar"].axes])
    print("instances:", [(inst.subfamilyNameID, dict(inst.coordinates)) for inst in vf2["fvar"].instances])
    print("avar:", vf2["avar"].segments)
    if "gvar" in vf2:
        g = vf2["gvar"]
        for name in GLYPHS:
            for v in g.variations.get(name, []):
                print(f"gvar {name}: axes={v.axes} coord={v.coordinates} all-deltas={v.coordinates is not None and v.getUsedPoints()}")
                print(f"  points={v.points} deltas_x={v.data[:0]}") if False else None


if __name__ == "__main__":
    main()
