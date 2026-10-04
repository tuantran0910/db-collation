// Copyright 2026 db-collation contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The closed set of collations this crate reproduces exactly.
//!
//! A [`Collation`] is a *resolved description*: the caller reads the source
//! database's catalog/metadata (or knows it statically) and passes the result
//! in. This crate never performs that discovery itself.

use core::cmp::Ordering;
use core::fmt;

use crate::error::{Error, Result};

/// A provider-specific collation data version token.
///
/// For `PostgreSQL` ICU this is the `collversion` reported by
/// `pg_collation_actual_version(oid)`, e.g. `153.128`. For `MySQL` it is the
/// UCA version implied by the collation name, e.g. `9.0.0`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VersionId(String);

impl VersionId {
    /// Construct a version token from anything string-like.
    #[must_use]
    pub fn new(v: impl Into<String>) -> Self {
        Self(v.into())
    }

    /// The token as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for VersionId {
    fn from(v: String) -> Self {
        Self(v)
    }
}

impl From<&str> for VersionId {
    fn from(v: &str) -> Self {
        Self(v.to_owned())
    }
}

impl fmt::Display for VersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `PostgreSQL` collation provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PostgresProvider {
    /// ICU provider (`collprovider = 'i'`).
    Icu,
    /// libc provider (`collprovider = 'c'`). Only `C`/`POSIX` are supported and
    /// they are bytewise.
    Libc,
    /// The `C`/`POSIX` locale under any provider: bytewise ordering.
    C,
}

/// A resolved `PostgreSQL` collation description.
///
/// Fields are private; use the constructors and [`PostgresCollation::with_rules`]
/// / [`PostgresCollation::with_encoding`] to build one. `rules` and `encoding`
/// are part of the identity: a custom-rule collation or a non-UTF-8 source is
/// not reproducible by this crate and is refused at construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct PostgresCollation {
    provider: PostgresProvider,
    locale: String,
    deterministic: bool,
    version: VersionId,
    rules: Option<String>,
    encoding: String,
}

impl PostgresCollation {
    /// Bytewise `C`/`POSIX` collation.
    ///
    /// The comparison is bytewise, but the library only supports UTF-8 sources,
    /// so a non-UTF-8 source is still refused at construction (the `encoding`
    /// field defaults to `UTF8`; use [`PostgresCollation::with_encoding`] to
    /// describe another).
    #[must_use]
    pub fn c() -> Self {
        Self {
            provider: PostgresProvider::C,
            locale: String::new(),
            deterministic: true,
            version: VersionId::new("c"),
            rules: None,
            encoding: "UTF8".to_owned(),
        }
    }

    /// A `PostgreSQL` ICU collation description with no custom rules, assuming a
    /// UTF-8 source. Use [`PostgresCollation::with_rules`] /
    /// [`PostgresCollation::with_encoding`] to describe other sources.
    #[must_use]
    pub fn icu(locale: impl Into<String>, deterministic: bool, version: VersionId) -> Self {
        Self {
            provider: PostgresProvider::Icu,
            locale: locale.into(),
            deterministic,
            version,
            rules: None,
            encoding: "UTF8".to_owned(),
        }
    }

    /// A `PostgreSQL` libc collation description. Only `C`/`POSIX` are
    /// reproducible (bytewise); any other libc locale is refused at
    /// construction. Use [`PostgresCollation::c`] for the `C` locale directly.
    #[must_use]
    pub fn libc(locale: impl Into<String>) -> Self {
        Self {
            provider: PostgresProvider::Libc,
            locale: locale.into(),
            deterministic: true,
            version: VersionId::new("libc"),
            rules: None,
            encoding: "UTF8".to_owned(),
        }
    }

    /// Attach custom ICU collation rules (`collicurules`).
    ///
    /// The library refuses **any** value here at construction — including the
    /// empty string — with `Error::Unsupported`. Custom ICU rules are not
    /// reproducible exactly; do not compare such a collation locally.
    #[must_use]
    pub fn with_rules(mut self, rules: impl Into<String>) -> Self {
        self.rules = Some(rules.into());
        self
    }

