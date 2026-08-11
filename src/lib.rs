//! Placeholder module proving the quality pipeline end-to-end
//! (tests → coverage → cargo-crap → mutants). Replace with real
//! feature code driven by a spec in `specs/`.

/// Saturating sum of a slice.
#[must_use]
pub fn checked_total(values: &[u64]) -> u64 {
    values.iter().fold(0u64, |acc, v| acc.saturating_add(*v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_slice_totals_zero() {
        assert_eq!(checked_total(&[]), 0);
    }

    #[test]
    fn sums_small_values() {
        assert_eq!(checked_total(&[1, 2, 3]), 6);
    }

    #[test]
    fn saturates_instead_of_overflowing() {
        assert_eq!(checked_total(&[u64::MAX, 1]), u64::MAX);
    }

    proptest! {
        #[test]
        fn never_less_than_any_element(values in prop::collection::vec(any::<u64>(), 0..8)) {
            let total = checked_total(&values);
            for v in &values {
                prop_assert!(total >= *v || total == u64::MAX);
            }
        }

        #[test]
        fn order_does_not_matter(mut values in prop::collection::vec(any::<u64>(), 0..8)) {
            let forward = checked_total(&values);
            values.reverse();
            prop_assert_eq!(forward, checked_total(&values));
        }
    }
}
