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

//! Compare and sort strings with a modelled `MySQL` collation.
//!
//! Run with `cargo run --example basic_mysql`.

use db_collation::Collation;
use db_collation::Result;

fn main() -> Result<()> {
    let ci = Collation::mysql_0900_ai_ci();

    println!("utf8mb4_0900_ai_ci (UCA 9.0.0, NO PAD)");
    println!("  a == A              -> {}", ci.equal("a", "A")?);
    println!("  e == e-acute        -> {}", ci.equal("e", "\u{e9}")?);
    println!("  sharp-s == ss       -> {}", ci.equal("\u{df}", "ss")?);
    println!("  a == a-space        -> {} (NO PAD)", ci.equal("a", "a ")?);

    let mut words = vec!["Banana", "apple", "Apple", "banana", "Ávila", "avila"];
    // compare returns Result; unwrap in an example is fine, fall back in real code.
    words.sort_by(|a, b| {
        ci.compare(a, b)
            .expect("binary/uca comparison is infallible")
    });
    println!("  sorted: {words:?}");

    let unicode = Collation::mysql_unicode_ci();
    println!("\nutf8mb4_unicode_ci (UCA 4.0.0, PAD SPACE)");
    println!(
        "  a == a-space        -> {} (PAD SPACE)",
        unicode.equal("a", "a ")?
    );
    println!(
        "  compare(\"a\", \"b\")   -> {:?}",
        unicode.compare("a", "b")?
    );

    let binary = Collation::binary();
    println!("\nbinary");
    println!("  A vs a              -> {:?}", binary.compare("A", "a")?);

    println!("\nunsupported configurations are refused:");
    match Collation::from_mysql_name_unspecified_version("utf8mb4_general_ci") {
        Ok(_) => println!("  unexpectedly supported"),
        Err(e) => println!("  utf8mb4_general_ci -> {e}"),
    }
    Ok(())
}
