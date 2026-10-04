#![no_main]

//! Fuzz the comparison contract: a collation must be a total preorder, and must
//! never panic. Exactness against a source is validated by the differential
//! harness, not here.

use std::cmp::Ordering;

use db_collation::Collation;
use libfuzzer_sys::fuzz_target;

/// Split `s` into up to three independently valid UTF-8 strings at character
/// boundaries. Never splits inside a multibyte character.
fn three_pieces(s: &str) -> (&str, &str, &str) {
    let mut boundaries: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    boundaries.push(s.len());
    let n = boundaries.len();
    let i = boundaries[n / 3];
    let j = boundaries[(2 * n) / 3];
    (&s[..i], &s[i..j], &s[j..])
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let (a, b, c3) = three_pieces(s);

    let collations = [
        Collation::binary(),
        Collation::mysql_0900_ai_ci(),
        Collation::mysql_unicode_ci(),
    ];

    for coll in collations {
        // The modelled collations do not fail on valid UTF-8; any error would be
        // a bug worth surfacing here.
        let ab = coll.compare(a, b).expect("comparison failed");
        let ba = coll.compare(b, a).expect("comparison failed");
        let ac = coll.compare(a, c3).expect("comparison failed");
        let bc = coll.compare(b, c3).expect("comparison failed");

        // Antisymmetry.
        assert_eq!(ab, ba.reverse());
        // Reflexivity.
        assert_eq!(coll.compare(a, a).unwrap(), Ordering::Equal);
        assert_eq!(coll.compare(b, b).unwrap(), Ordering::Equal);
        assert_eq!(coll.compare(c3, c3).unwrap(), Ordering::Equal);
        // Consistency of equal().
        assert_eq!(coll.equal(a, b).unwrap(), ab == Ordering::Equal);

        // Transitivity of the ordering.
        if ab != Ordering::Greater && bc != Ordering::Greater {
            assert_ne!(ac, Ordering::Greater, "transitivity violated");
        }
        if ab != Ordering::Less && bc != Ordering::Less {
            assert_ne!(ac, Ordering::Less, "transitivity violated");
        }

        // Equivalence substitution: if a == b then a and b compare identically
        // against c3.
        if ab == Ordering::Equal {
            assert_eq!(ac, bc, "equivalence substitution violated");
        }
    }
});
