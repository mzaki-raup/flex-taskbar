#![cfg_attr(not(windows), allow(dead_code))]

//! Measuring FlexTaskbar itself (*Diagnostics…*): drawing times kept as
//! running statistics, and the report's wording. Pure, so it can be
//! unit-tested off Windows; `win::diagnostics` reads the process numbers.

/// Running statistics of one kind of work, in microseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stat {
    pub count: u64,
    pub total_us: u64,
    pub max_us: u64,
    pub last_us: u64,
}

impl Stat {
    pub fn add(&mut self, us: u64) {
        self.count += 1;
        self.total_us = self.total_us.saturating_add(us);
        self.max_us = self.max_us.max(us);
        self.last_us = us;
    }

    pub fn average_us(&self) -> u64 {
        self.total_us.checked_div(self.count).unwrap_or(0)
    }

    /// "1,204 times, average 0.8 ms, longest 4.1 ms" (or "not yet").
    pub fn describe(&self) -> String {
        if self.count == 0 {
            return "not yet".into();
        }
        format!(
            "{} {}, average {}, longest {}",
            thousands(self.count),
            if self.count == 1 { "time" } else { "times" },
            ms(self.average_us()),
            ms(self.max_us)
        )
    }
}

/// `12345678` → "12,345,678".
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Microseconds as milliseconds with one decimal.
pub fn ms(us: u64) -> String {
    format!("{}.{} ms", us / 1000, (us % 1000) / 100)
}

/// Bytes as MB with one decimal.
pub fn mb(bytes: u64) -> String {
    let tenths = bytes * 10 / (1024 * 1024);
    format!("{}.{} MB", tenths / 10, tenths % 10)
}

/// Seconds as "2 h 05 min", "3 min 07 s" or "42 s".
pub fn duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h} h {m:02} min")
    } else if m > 0 {
        format!("{m} min {s:02} s")
    } else {
        format!("{s} s")
    }
}

/// Average CPU use in percent of one core over `wall_secs`, to one decimal.
pub fn cpu_percent(cpu_us: u64, wall_secs: f64) -> String {
    if wall_secs <= 0.0 {
        return "0.0 %".into();
    }
    format!("{:.1} %", cpu_us as f64 / 1e6 / wall_secs * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats() {
        let mut s = Stat::default();
        assert_eq!(s.describe(), "not yet");
        assert_eq!(s.average_us(), 0);
        s.add(800);
        assert_eq!(s.describe(), "1 time, average 0.8 ms, longest 0.8 ms");
        s.add(4100);
        s.add(300);
        assert_eq!((s.count, s.max_us, s.last_us, s.average_us()), (3, 4100, 300, 1733));
        assert_eq!(s.describe(), "3 times, average 1.7 ms, longest 4.1 ms");
    }

    #[test]
    fn formatting() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1204), "1,204");
        assert_eq!(thousands(12_345_678), "12,345,678");
        assert_eq!(ms(0), "0.0 ms");
        assert_eq!(ms(16_650), "16.6 ms");
        assert_eq!(mb(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
        assert_eq!(duration(42), "42 s");
        assert_eq!(duration(187), "3 min 07 s");
        assert_eq!(duration(7500), "2 h 05 min");
        assert_eq!(cpu_percent(600_000, 60.0), "1.0 %");
        assert_eq!(cpu_percent(5, 0.0), "0.0 %");
    }
}
