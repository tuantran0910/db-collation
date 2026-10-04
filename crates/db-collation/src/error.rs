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

//! Error type for [`crate::Collation`] construction and use.

use core::fmt;

use crate::spec::VersionId;

/// Convenience alias for [`Result`](core::result::Result) with [`Error`].
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced when building or using a collation.
///
/// The dominant variant is [`Error::Unsupported`]: this crate refuses to guess.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The configuration is not one this crate models exactly.
    ///
    /// The caller must fall back to comparing on the source database.
    Unsupported {
        /// The provider or engine that was requested.
        engine: &'static str,
        /// The collation name or locale that was requested.
        collation: String,
        /// Why it is unsupported.
        reason: &'static str,
    },

    /// The supplied version metadata does not match the local collation data.
    ///
    /// Falling back to the source database is the correct response.
    VersionMismatch {
        /// Version the caller says the source is using.
        source: VersionId,
        /// Version this build of the library would use.
        local: VersionId,
    },

    /// The configuration was recognised but a required backend feature is
    /// disabled at compile time.
    FeatureDisabled {
        /// The cargo feature that would enable it.
        feature: &'static str,
    },

    /// The collation is modelled, but a *specific input* exceeds a documented
    /// limit of the source backend, so the comparison cannot be reproduced
    /// exactly (e.g. Oracle truncates its UCA sort key past 2000 bytes).
    ///
    /// Unlike [`Error::Unsupported`], the configuration itself is supported;
    /// only this input must fall back to comparing on the source. This is a
    /// per-input outcome, not a construction failure.
    InputLimit {
        /// The provider or engine that was requested.
        engine: &'static str,
        /// Why the input cannot be compared.
        reason: &'static str,
    },
}

impl Error {
    /// Construct an [`Error::Unsupported`].
    pub(crate) fn unsupported(
        engine: &'static str,
        collation: impl Into<String>,
        reason: &'static str,
    ) -> Self {
        Self::Unsupported {
            engine,
            collation: collation.into(),
            reason,
        }
    }

    /// Construct an [`Error::InputLimit`].
    #[cfg(feature = "oracle-uca")]
    pub(crate) fn input_limit(engine: &'static str, reason: &'static str) -> Self {
        Self::InputLimit { engine, reason }
    }

    /// Returns `true` if the caller should fall back to the source database.
    #[must_use]
    pub const fn is_fallback_required(&self) -> bool {
        matches!(
            self,
            Self::Unsupported { .. }
                | Self::VersionMismatch { .. }
                | Self::FeatureDisabled { .. }
                | Self::InputLimit { .. }
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported {
                engine,
                collation,
                reason,
            } => write!(
                f,
                "unsupported {engine} collation \"{collation}\": {reason}; compare on the source instead"
            ),
            Self::VersionMismatch { source, local } => write!(
                f,
                "collation data version mismatch: source {source}, local {local}; compare on the source instead"
            ),
            Self::FeatureDisabled { feature } => {
                write!(
                    f,
                    "backend disabled at compile time; enable feature {feature:?}"
                )
            }
            Self::InputLimit { engine, reason } => write!(
                f,
                "{engine} collation cannot compare this input: {reason}; compare on the source instead"
            ),
        }
    }
}

impl core::error::Error for Error {}
