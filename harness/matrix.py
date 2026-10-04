"""Matrix of configurations and which ones the crate claims exact support for."""

from .model import Spec

# PostgreSQL ICU configs exercised by the matching-ICU candidate.
PG_SPECS = [
    Spec("pg-und-det", "postgres", "und", True),
    Spec("pg-en", "postgres", "en", True),
    Spec("pg-sv", "postgres", "sv", True),
    Spec("pg-tr", "postgres", "tr", True),
    Spec("pg-phonebk", "postgres", "de-u-co-phonebk", True),
    Spec("pg-kn", "postgres", "und-u-kn-true", True),
    Spec("pg-kf-upper", "postgres", "und-u-kf-upper", True),
    Spec("pg-ks2", "postgres", "und-u-ks-level2", True),
    Spec("pg-ks3", "postgres", "und-u-ks-level3", True),
    Spec("pg-ks4", "postgres", "und-u-ks-level4", True),
    Spec("pg-kv-punct", "postgres", "und-u-kv-punct", True),
    Spec("pg-kv-space", "postgres", "und-u-kv-space", True),
    Spec("pg-kb", "postgres", "fr-u-kb-true", True),
    Spec("pg-und-nondet", "postgres", "und", False),
    # Custom rules are part of the collation identity and are not reproducible;
    # the crate must refuse this even though the version matches.
    Spec("pg-custom-rules", "postgres", "und", False, supported=False, rules="& z < a"),
]

# MySQL collations the *crate* models exactly. `utf8mb4_0900_bin` is exact
# because NO PAD + code-point order == bytewise order for valid UTF-8 (it is the
# binary backend). `utf8mb4_bin` is PAD SPACE and is deliberately NOT included:
# it is asserted unsupported below so the crate must refuse it.
MYSQL_SUPPORTED = ["utf8mb4_0900_ai_ci", "utf8mb4_unicode_ci", "utf8mb4_0900_bin"]
MYSQL_UNSUPPORTED = [
    "utf8mb4_bin",
    "utf8mb4_0900_as_ci",
    "utf8mb4_0900_as_cs",
    "utf8mb4_unicode_520_ci",
    "utf8mb4_general_ci",
    "utf8mb4_cs_0900_ai_ci",
    "utf8mb4_zh_0900_as_cs",
]

MYSQL_SPECS = [Spec("my-" + c, "mysql", c, True, supported=True) for c in MYSQL_SUPPORTED] + [
    Spec("my-" + c, "mysql", c, True, supported=False) for c in MYSQL_UNSUPPORTED
]

PG_IMAGES = [
    "postgres:14",  # trixie / ICU 76
    "postgres:15",  # trixie / ICU 76
    "postgres:15-bookworm",  # ICU 72
    "postgres:16",  # trixie / ICU 76
    "postgres:17",  # trixie / ICU 76
    "postgres:18",  # trixie / ICU 76
]

# `postgres:15-bullseye` (Debian 11, ICU 67) is deliberately excluded: bullseye
# is EOL, its security packages are purged from deb.debian.org, and the pgdg
# repo is retired, so the candidate image cannot be built from live mirrors. To
# restore it, build the candidate against a pinned `debian/snapshot` builder
# base (https://snapshot.debian.org/) — add the image to PG_BUILDER_IMAGE below.
# The algorithm is ICU-version parametric and ICU 72/76 already exercise the
# version-difference path, so ICU 67 is low marginal value.
#
# Oracle images whose own apt is retired: map the image to a builder base that
# still installs the same ICU major, plus the Debian suite to repair sources
# with. The runtime base stays the untouched oracle image. Example:
#   {"postgres:15-bullseye": ("debian/snapshot:bullseye", "bullseye")}
PG_BUILDER_IMAGE: dict[str, tuple[str, str]] = {}
MYSQL_IMAGES = ["mysql:8.0", "mysql:8.4", "mysql:9.4"]