    /// Set the source encoding (`pg_collation`/database encoding, e.g. `UTF8`,
    /// `LATIN1`).
    #[must_use]
    pub fn with_encoding(mut self, encoding: impl Into<String>) -> Self {
        self.encoding = encoding.into();
        self
    }

    /// The provider.
    #[must_use]
    pub const fn provider(&self) -> PostgresProvider {
        self.provider
    }

    /// ICU locale id (empty for `C`).
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Whether the source collation is deterministic.
    #[must_use]
    pub const fn deterministic(&self) -> bool {
        self.deterministic
    }

    /// The source collation data version (`collversion`).
    #[must_use]
    pub const fn version(&self) -> &VersionId {
        &self.version
    }

    /// Custom ICU collation rules, if any.
    #[must_use]
    pub fn rules(&self) -> Option<&str> {
        self.rules.as_deref()
    }

    /// The source encoding.
    #[must_use]
    pub fn encoding(&self) -> &str {
        &self.encoding
    }
}

/// `MySQL` UCA weight-table version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MysqlUcaVersion {
    /// UCA 4.0.0, used by `utf8mb4_unicode_ci` and friends.
    Uca400,
    /// UCA 9.0.0, used by `utf8mb4_0900_ai_ci` and friends.
    Uca900,
}

impl MysqlUcaVersion {
    /// The canonical version token.
    #[must_use]
    pub fn version_id(self) -> VersionId {
        match self {
            Self::Uca400 => VersionId::new("4.0.0"),
            Self::Uca900 => VersionId::new("9.0.0"),
        }
    }
}

/// Trailing-space comparison semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Pad {
    /// Trailing spaces are significant (NO PAD; the `utf8mb4_0900_*` family).
    NoPad,
    /// Trailing spaces are ignored (PAD SPACE; UCA 4.0.0 collations).
    Space,
}

/// A resolved `MySQL` collation description.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct MysqlCollation {
    /// The collation name, e.g. `utf8mb4_0900_ai_ci`.
    pub name: String,
    /// The UCA version of its weight table.
    pub uca_version: MysqlUcaVersion,
    /// Padding semantics.
    pub pad: Pad,
}

impl MysqlCollation {
    /// Build a `MySQL` UCA collation description from its resolved parts.
    #[must_use]
    pub fn new(name: impl Into<String>, uca_version: MysqlUcaVersion, pad: Pad) -> Self {
        Self {
            name: name.into(),
            uca_version,
            pad,
        }
    }

    /// `utf8mb4_0900_ai_ci`: UCA 9.0.0, accent- and case-insensitive, NO PAD.
    #[must_use]
    pub fn utf8mb4_0900_ai_ci() -> Self {
        Self {
            name: "utf8mb4_0900_ai_ci".to_owned(),
            uca_version: MysqlUcaVersion::Uca900,
            pad: Pad::NoPad,
        }
    }

    /// `utf8mb4_unicode_ci`: UCA 4.0.0, accent- and case-insensitive, PAD SPACE.
    #[must_use]
    pub fn utf8mb4_unicode_ci() -> Self {
        Self {
            name: "utf8mb4_unicode_ci".to_owned(),
            uca_version: MysqlUcaVersion::Uca400,
            pad: Pad::Space,
        }
    }
}

/// `Oracle` collation provider.
///
/// Oracle compares character data under a *named collation* selected by
/// `NLS_SORT`. Only two families are reproducible by this crate: the bytewise
/// `BINARY` collation and the `UCA*_DUCET` Unicode Collation Algorithm
/// collations built from the open DUCET tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum OracleProvider {
    /// Bytewise comparison (`NLS_SORT=BINARY`) — the `Binary` backend.
    Binary,
    /// A `UCA*_DUCET` collation backed by an open DUCET weight table.
    Uca,
}

/// `Oracle` UCA version for a `UCA*_DUCET` collation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum OracleUcaVersion {
    /// `UCA0700_DUCET` — Unicode 7.0.0 DUCET.
    Uca700,
    /// `UCA1210_DUCET` — Unicode 12.1.0 DUCET.
    Uca1210,
}

