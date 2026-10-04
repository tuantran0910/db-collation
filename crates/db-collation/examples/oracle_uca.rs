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

//! Compare strings under Oracle's `UCA1210_DUCET` collation, offline.

use std::cmp::Ordering;

use db_collation::{Collation, OracleCollation, OracleUcaVersion, Result};

fn main() -> Result<()> {
    let c = Collation::oracle(OracleCollation::uca_ducet(OracleUcaVersion::Uca1210))?;

    // Case and accent differences are secondary/tertiary under the DUCET.
    assert_eq!(c.compare("a", "A")?, Ordering::Less);
    assert_eq!(c.compare("a", "á")?, Ordering::Less);
    // The eszett expands to two "s" primaries; it differs from "ss" only at the
    // secondary level, so "ss" sorts first.
    assert_eq!(c.compare("ss", "ß")?, Ordering::Less);
    assert_eq!(c.compare("strasse", "straße")?, Ordering::Less);

    println!(
        "UCA1210_DUCET: \"strasse\" < \"straße\" = {}",
        c.compare("strasse", "straße")? == Ordering::Less
    );
    Ok(())
}
