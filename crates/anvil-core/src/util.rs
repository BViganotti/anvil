//! Small self-contained utilities: deterministic RNG, bounded parsing helpers
//! and engineering value parsing.

/// A deterministic xorshift64* generator. Anvil never uses OS randomness for
/// design decisions: autoroute results, generated examples and fuzz corpora all
/// derive from explicit seeds.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng {
            state: if seed == 0 { 0x9e37_79b9_7f4a_7c15 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform integer in `[0, n)`. Returns 0 for `n == 0`.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }

    pub fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo) as u64;
        lo + (self.next_u64() % span) as i64
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let j = self.below((i + 1) as u32) as usize;
            items.swap(i, j);
        }
    }
}

/// Parse an engineering value with an SI suffix. Accepted examples:
/// `10`, `4.7k`, `1M`, `2.2u`, `100n`, `1e3`, `-5`, `0.05`.
///
/// `M` means mega and `m` means milli; `k`/`K` both mean kilo. Trailing unit
/// text is rejected unless it is one of the recognised multipliers followed by
/// an optional unit marker (`4.7kOhm` is accepted, `abc` is not).
pub fn parse_eng(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s.len() > 32 {
        return None;
    }
    let mut idx = 0;
    let bytes = s.as_bytes();
    if bytes[0] == b'+' || bytes[0] == b'-' {
        idx = 1;
    }
    let start_digits = idx;
    while idx < bytes.len() && (bytes[idx].is_ascii_digit() || bytes[idx] == b'.') {
        idx += 1;
    }
    if idx == start_digits {
        return None;
    }
    let number: f64 = s[..idx].parse().ok()?;
    if !number.is_finite() {
        return None;
    }
    let tail = &s[idx..];
    if tail.is_empty() {
        return Some(number);
    }
    let (mult, rest) = match tail.as_bytes()[0] {
        b'p' => (1e-12, &tail[1..]),
        b'n' => (1e-9, &tail[1..]),
        b'u' | b'U' => (1e-6, &tail[1..]),
        b'm' => (1e-3, &tail[1..]),
        b'k' | b'K' => (1e3, &tail[1..]),
        b'M' => (1e6, &tail[1..]),
        b'G' => (1e9, &tail[1..]),
        b'T' => (1e12, &tail[1..]),
        b'e' | b'E' => return s.parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => return None,
    };
    // Allow a short alphabetic unit marker after the multiplier.
    if !rest.is_empty()
        && !rest
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == 'Ω' || c == 'F' || c == 'H')
    {
        return None;
    }
    if rest.len() > 8 {
        return None;
    }
    let value = number * mult;
    if value.is_finite() {
        Some(value)
    } else {
        None
    }
}

/// Parse a decimal integer with hard bounds.
pub fn parse_i64_limited(s: &str, min: i64, max: i64) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || s.len() > 24 {
        return None;
    }
    let v: i64 = s.parse().ok()?;
    if v < min || v > max {
        None
    } else {
        Some(v)
    }
}

/// Format a nanometre value as millimetres with at most six decimals and no
/// trailing zeros. Used for Gerber comments, documentation and UI labels.
pub fn fmt_mm(nm: i64) -> String {
    let neg = nm < 0;
    let n = (nm as i128).unsigned_abs();
    let whole = n / 1_000_000;
    let frac = n % 1_000_000;
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    s.push_str(&whole.to_string());
    if frac != 0 {
        let mut f = format!("{:06}", frac);
        while f.ends_with('0') {
            f.pop();
        }
        s.push('.');
        s.push_str(&f);
    }
    s
}

/// Shared hard limits. Every parser in Anvil enforces a bound from this table;
/// they exist so malformed input cannot exhaust memory or stack.
pub mod limits {
    pub const MAX_COMPONENTS: usize = 5_000;
    pub const MAX_PINS_PER_COMPONENT: usize = 256;
    pub const MAX_WIRES: usize = 50_000;
    pub const MAX_WIRE_POINTS: usize = 2_000;
    pub const MAX_LABELS: usize = 20_000;
    pub const MAX_BUSES: usize = 256;
    pub const MAX_BUS_MEMBERS: usize = 128;
    pub const MAX_NAME: usize = 128;
    pub const MAX_COMPONENT_PROPERTIES: usize = 64;
    pub const MAX_NETLIST_NODES: usize = 200_000;
    pub const MAX_SIM_NODES: usize = 512;
    pub const MAX_SIM_ELEMENTS: usize = 4_000;
    pub const MAX_SIM_STEPS: u32 = 2_000_000;
    pub const MAX_TRACE_POINTS: usize = 10_000;
    pub const MAX_BOARD_ITEMS: usize = 20_000;
    pub const MAX_JSON_DEPTH: usize = 64;
    pub const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
    pub const MAX_HTTP_HEADER: usize = 32 * 1024;
    pub const MAX_HTTP_BODY: usize = 4 * 1024 * 1024;
    pub const MAX_GERBER_BYTES: usize = 16 * 1024 * 1024;
    pub const MAX_GERBER_OPS: usize = 1_000_000;
    pub const MAX_RASTER: usize = 3000;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engineering_values() {
        assert_eq!(parse_eng("10"), Some(10.0));
        assert_eq!(parse_eng("4.7k"), Some(4700.0));
        assert_eq!(parse_eng("1M"), Some(1e6));
        assert_eq!(parse_eng("2.2u"), Some(2.2e-6));
        assert!((parse_eng("100n").unwrap() - 100e-9).abs() < 1e-20);
        assert_eq!(parse_eng("1e3"), Some(1000.0));
        assert_eq!(parse_eng("-5"), Some(-5.0));
        assert_eq!(parse_eng("4.7kOhm"), Some(4700.0));
        assert_eq!(parse_eng("abc"), None);
        assert_eq!(parse_eng(""), None);
        assert_eq!(parse_eng("1e999"), None);
    }

    #[test]
    fn rng_is_reproducible() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_mm(1_500_000), "1.5");
        assert_eq!(fmt_mm(-2_000_000), "-2");
        assert_eq!(fmt_mm(1), "0.000001");
    }
}