impl OracleUcaVersion {
    /// The canonical version token.
    #[must_use]
    pub fn version_id(self) -> VersionId {
        match self {
            Self::Uca700 => VersionId::new("7.0.0"),
            Self::Uca1210 => VersionId::new("12.1.0"),
        }
    }

    /// The `Oracle` collation name this version corresponds to.
    #[must_use]
    pub const fn collation_name(self) -> &'static str {
        match self {
            Self::Uca700 => "UCA0700_DUCET",
            Self::Uca1210 => "UCA1210_DUCET",
        }
    }
}

/// A resolved `Oracle` collation description.
///
/// Fields are private; use the constructors. Only `NLS_SORT=BINARY` on an
/// `AL32UTF8` database with `VARCHAR2` semantics, and the `UCA0700_DUCET` /
/// `UCA1210_DUCET` collations, are reproducible. Every other Oracle collation
/// (monolingual, `_M`, `BINARY_CI`/`BINARY_AI`, `*_ROOT`, `*_ORADUCET`, the
/// tailored UCA collations) is refused at construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct OracleCollation {
    provider: OracleProvider,
    /// The resolved `NLS_SORT` name (normalized), e.g. `BINARY`, `UCA1210_DUCET`.
    nls_sort: String,
    uca_version: Option<OracleUcaVersion>,
    // Only AL32UTF8 is modelled. Kept explicit so a wrong source is refused
    // rather than silently compared with the wrong bytes.
    charset: String,
    // `CHAR` is blank-padded; `VARCHAR2` is not. Only the latter is modelled.
    blank_padded: bool,
}

impl OracleCollation {
    /// Bytewise `NLS_SORT=BINARY` on an `AL32UTF8` `VARCHAR2`.
    #[must_use]
    pub fn binary() -> Self {
        Self {
            provider: OracleProvider::Binary,
            nls_sort: "BINARY".to_owned(),
            uca_version: None,
            charset: "AL32UTF8".to_owned(),
            blank_padded: false,
        }
    }

    /// A `UCA*_DUCET` collation on an `AL32UTF8` `VARCHAR2`.
    #[must_use]
    pub fn uca_ducet(version: OracleUcaVersion) -> Self {
        Self {
            provider: OracleProvider::Uca,
            nls_sort: version.collation_name().to_owned(),
            uca_version: Some(version),
            charset: "AL32UTF8".to_owned(),
            blank_padded: false,
        }
    }

    /// Build a description from a raw resolved `NLS_SORT` name and source
    /// charset. The name is normalized (trimmed, upper-cased). Unknown or
    /// unsupported names are refused at [`Collation::oracle`].
    #[must_use]
    pub fn from_nls_sort(nls_sort: impl Into<String>, charset: impl Into<String>) -> Self {
        let name = nls_sort.into().trim().to_ascii_uppercase();
        let (provider, uca_version) = match name.as_str() {
            "UCA0700_DUCET" => (OracleProvider::Uca, Some(OracleUcaVersion::Uca700)),
            "UCA1210_DUCET" => (OracleProvider::Uca, Some(OracleUcaVersion::Uca1210)),
            // `BINARY` and anything else: unsupported names are refused at
            // construction; only `BINARY` is a valid binary-provider name.
            _ => (OracleProvider::Binary, None),
        };
        Self {
            provider,
            nls_sort: name,
            uca_version,
            charset: charset.into(),
            blank_padded: false,
        }
    }

    /// Mark the source as a blank-padded `CHAR`/`NCHAR` column.
    ///
    /// Blank-padded comparison differs from `VARCHAR2` even under the same
    /// collation, so any blank-padded source is refused at construction.
    #[must_use]
    pub fn with_blank_padded(mut self, blank_padded: bool) -> Self {
        self.blank_padded = blank_padded;
        self
    }

    /// The provider.
    #[must_use]
    pub const fn provider(&self) -> OracleProvider {
        self.provider
    }

    /// The resolved `NLS_SORT` name.
    #[must_use]
    pub fn nls_sort(&self) -> &str {
        &self.nls_sort
    }

