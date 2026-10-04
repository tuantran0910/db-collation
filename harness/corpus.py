"""Scenario-class corpus generators.

Each generated string is tagged with a scenario class so mismatches can be
attributed to the *kind* of input rather than to raw pairs. Classes cover the
algorithmic dimensions of UCA/MySQL/PostgreSQL collation: equality, case, accents,
canonical reordering, ignorables, expansions, contractions, numeric, strengths,
padding, scripts, emoji, supplementary/new code points and long prefixes.
"""

import random

RANGES = [
    (0x105C0, 0x105D0),
    (0x10D40, 0x10D50),
    (0x16100, 0x16110),
    (0x11DB0, 0x11DC0),
    (0x16EA0, 0x16EB0),
    (0x1E6C0, 0x1E6D0),
    (0x1E4D0, 0x1E4E0),
]

ALPHABETS = [
    list("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 _-.:"),
    list("aàáâäãåāăąæçćčďđėęěëèéêfğhıiíîïjklłmnńňoóôöõøőpqrřsśšßtťuúûüůűvwxyýÿzźżž"),
    list("αβγδεζηθικλμνξοπρστυφχψωΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ"),
    list("仟仠仡仢代令以仦仧仨仩仪仫们仭仮仯仰仱仲仳仴仵件价任仸仹仺任仼份仾仿"),
    list("ا ب ت ث ج ح خ د ذ ر ز س ش ص ض ط ظ ع غ"),
    list("あいうえおかきくけこさしすせそたちつてとなにぬねの"),
    ["😀", "😁", "😂", "🤣", "😃", "😄", "😅", "a", "A", "1", " ", "\u0301", "\u0308"],
]


