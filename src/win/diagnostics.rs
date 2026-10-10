//! *Diagnostics…*: what FlexTaskbar costs right now — memory, CPU time,
//! GDI/USER objects, handles — and how long drawing the bar and the
//! flyouts takes. Shown in a message box (Ctrl+C copies it) and written to
//! `FlexTaskbarData\diagnostics.txt`.

use super::{app, autostart, paths, ui};
use crate::perf::{self, Stat};
use std::cell::RefCell;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{
    GR_GDIOBJECTS, GR_GDIOBJECTS_PEAK, GR_USEROBJECTS, GR_USEROBJECTS_PEAK, GetCurrentProcess, GetGuiResources,
    GetProcessHandleCount, GetProcessTimes,
};

struct Stats {
    started: std::time::Instant,
    bar: Stat,
    flyout: Stat,
}

thread_local! {
    static STATS: RefCell<Stats> =
        RefCell::new(Stats { started: std::time::Instant::now(), bar: Stat::default(), flyout: Stat::default() });
}

/// How many starts to keep a note of.
const KEEP_STARTS: usize = 8;

/// Starts the clock, and notes this start in the data folder (call at
/// startup).
///
/// The note is what lets the report say whether a copy started *at logon* or
/// only when somebody ran it. Nothing else can answer that afterwards:
/// Windows keeps no record of having run a Run entry, and a copy that never
/// started leaves no trace at all. Eight is enough to see a pattern across a
/// few reboots and small enough never to matter.
pub fn start() {
    STATS.with(|s| s.borrow_mut().started = std::time::Instant::now());
    record_start();
}

/// Where the starts are noted.
fn starts_path() -> std::path::PathBuf {
    paths::get().data.join("starts.log")
}

/// Seconds since Windows itself started.
fn since_boot() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() / 1000 }
}

fn read_starts() -> Vec<perf::Start> {
    std::fs::read_to_string(starts_path()).map(|t| perf::parse_starts(&t)).unwrap_or_default()
}

/// Adds this start to the note. A failure is ignored: a record of starts is
/// never worth refusing to start over.
fn record_start() {
    let mut starts = read_starts();
    starts.push(perf::Start { at: crate::config::unix_time(), after_boot: since_boot() });
    let _ = std::fs::write(starts_path(), perf::starts_file(&starts, KEEP_STARTS));
}

/// The bar was drawn, taking `since.elapsed()`.
pub fn bar_drawn(since: std::time::Instant) {
    let us = since.elapsed().as_micros() as u64;
    STATS.with(|s| s.borrow_mut().bar.add(us));
}

/// A flyout was drawn.
pub fn flyout_drawn(since: std::time::Instant) {
    let us = since.elapsed().as_micros() as u64;
    STATS.with(|s| s.borrow_mut().flyout.add(us));
}

/// Whether Windows is set to start this copy, said plainly — including the
/// case that catches people out: the entry is on, but names a copy somewhere
/// else, so this one will never be the one Windows starts.
fn autostart_state() -> String {
    match (autostart::is_enabled(), autostart::registered_command()) {
        (true, _) => "on, and set to start this copy".into(),
        (false, Some(other)) => {
            format!("on, but set to start {other} — not this copy, so this one won't start by itself")
        }
        (false, None) => "off".into(),
    }
}

fn filetime_us(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64) / 10
}

/// The report, line by line.
pub fn report() -> String {
    let (started, bar, flyout) = STATS.with(|s| {
        let s = s.borrow();
        (s.started, s.bar, s.flyout)
    });
    let uptime = started.elapsed().as_secs_f64();
    let me = unsafe { GetCurrentProcess() };
    let mut mem = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let mem_ok =
        unsafe { K32GetProcessMemoryInfo(me, &mut mem as *mut _ as *mut PROCESS_MEMORY_COUNTERS, mem.cb).as_bool() };
    let (mut created, mut exited, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    let cpu_us = unsafe { GetProcessTimes(me, &mut created, &mut exited, &mut kernel, &mut user) }
        .map(|_| filetime_us(kernel) + filetime_us(user))
        .ok();
    let (gdi, gdi_peak, usr, usr_peak) = unsafe {
        (
            GetGuiResources(me, GR_GDIOBJECTS),
            GetGuiResources(me, GR_GDIOBJECTS_PEAK),
            GetGuiResources(me, GR_USEROBJECTS),
            GetGuiResources(me, GR_USEROBJECTS_PEAK),
        )
    };
    let mut handles = 0u32;
    let handles_ok = unsafe { GetProcessHandleCount(me, &mut handles) }.is_ok();
    let (apps, categories) = app::with(|s| {
        let mut n = 0;
        crate::tree::walk(&s.cfg.categories, &mut |_, _| n += 1);
        (s.catalog.apps.len(), n)
    });
    let mut lines = vec![
        format!("FlexTaskbar {}", env!("CARGO_PKG_VERSION")),
        format!("Running for {}", perf::duration(uptime as u64)),
        String::new(),
    ];
    if mem_ok {
        // Wine leaves the private bytes (and the counts below) at 0: say
        // nothing rather than something wrong.
        let private = match mem.PrivateUsage {
            0 => String::new(),
            n => format!(", {} private", perf::mb(n as u64)),
        };
        lines.push(format!(
            "Memory: {} in use (working set){private}, peak {}",
            perf::mb(mem.WorkingSetSize as u64),
            perf::mb(mem.PeakWorkingSetSize as u64)
        ));
    }
    if let Some(cpu) = cpu_us {
        lines.push(format!(
            "CPU time: {} in all, {} of one core on average",
            perf::ms(cpu),
            perf::cpu_percent(cpu, uptime)
        ));
    }
    if gdi > 0 || usr > 0 {
        lines.push(format!("GDI objects: {gdi} (peak {gdi_peak}), USER objects: {usr} (peak {usr_peak})"));
    }
    if handles_ok && handles > 0 {
        lines.push(format!("Handles: {handles}"));
    }
    lines.push(String::new());
    lines.push(format!("Drawing the bar: {}", bar.describe()));
    lines.push(format!("Drawing a flyout: {}", flyout.describe()));
    // When this copy started, and the ones before it: the only way to tell
    // afterwards whether "Start with Windows" really started it.
    lines.push(String::new());
    let now = crate::config::unix_time();
    let starts = read_starts();
    match starts.split_last() {
        Some((this, before)) => {
            lines.push(format!("This copy started {}", perf::describe_start(this, now)));
            if !before.is_empty() {
                lines.push("Started before that:".into());
                for s in before.iter().rev() {
                    lines.push(format!("  {}", perf::describe_start(s, now)));
                }
            }
        }
        None => lines.push("No record of this start (the data folder couldn't be written).".into()),
    }
    lines.push(format!("Start with Windows: {}", autostart_state()));

    lines.push(String::new());
    lines.push(format!("{apps} apps, {categories} categories"));
    lines.push("The supervisor process (crash recovery) is separate and not counted here.".into());
    lines.join("\n")
}

/// Shows the report and saves it to `FlexTaskbarData\diagnostics.txt`.
pub fn show() {
    let text = report();
    let file = paths::get().data.join("diagnostics.txt");
    let saved = std::fs::write(&file, &text).is_ok();
    let footer =
        if saved { format!("\n\nSaved to {}. Ctrl+C copies this message.", file.display()) } else { String::new() };
    ui::info(None, &format!("{text}{footer}"));
}