    /// The UCA version, for `UCA*_DUCET` collations.
    #[must_use]
    pub const fn uca_version(&self) -> Option<OracleUcaVersion> {
        self.uca_version
    }

    /// The source character set (only `AL32UTF8` is modelled).
    #[must_use]
    pub fn charset(&self) -> &str {
        &self.charset
    }

    /// Whether the source is blank-padded (`CHAR`/`NCHAR`).
    #[must_use]
    pub const fn blank_padded(&self) -> bool {
        self.blank_padded
    }
}

/// Which backend a [`Collation`] uses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Backend {
    /// Bytewise comparison (`PostgreSQL` `C`/`POSIX`, `MySQL` `*_bin`).
    Binary,
    /// `PostgreSQL` ICU.
    PostgresIcu(PostgresCollation),
    /// `MySQL` UCA weight tables.
    MysqlUca(MysqlCollation),
    /// `Oracle` (bytewise `BINARY` or `UCA*_DUCET`).
    Oracle(OracleCollation),
}

/// A collation that this crate can compare with, or an explicit refusal.
///
/// Construct one of these, then call [`Collation::compare`]. Construction fails
/// closed: anything not modelled exactly yields [`Error`].
///
/// Serialization round-trips the *description* ([`Backend`]); deserialization
/// re-runs the same validated construction, so a tampered or stale payload
/// cannot produce an unchecked `Collation`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "Backend", into = "Backend"))]
pub struct Collation {
    backend: Backend,
}

#[cfg(feature = "serde")]
impl TryFrom<Backend> for Collation {
    type Error = Error;

    fn try_from(backend: Backend) -> Result<Self> {
        Self::from_backend(backend)
    }
}

#[cfg(feature = "serde")]
impl From<Collation> for Backend {
    fn from(collation: Collation) -> Self {
        collation.backend
    }
}

impl Collation {
    /// Bytewise comparison. Always available.
    #[must_use]
    pub const fn binary() -> Self {
        Self {
            backend: Backend::Binary,
        }
    }

    /// `PostgreSQL` `C`/`POSIX`, bytewise.
    #[must_use]
    pub const fn postgres_c() -> Self {
        Self::binary()
    }

    /// Construct a `PostgreSQL` ICU collation, validating the version at runtime.
    ///
    /// Returns [`Error::VersionMismatch`] if `source_version` does not match the
    /// ICU data the linked backend would use, and [`Error::Unsupported`] for
    /// custom rules or non-ICU providers.
    pub fn postgres_icu(
        locale: impl Into<String>,
        deterministic: bool,
        source_version: VersionId,
    ) -> Result<Self> {
        let spec = PostgresCollation::icu(locale, deterministic, source_version);
        Self::postgres(spec)
    }

    /// Construct a collation from a fully resolved `PostgreSQL` description.
    ///
    /// Refuses configurations this crate cannot reproduce exactly: libc
    /// providers other than `C`/`POSIX`, custom ICU rules, and non-UTF-8
    /// sources.
    #[allow(clippy::needless_pass_by_value)]
    pub fn postgres(spec: PostgresCollation) -> Result<Self> {
        if !spec.encoding().eq_ignore_ascii_case("UTF8") {
            return Err(Error::unsupported(
                "postgres",
                spec.locale(),
                "only UTF-8 sources are reproducible; other encodings are refused",
            ));
        }
        if spec.rules().is_some() {
            return Err(Error::unsupported(
                "postgres",
                spec.locale(),
                "custom ICU collation rules (collicurules) are not reproducible",
            ));
        }
        match spec.provider() {
            PostgresProvider::C => Ok(Self::binary()),
            PostgresProvider::Libc => {
                // `C`/`POSIX` under libc are bytewise and reproducible; other
                // libc locales depend on the host OS locale database and are not.
                let locale = spec.locale();
                if locale.eq_ignore_ascii_case("C") || locale.eq_ignore_ascii_case("POSIX") {
                    Ok(Self::binary())
                } else {
                    Err(Error::unsupported(
                        "postgres",
                        locale,
                        "libc collations other than C/POSIX are not reproducible portably",
                    ))
                }
            }
            PostgresProvider::Icu => {
                #[cfg(feature = "postgres-icu")]
                {
                    crate::postgres::validate(&spec)?;
                    Ok(Self {
                        backend: Backend::PostgresIcu(spec),
                    })
                }
                #[cfg(not(feature = "postgres-icu"))]
                {
                    let _ = spec;
                    Err(Error::FeatureDisabled {
                        feature: "postgres-icu",
                    })
                }
            }
        }
    }