def generate(seed: int = 20261003, scale: float = 1.0) -> list:
    from .model import Scenario

    rows = []

    def add(cat, s):
        rows.append((cat, s))

    add("empty", "")
    for s in [
        " ",
        "  ",
        "   ",
        "a ",
        "a  ",
        " a",
        "a b",
        "a  b",
        "a b ",
        "a\t",
        "a\t ",
        "\u00a0",
        "a\u00a0",
        " a ",
        "   a   ",
    ]:
        add("space_pad", s)

    for cp in [0x01, 0x02, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x1F, 0x7F]:
        add("control", chr(cp))
    add("control", "a\tb")
    add("control", "a\x01b")

    for i in range(32, 127):
        add("ascii_single", chr(i))
    for a in "aAbBcC01":
        for b in "aA0 ":
            add("ascii_combo", a + b)

    for s in [
        "a",
        "A",
        "ß",
        "ẞ",
        "ss",
        "SS",
        "İ",
        "i",
        "I",
        "ı",
        "ﬁ",
        "ǅ",
        "ǆ",
        "Ǆ",
        "Σ",
        "σ",
        "ς",
        "Å",
        "å",
        "Å",
    ]:
        add("case", s)

    for p, q in [
        ("é", "e\u0301"),
        ("É", "E\u0301"),
        ("â", "a\u0302"),
        ("ñ", "n\u0303"),
        ("ü", "u\u0308"),
        ("ç", "c\u0327"),
        ("ő", "o\u030b"),
    ]:
        add("accent_pre", p)
        add("accent_dec", q)

    for s in [
        "a\u0315\u0300",
        "a\u0300\u0315",
        "q\u0307\u0323",
        "q\u0323\u0307",
        "e\u0301\u0327",
        "e\u0327\u0301",
        "o\u0308\u0304",
        "o\u0304\u0308",
    ]:
        add("canon_reorder", s)
    for s in [
        "x" + "\u0300\u0301\u0302\u0303\u0304\u0305\u0306\u0307\u0308",
        "x" + "\u0308\u0307\u0306\u0305\u0304\u0303\u0302\u0301\u0300",
    ]:
        add("many_marks", s)

    for cp in [
        "\u00ad",
        "\u200b",
        "\u200c",
        "\u200d",
        "\u2060",
        "\ufeff",
        "\ufe00",
        "\ufe0f",
        "\u034f",
        "\u180b",
    ]:
        add("ignorable", cp)
        add("ignorable", "a" + cp)
        add("ignorable", "a" + cp + "b")

    for s in [
        "ﬁ",
        "fl",
        "ﬂ",
        "①",
        "1",
        "㍿",
        "½",
        "1/2",
        "Ⅻ",
        "XII",
        "ﬀ",
        "ff",
        "ﬃ",
        "ffi",
        "æ",
        "ae",
        "Æ",
        "œ",
        "oe",
        "Œ",
        "ǳ",
        "dz",
        "ǆ",
        "dž",
        "ǅ",
        "ß",
        "ss",
        "ﬅ",
        "st",
        "ｱ",
        "ア",
        "㍉",
        "ﾐﾘ",
    ]:
        add("compat_expand", s)

    for s in [
        "ch",
        "cH",
        "Ch",
        "CH",
        "ll",
        "lL",
        "Ll",
        "LL",
        "dz",
        "dž",
        "ǆ",
        "cs",
        "zs",
        "gy",
        "ny",
        "ty",
        "aa",
        "å",
    ]:
        add("contraction", s)

    # Discontiguous contractions (UTS #10): a contraction may match across
    # intervening non-starters whose combining class is lower than the mark that
    # completes it, and this must reproduce Oracle exactly. The blocking cases
    # keep the mark as separate so the two strings differ.
    for s in [
        "\u0438\u0591\u0306a",  # и + U+0591(220) + breve(230) + a
        "\u0418\u0591\u0306a",  # uppercase
        "\u0627\u0591\u0653a",  # Arabic alef + U+0591 + madda
        "\u0627\u0591\u0654a",  # Arabic alef + U+0591 + hamza
        "\u0648\u0591\u0654a",  # Arabic waw + U+0591 + hamza
        "\u0438\u0061\u0306",  # starter blocks -> no match
        "\u0438\u0308\u0306",  # equal/higher class blocks
        "\u0438\u034f\u0306",  # CGJ blocks
        # A consumed mark must not participate in a second contraction (Tibetan
        # repeated non-starters): Oracle emits the first contraction followed by
        # the remaining mark.
        "\u0f71\u0f71\u0f72",
        "\u0f71\u0f72\u0f72",
        "\u0fb2\u0fb2\u0f72\u0f71\u0f71",
        "\u0fb2\u0fb2\u0f71\u0f72\u0f72",
    ]:
        add("discontiguous", s)

    for s in [
        "0",
        "1",
        "2",
        "9",
        "10",
        "02",
        "001",
        "1.5",
        "1,5",
        "-1",
        "+1",
        "1a",
        "a1",
        "1 2",
        "10 2",
        "2 10",
        "1.10",
        "1.2",
    ]:
        add("numeric", s)

    for s in [
        "-",
        "_",
        ".",
        ",",
        "!",
        "?",
        "@",
        "#",
        "$",
        "€",
        "¥",
        "+",
        "<",
        ">",
        "=",
        "↔",
        "→",
        "©",
        "®",
        "™",
        "°",
        "%",
        "&",
        "*",
        "(",
        ")",
        "[",
        "]",
        "{",
        "}",
        "a-b",
        "a_b",
        "a.b",
        "a b",
    ]:
        add("punct", s)

    for s in [
        "😀",
        "😃",
        "👍",
        "👍🏽",
        "👨\u200d👩\u200d👧",
        "🇺🇸",
        "1\ufe0f\u20e3",
        "☀",
        "☀\ufe0f",
        "❤",
        "❤\ufe0f",
        "❤\u200d🔥",
        "©\ufe0f",
        "🏳\ufe0f\u200d🌈",
    ]:
        add("emoji", s)

    for s in [
        "あ",
        "ア",
        "ｱ",
        "ぁ",
        "ぃ",
        "中",
        "漢",
        "㍿",
        "𠀀",
        "い",
        "ろ",
        "は",
        "α",
        "Α",
        "ά",
        "ἀ",
        "я",
        "Я",
        "ё",
        "е",
        "ا",
        "أ",
        "إ",
        "ب",
        "ک",
        "ك",
        "א",
        "ב",
        "אָ",
        "ก",
        "ข",
        "ก้",
        "ि",
        "ी",
        "क",
        "कि",
    ]:
        add("script", s)

    for s in [
        "ا\u064e",
        "ا\u0650",
        "ا\u064f",
        "ب\u0651",
        "ب\u0651\u064e",
        "ل\u0627",
        "ب",
        "ب\u0627",
        "\u0627\u0644\u0644\u0647",
    ]:
        add("rtl_diacritic", s)

    for lo, hi in RANGES:
        for cp in range(lo, hi):
            add("new_unicode", chr(cp))
    for s in [
        "\U00010000",
        "\U0001d11e",
        "\U0001f600",
        "\U000e0001",
        "\U000e0100",
        "\U000e0002",
        "\U000e0020",
        "\U000e01ef",
    ]:
        add("supplementary", s)

    # Implicit-weight range endpoints and neighbours (beyond the copied table),
    # including the CJK-Ext-E endpoint U+2CEA1 witness.
    for cp in [
        0x2CEA0,
        0x2CEA1,
        0x2CEA2,
        0x0378,
        0x3400,
        0x4DB5,
        0x4E00,
        0x9FD5,
        0x20000,
        0x2A6D6,
        0x2A700,
        0x2B740,
        0x2B820,
        0x17000,
        0x18AFF,
        0xAC00,
        0xD7A3,
        0x1100,
        0x1161,
        0x11A7,
    ]:
        add("implicit_boundary", chr(cp))

    for n, tail in [(300, "a"), (300, "b"), (299, "z"), (128, "x")]:
        add("long_prefix", "p" * n + tail)
    add("long_prefix", "p" * 299 + "b")
    add("long_prefix", "p" * 300)

    rng = random.Random(seed)
    for _ in range(int(700 * scale)):
        alpha = rng.choice(ALPHABETS)
        n = rng.randint(0, 8)
        add("random_mixed", "".join(rng.choice(alpha) for _ in range(n)))
    for _ in range(int(300 * scale)):
        alpha = rng.choice(ALPHABETS)
        add("random_short", "".join(rng.choice(alpha) for _ in range(rng.randint(1, 3))))

    for base in ["", "a", "ab", "z"]:
        for suf in ["\u0001", "\t", "\u000b", " ", "\u00a0", "a", "\u0301"]:
            add("pad_edge", base + suf)

    seen = set()
    out = []
    for cat, s in rows:
        if (cat, s) in seen:
            continue
        seen.add((cat, s))
        out.append(Scenario(len(out), cat, s))
    return out


def category_counts(corpus) -> dict:
    c = {}
    for sc in corpus:
        c[sc.cat] = c.get(sc.cat, 0) + 1
    return c
