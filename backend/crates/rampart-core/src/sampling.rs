//! Deterministic head sampling for the high-volume ingest tiers.
//!
//! The decision is a pure function of a stable key (a trace id, mostly) and the
//! keep-percentage, so it needs no state and is identical across replicas and
//! across the spans/logs of one trace — keeping a trace's waterfall and its
//! correlated logs whole rather than half-sampled. FNV-1a (not the default
//! hasher) so the mapping is fixed forever, not per-process.

/// Keep an item whose sampling key is `key` at `keep_pct` percent. `>= 100`
/// keeps everything (sampling off); `<= 0` drops everything.
pub fn keep(key: &str, keep_pct: i32) -> bool {
    if keep_pct >= 100 {
        return true;
    }
    if keep_pct <= 0 {
        return false;
    }
    (fnv1a(key.as_bytes()) % 100) < keep_pct as u64
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::keep;

    #[test]
    fn off_keeps_all_on_drops_all() {
        for k in ["a", "b", "trace-123"] {
            assert!(keep(k, 100));
            assert!(keep(k, 1000));
            assert!(!keep(k, 0));
            assert!(!keep(k, -5));
        }
    }

    #[test]
    fn deterministic_per_key() {
        // Same key + pct always decides the same way (replica-safe).
        let k = "0123456789abcdef";
        assert_eq!(keep(k, 50), keep(k, 50));
    }

    #[test]
    fn roughly_proportional() {
        // ~10% of 10k distinct keys kept at pct=10 (loose bounds — it's a hash).
        let kept = (0..10_000)
            .filter(|i| keep(&format!("trace-{i}"), 10))
            .count();
        assert!((600..1600).contains(&kept), "kept={kept}");
    }
}