    /// Construct the modelled `MySQL` `utf8mb4_0900_ai_ci` collation.
    ///
    /// Only available with the `mysql-uca` feature (enabled by default). Use
    /// [`Collation::mysql`] when the feature set is not known at compile time;
    /// it returns [`Error::FeatureDisabled`] instead.
    #[cfg(feature = "mysql-uca")]
    #[must_use]
    pub fn mysql_0900_ai_ci() -> Self {
        Self::mysql(MysqlCollation::utf8mb4_0900_ai_ci()).expect("utf8mb4_0900_ai_ci is supported")
    }

    /// Construct the modelled `MySQL` `utf8mb4_unicode_ci` collation.
    ///
    /// Only available with the `mysql-uca` feature (enabled by default). Use
    /// [`Collation::mysql`] when the feature set is not known at compile time;
    /// it returns [`Error::FeatureDisabled`] instead.
    #[cfg(feature = "mysql-uca")]
    #[must_use]
    pub fn mysql_unicode_ci() -> Self {
        Self::mysql(MysqlCollation::utf8mb4_unicode_ci()).expect("utf8mb4_unicode_ci is supported")
    }

    /// Construct a collation from a fully resolved `MySQL` description.
    #[allow(clippy::needless_pass_by_value)]
    pub fn mysql(spec: MysqlCollation) -> Result<Self> {
        #[cfg(feature = "mysql-uca")]
        {
            crate::mysql::validate(&spec)?;
            Ok(Self {
                backend: Backend::MysqlUca(spec),
            })
        }
        #[cfg(not(feature = "mysql-uca"))]
        {
            let _ = spec;
            Err(Error::FeatureDisabled {
                feature: "mysql-uca",
            })
        }
    }

    /// Construct a collation from a fully resolved `Oracle` description.
    ///
    /// Only `NLS_SORT=BINARY` on `AL32UTF8` `VARCHAR2` (bytewise) and the
    /// `UCA0700_DUCET` / `UCA1210_DUCET` collations are reproducible. Every
    /// other Oracle collation — monolingual, `_M`, `BINARY_CI`/`BINARY_AI`,
    /// `*_ROOT`, `*_ORADUCET`, tailored UCA, non-`AL32UTF8`, blank-padded — is
    /// refused with [`Error::Unsupported`].
    #[allow(clippy::needless_pass_by_value)]
    pub fn oracle(spec: OracleCollation) -> Result<Self> {
        #[cfg(feature = "oracle")]
        {
            Ok(Self {
                backend: crate::oracle::validate(&spec)?,
            })
        }
        #[cfg(not(feature = "oracle"))]
        {
            let _ = spec;
            Err(Error::FeatureDisabled { feature: "oracle" })
        }
    }

    /// Resolve a `MySQL` collation by name **without a version**, refusing any
    /// name whose semantics depend on the server version.
    ///
    /// This is intentionally conservative. Prefer [`Collation::mysql`] with an
    /// explicit [`MysqlCollation`] resolved by the caller.
    pub fn from_mysql_name_unspecified_version(name: &str) -> Result<Self> {
        match name {
            "utf8mb4_0900_ai_ci" => Self::mysql(MysqlCollation::utf8mb4_0900_ai_ci()),
            "utf8mb4_unicode_ci" => Self::mysql(MysqlCollation::utf8mb4_unicode_ci()),
            // `utf8mb4_0900_bin` is NO PAD and orders by code point; for valid
            // UTF-8 that is exactly the bytewise `binary` backend. (The legacy
            // `utf8mb4_bin` is PAD SPACE and is intentionally not mapped here.)
            "utf8mb4_0900_bin" => Ok(Self::binary()),
            _ => Err(Error::unsupported(
                "mysql",
                name,
                "unrecognised or version-dependent collation; resolve it explicitly",
            )),
        }
    }

