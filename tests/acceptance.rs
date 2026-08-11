//! Acceptance tests: one test per spec scenario, named after it.
//! Structure each test as Given / When / Then, mirroring the spec.

use keeler_example::checked_total;

// Placeholder scenario — replace with tests named after your first
// spec's scenarios. Scenario: Overflow never panics
#[test]
fn overflow_never_panics() {
    // Given a set of values whose sum exceeds u64::MAX
    let values = [u64::MAX, u64::MAX, 42];
    // When the total is computed
    let total = checked_total(&values);
    // Then the result saturates instead of panicking
    assert_eq!(total, u64::MAX);
}
