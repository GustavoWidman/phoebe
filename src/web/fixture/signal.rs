//! noise that is a function of the moment alone, so any moment can be
//! computed directly, with nothing carried over from the one before

const MINUTE: i64 = 60_000;

/// a uniform value in [0, 1) for `(seed, i)`
pub fn hash(seed: u64, i: i64) -> f64 {
    (mix(seed, i) >> 11) as f64 / (1u64 << 53) as f64
}

/// a well-mixed 64-bit value for `(seed, i)`: splitmix64's finalizer
pub fn mix(seed: u64, i: i64) -> u64 {
    let mut z = seed
        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
        .wrapping_add(i as u64)
        .wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// noise in [-1, 1] that drifts over `period` milliseconds: random knots
/// `period` apart, eased into each other
fn smooth(seed: u64, at: i64, period: i64) -> f64 {
    let knot = at.div_euclid(period);
    let along = at.rem_euclid(period) as f64 / period as f64;
    let ease = (1.0 - (along * std::f64::consts::PI).cos()) / 2.0;
    let (a, b) = (hash(seed, knot), hash(seed, knot + 1));
    (a + (b - a) * ease) * 2.0 - 1.0
}

/// a metric's wander around its level, in [-1, 1]: a slow drift and a
/// faster jitter
pub fn wobble(seed: u64, at: i64) -> f64 {
    0.65 * smooth(seed, at, 20 * MINUTE) + 0.35 * smooth(seed ^ 0x5bd1_e995, at, 3 * MINUTE)
}

/// a value that follows straight lines between `knots`, `(at, value)` in
/// time order, and holds the first and last beyond them
pub fn knots(knots: &[(i64, f64)], at: i64) -> f64 {
    let after = knots.partition_point(|(t, _)| *t <= at);
    match (after.checked_sub(1).map(|i| knots[i]), knots.get(after)) {
        (Some((t0, v0)), Some(&(t1, v1))) => v0 + (v1 - v0) * (at - t0) as f64 / (t1 - t0) as f64,
        (Some((_, v)), None) | (None, Some(&(_, v))) => v,
        (None, None) => 0.0,
    }
}