    /// The backend this collation uses.
    #[must_use]
    pub const fn backend(&self) -> &Backend {
        &self.backend
    }

    /// Compare two UTF-8 strings exactly as the source database would.
    ///
    /// This is the primary operation. Sort keys are intentionally not part of
    /// the stable API: the contract is a three-way comparison, and a collation
    /// that also defines a bytewise tie-break (`PostgreSQL` deterministic
    /// collations) cannot be reproduced by a single ICU sort key, which encodes
    /// only the primary ICU comparison.
    ///
    /// Returns [`Error`] if the underlying backend fails at runtime. The binary
    /// backend never fails; the ICU backend can, and a failed comparison is
    /// never silently reported as equality.
    pub fn compare(&self, a: &str, b: &str) -> Result<Ordering> {
        match &self.backend {
            Backend::Binary => Ok(a.as_bytes().cmp(b.as_bytes())),
            #[cfg(feature = "postgres-icu")]
            Backend::PostgresIcu(spec) => crate::postgres::compare(spec, a, b),
            #[cfg(not(feature = "postgres-icu"))]
            Backend::PostgresIcu(_) => unreachable!("postgres backend cannot be constructed"),
            #[cfg(feature = "mysql-uca")]
            Backend::MysqlUca(spec) => Ok(crate::mysql::compare(spec, a, b)),
            #[cfg(not(feature = "mysql-uca"))]
            Backend::MysqlUca(_) => unreachable!("mysql backend cannot be constructed"),
            #[cfg(feature = "oracle-uca")]
            Backend::Oracle(spec) => crate::oracle::compare(spec, a, b),
            #[cfg(not(feature = "oracle-uca"))]
            Backend::Oracle(_) => unreachable!("oracle backend cannot be constructed"),
        }
    }

    /// Whether `a` and `b` are equal under this collation.
    ///
    /// Returns [`Error`] for the same reasons as [`Collation::compare`].
    pub fn equal(&self, a: &str, b: &str) -> Result<bool> {
        Ok(self.compare(a, b)? == Ordering::Equal)
    }

    /// Build a collation from a resolved [`Backend`] description, re-running the
    /// same validation as the public constructors.
    ///
    /// This is the single construction path used by deserialization, so an
    /// arbitrary or stale description cannot bypass the support boundary.
    pub fn from_backend(backend: Backend) -> Result<Self> {
        match backend {
            Backend::Binary => Ok(Self::binary()),
            Backend::PostgresIcu(spec) => Self::postgres(spec),
            Backend::MysqlUca(spec) => Self::mysql(spec),
            Backend::Oracle(spec) => Self::oracle(spec),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_is_bytewise() {
        let c = Collation::binary();
        assert_eq!(c.compare("a", "b").unwrap(), Ordering::Less);
        assert_eq!(c.compare("A", "a").unwrap(), Ordering::Less);
        assert!(c.equal("a", "a").unwrap());
    }

    #[test]
    fn test_postgres_libc_is_refused() {
        let spec = PostgresCollation::libc("en_US.UTF-8");
        assert!(Collation::postgres(spec).is_err());
    }

    #[test]
    fn test_postgres_libc_c_is_binary() {
        for locale in ["C", "POSIX", "posix"] {
            let spec = PostgresCollation::libc(locale);
            let c = Collation::postgres(spec).unwrap();
            assert!(matches!(c.backend(), Backend::Binary), "{locale}");
        }
    }

    #[test]
    fn test_unspecified_mysql_name_unknown_is_refused() {
        assert!(Collation::from_mysql_name_unspecified_version("utf8mb4_general_ci").is_err());
        assert!(Collation::from_mysql_name_unspecified_version("utf8mb4_zh_0900_as_cs").is_err());
    }

    #[test]
    fn test_collation_is_send_sync() {
        static_assertions::assert_impl_all!(Collation: Send, Sync);
    }

    #[test]
    fn test_error_is_fallback() {
        let e = Error::unsupported("mysql", "x", "y");
        assert!(e.is_fallback_required());
    }
}
