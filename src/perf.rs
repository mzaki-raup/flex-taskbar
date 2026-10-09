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

// ---------------------------------------------------------------- starts

/// A start a copy of FlexTaskbar recorded for itself: when, and how long
/// after Windows itself had started.
///
/// Kept so *Diagnostics…* can answer a question nothing else can: did it
/// start at logon, or only when someone ran it by hand? After a reboot that
/// is the difference between "Start with Windows" working and not, and
/// without a record there is no way to tell after the fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    /// Unix time it started.
    pub at: u64,
    /// Seconds between Windows starting and this copy starting.
    pub after_boot: u64,
}

/// How soon after Windows starts a launch still counts as "at logon".
/// Windows staggers the Run entries, and a slow machine can take a while to
/// reach the desktop, so this is generous rather than exact.
pub const AT_LOGON_SECS: u64 = 180;

impl Start {
    /// Whether this looks like Windows started it rather than a person.
    pub fn at_logon(&self) -> bool {
        self.after_boot <= AT_LOGON_SECS
    }

    /// One line of the file: `<unix time> <seconds after boot>`.
    pub fn to_line(self) -> String {
        format!("{} {}", self.at, self.after_boot)
    }

    pub fn from_line(line: &str) -> Option<Start> {
        let mut parts = line.split_whitespace();
        let at = parts.next()?.parse().ok()?;
        let after_boot = parts.next()?.parse().ok()?;
        parts.next().is_none().then_some(Start { at, after_boot })
    }
}

/// The starts in a file, oldest first. Lines that make no sense are dropped
/// rather than losing the rest: this is a record, not settings.
pub fn parse_starts(text: &str) -> Vec<Start> {
    text.lines().filter_map(|l| Start::from_line(l.trim())).collect()
}

/// What to write back, keeping the newest `keep` of them.
pub fn starts_file(starts: &[Start], keep: usize) -> String {
    let from = starts.len().saturating_sub(keep);
    starts[from..].iter().map(|s| s.to_line()).collect::<Vec<_>>().join("\n")
}

/// How a start reads in the report: "at logon" is the part that matters.
pub fn describe_start(start: &Start, now: u64) -> String {
    let ago = now.saturating_sub(start.at);
    let how = if start.at_logon() {
        format!("at logon, {} after Windows started", duration(start.after_boot))
    } else {
        format!("{} after Windows started, so not at logon", duration(start.after_boot))
    };
    format!("{} ago \u{2014} {how}", duration(ago))
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_start_soon_after_windows_counts_as_at_logon() {
        assert!(Start { at: 100, after_boot: 0 }.at_logon());
        assert!(Start { at: 100, after_boot: AT_LOGON_SECS }.at_logon(), "the boundary counts");
        assert!(!Start { at: 100, after_boot: AT_LOGON_SECS + 1 }.at_logon());
        assert!(!Start { at: 100, after_boot: 3600 }.at_logon(), "an hour later is somebody running it");
    }

    #[test]
    fn a_start_survives_being_written_and_read_back() {
        let s = Start { at: 1_760_000_000, after_boot: 42 };
        assert_eq!(Start::from_line(&s.to_line()), Some(s));
    }

    #[test]
    fn nonsense_lines_are_dropped_without_losing_the_rest() {
        let text = "100 5\nrubbish\n\n200 7\n300\n400 8 9\n500 x\n600 10";
        let got = parse_starts(text);
        assert_eq!(got.iter().map(|s| s.at).collect::<Vec<_>>(), [100, 200, 600]);
    }

    #[test]
    fn only_the_newest_starts_are_kept() {
        let starts: Vec<Start> = (1..=10).map(|n| Start { at: n * 100, after_boot: n }).collect();
        let kept = parse_starts(&starts_file(&starts, 3));
        assert_eq!(kept.iter().map(|s| s.at).collect::<Vec<_>>(), [800, 900, 1000], "the last three");
    }

    #[test]
    fn keeping_more_than_there_are_keeps_them_all() {
        let starts = [Start { at: 100, after_boot: 1 }, Start { at: 200, after_boot: 2 }];
        assert_eq!(parse_starts(&starts_file(&starts, 10)), starts);
        assert_eq!(starts_file(&[], 5), "", "and nothing recorded is an empty file");
    }

    #[test]
    fn the_report_says_whether_a_start_was_at_logon() {
        let now = 10_000;
        let logon = describe_start(&Start { at: 9_000, after_boot: 20 }, now);
        assert!(logon.contains("at logon"), "{logon}");
        let by_hand = describe_start(&Start { at: 9_000, after_boot: 7_200 }, now);
        assert!(by_hand.contains("not at logon"), "{by_hand}");
        // Both say how long ago, so a start can be lined up with a reboot.
        assert!(logon.starts_with(&duration(1_000)) && by_hand.starts_with(&duration(1_000)));
    }
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
