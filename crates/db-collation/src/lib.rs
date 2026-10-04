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

//! # db-collation
//!
//! Offline, exact string comparison under an **explicitly identified database
//! collation and version**.
//!
//! This crate exists for one purpose: reproduce the *exact* ordering semantics
//! of a source database's collation locally, without talking to that database.
//! It is a pure function of a [`Collation`] description and the input bytes. It
//! opens no sockets, holds no drivers, and never issues a query.
//!
//! ## Guarantees and non-guarantees
//!
//! - **Exact or refused.** For a supported configuration, [`Collation::compare`]
//!   matches the source database's comparison operator exactly (validated by the
//!   differential harness in `harness/`). For anything not explicitly modelled,
//!   construction returns [`Error::Unsupported`] and the caller should fall back
//!   to comparing on the source database.
//! - **No approximation.** Selecting the same locale name is *not* sufficient:
//!   ICU data versions change ordering (e.g. `あ` vs `U+11DB0` reversed between
//!   ICU 76 and 78), and `MySQL` collations differ by UCA version, tailoring and
//!   padding. This crate therefore gates on version, not on names alone.
//! - **Deterministic tie-break.** `PostgreSQL` deterministic collations break ICU
//!   equality ties with a bytewise comparison (shorter string first). That is
//!   reproduced here; nondeterministic collations intentionally do not.
//!
//! ## Supported configurations
//!
//! See [`Collation`] and the crate `README` for the current matrix.
//!
//! # Examples
//!
//! ```
//! use db_collation::{Collation, Result};
//! use std::cmp::Ordering;
//!
//! fn main() -> Result<()> {
//!     // Bytewise comparison is always available.
//!     assert_eq!(Collation::binary().compare("a", "b")?, Ordering::Less);
//!
//!     // Unsupported configurations fail closed, so the caller can fall back.
//!     assert!(Collation::from_mysql_name_unspecified_version("utf8mb4_general_ci").is_err());
//!     Ok(())
//! }
//! ```
//!
//! With the `mysql-uca` feature (enabled by default):
//!
//! ```
//! # #[cfg(feature = "mysql-uca")] {
//! use db_collation::{Collation, Result};
//! use std::cmp::Ordering;
//!
//! fn main() -> Result<()> {
//!     let c = Collation::mysql_0900_ai_ci();
//!     assert_eq!(c.compare("a", "A")?, Ordering::Equal);
//!     Ok(())
//! }
//! # }
//! ```
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_op_in_unsafe_fn)]

mod error;
#[cfg(feature = "mysql-uca")]
#[allow(unreachable_pub)]
mod mysql;
#[cfg(feature = "postgres-icu")]
#[allow(unreachable_pub)]
mod postgres;
mod spec;
#[cfg(feature = "postgres-icu")]
#[allow(unreachable_pub)]
mod thread_local;

pub use error::{Error, Result};
#[cfg(feature = "postgres-icu")]
pub use postgres::{collation_version, icu_library_version};
pub use spec::{
    Backend, Collation, MysqlCollation, MysqlUcaVersion, Pad, PostgresCollation, PostgresProvider,
    VersionId,
};
