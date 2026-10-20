//! This machine's minute look (docs/machine.md): the kernel's own figures, the processes worth naming with the
//! scope each runs under, and the standing kept of them, whose alerts are memory pressure that stands and the
//! disk under its line. What a scope stands for, and what to do about any of it, is the caller's.
//!
//! The kernel's files are its own and their shapes are its own, so they are read here line by line:
//! `/proc/pressure/*` as `some`/`full` lines of `name=value` pairs, `/proc/meminfo` as `Name: value kB`.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};

use serde_json::json;

use store::Store;
use store::settings::{MachineLimit, leading_float};

use super::js::round_to;
pub use bmo_node_client::machines::{
  Alert, KernelReading, Largest, Pressure, PressureLine, ScopeUse, Standing,
};

/// How often the watch looks.
pub const WATCH_MS: i64 = 60_000;

/// Where the minute look keeps the last standing it read.
pub const STANDING_KEY: &str = "machine_standing";

// Since when memory pressure has stood over the alert line.
const PRESSURE_SINCE_KEY: &str = "machine_pressure_since";

/// The variable naming another proc root, which is how a test reads a machine of its own.
pub const PROC_ROOT_VARIABLE: &str = "BMO_PROC_ROOT";

// A process below this holds too little to be worth asking whose it is, unless it is a browser.
const NAMED_FROM_MEGABYTES: f64 = 50.0;

// The names a Chrome or Chromium binary runs under, as the kernel shortens them to fifteen characters.
const BROWSER_NAMES: [&str; 5] =
  ["chrome", "chromium", "chromium-browse", "headless_shell", "google-chrome"];

/// How many of the largest processes the standing names.
const LARGEST: usize = 5;

const GIGABYTE: f64 = 1024.0 * 1024.0 * 1024.0;

fn pressure_line(fields: &[&str]) -> Option<PressureLine> {
  let values: BTreeMap<&str, &str> = fields.iter().filter_map(|pair| pair.split_once('=')).collect();
  let read = |name: &str| {
    values
      .get(name)
      .and_then(|value| leading_float(value))
      .filter(|value| value.is_finite())
  };

  Some(PressureLine {
    avg10: read("avg10")?,
    avg60: read("avg60")?,
    avg300: read("avg300")?,
  })
}

/// One pressure file's two lines, or none where the text is not one.
pub fn parse_pressure(text: &str) -> Option<Pressure> {
  let mut some = None;
  let mut full = None;

  for line in text.lines() {
    let words: Vec<&str> = line.split_whitespace().collect();

    match words.first() {
      Some(&"some") => some = pressure_line(&words[1..]),
      Some(&"full") => full = pressure_line(&words[1..]),
      _ => {}
    }
  }

  Some(Pressure { some: some?, full })
}

/// The `/proc/meminfo` figures by name, in kilobytes.
pub fn parse_meminfo(text: &str) -> BTreeMap<String, i64> {
  text
    .lines()
    .filter_map(|line| {
      let (name, rest) = line.split_once(':')?;
      let value = rest.split_whitespace().next()?.parse::<i64>().ok()?;

      Some((name.trim().to_owned(), value))
    })
    .collect()
}

fn gigabytes(kilobytes: i64) -> f64 {
  kilobytes as f64 / 1024.0 / 1024.0
}

/// The free space of the disk a path stands on, for a writer that is not the root user, in gigabytes.
fn disk_free_gb(path: &Path) -> Option<f64> {
  let named = CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
  // SAFETY: statvfs fills the zeroed structure it is handed and reads only the named path.
  let mut disk: libc::statvfs = unsafe { std::mem::zeroed() };

  // SAFETY: the path is a valid C string and the structure is ours for the call.
  (unsafe { libc::statvfs(named.as_ptr(), &mut disk) } == 0)
    .then(|| disk.f_bavail as f64 * disk.f_frsize as f64 / GIGABYTE)
}

/// The machine a proc root reads now, or none where its load or memory cannot be read; with the free
/// space of the disk `disk` stands on where a path is named.
pub fn read_machine(root: &Path, now_ms: i64, disk: Option<&Path>) -> Option<KernelReading> {
  let text = |name: &str| std::fs::read_to_string(root.join(name)).ok();
  let load1 = text("loadavg")?.split_whitespace().next()?.parse::<f64>().ok()?;
  let mem = parse_meminfo(&text("meminfo")?);
  // The logical CPUs the kernel counts, one `cpuN` line each.
  let threads = text("stat")
    .unwrap_or_default()
    .lines()
    .filter(|line| {
      line
        .strip_prefix("cpu")
        .and_then(|rest| rest.split_once(char::is_whitespace))
        .is_some_and(|(number, _)| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
    })
    .count() as i64;
  let (Some(&total), Some(&available)) = (mem.get("MemTotal"), mem.get("MemAvailable")) else {
    return None;
  };

  if !load1.is_finite() || threads == 0 {
    return None;
  }

  let pressure = |name: &str| text(&format!("pressure/{name}")).and_then(|text| parse_pressure(&text));

  Some(KernelReading {
    at: now_ms,
    load1,
    threads,
    mem_total_gb: gigabytes(total),
    mem_available_gb: gigabytes(available),
    swap_total_gb: gigabytes(mem.get("SwapTotal").copied().unwrap_or(0)),
    swap_free_gb: gigabytes(mem.get("SwapFree").copied().unwrap_or(0)),
    disk_free_gb: disk.and_then(disk_free_gb),
    memory: pressure("memory"),
    cpu: pressure("cpu"),
  })
}

/// One process worth naming: a browser, or one holding enough memory to matter.
#[derive(Clone, Debug, PartialEq)]
pub struct Process {
  pub pid: i64,
  pub name: String,
  /// Resident memory and swap together, in megabytes.
  pub megabytes: f64,
  pub browser: bool,
  /// The scope its environment names, as it stands there.
  pub scope: Option<String>,
}

fn status_kilobytes(status: &str, name: &str) -> f64 {
  status
    .lines()
    .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
    .and_then(|rest| rest.split_whitespace().next()?.parse::<f64>().ok())
    .unwrap_or(0.0)
}

/// Every process worth naming — every browser main process, and every process holding 50 MB or more in
/// memory and swap — with the scope its environment names under `scope_variable`, where one is named. A
/// process gone between the listing and its reading is left out.
pub fn read_processes(root: &Path, scope_variable: Option<&str>) -> Vec<Process> {
  let Ok(entries) = std::fs::read_dir(root) else {
    return vec![];
  };
  let mut found = Vec::new();

  for entry in entries.flatten() {
    let pid = entry.file_name().to_string_lossy().into_owned();

    if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
      continue;
    }

    let Ok(status) = std::fs::read_to_string(root.join(&pid).join("status")) else {
      continue;
    };
    let name = status
      .lines()
      .next()
      .map(|line| line.trim_start_matches("Name:").trim().to_owned())
      .unwrap_or_default();
    let megabytes = (status_kilobytes(&status, "VmRSS") + status_kilobytes(&status, "VmSwap")) / 1024.0;
    // A browser's main process: its renderers, GPU and utility helpers all carry a `--type=` argument.
    let browser = BROWSER_NAMES.contains(&name.as_str())
      && std::fs::read(root.join(&pid).join("cmdline"))
        .is_ok_and(|args| !args.split(|&byte| byte == 0).any(|arg| arg.starts_with(b"--type=")));

    if !browser && megabytes < NAMED_FROM_MEGABYTES {
      continue;
    }

    found.push(Process {
      pid: pid.parse().unwrap_or_default(),
      scope: scope_variable.and_then(|variable| scope_of(&root.join(&pid).join("environ"), variable)),
      name,
      megabytes,
      browser,
    });
  }

  found
}

// The scope a process's environment names under the variable, as it stands there; none where it is empty.
fn scope_of(environ: &Path, variable: &str) -> Option<String> {
  let environ = std::fs::read(environ).ok()?;
  let prefix = format!("{variable}=");
  let entry = environ.split(|&byte| byte == 0).find(|pair| pair.starts_with(prefix.as_bytes()))?;
  let scope = String::from_utf8_lossy(&entry[prefix.len()..]).into_owned();

  (!scope.is_empty()).then_some(scope)
}

/// The memory pressure the limits are held to: the share of the last minute every task stood stalled.
pub fn memory_full(reading: Option<&KernelReading>) -> Option<f64> {
  reading?.memory.as_ref()?.full.as_ref().map(|full| full.avg60)
}

/// The three limits as the store holds them (docs/machine.md, 「The limits」).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
  pub alert_memory_full_pct: f64,
  pub alert_minutes: f64,
  pub alert_disk_free_gb: f64,
}

impl Limits {
  /// Each limit as its setting holds it, the default where none that is a positive number is set.
  pub fn read(store: &Store) -> store::Result<Self> {
    Ok(Self {
      alert_memory_full_pct: store.machine_limit(MachineLimit::AlertMemoryFullPct)?,
      alert_minutes: store.machine_limit(MachineLimit::AlertMinutes)?,
      alert_disk_free_gb: store.machine_limit(MachineLimit::AlertDiskFreeGb)?,
    })
  }

  pub fn defaults() -> Self {
    let value = MachineLimit::default_value;

    Self {
      alert_memory_full_pct: value(MachineLimit::AlertMemoryFullPct),
      alert_minutes: value(MachineLimit::AlertMinutes),
      alert_disk_free_gb: value(MachineLimit::AlertDiskFreeGb),
    }
  }
}

/// The machine as read: what each scope and the largest processes hold, and no alert yet, which only the
/// minute look, holding how long a pressure has stood, can tell.
pub fn judge(reading: Option<KernelReading>, processes: &[Process]) -> Standing {
  let mut scopes: Vec<(String, f64, i64)> = Vec::new();

  for process in processes {
    let Some(scope) = &process.scope else {
      continue;
    };

    match scopes.iter_mut().find(|(held, ..)| held == scope) {
      Some(found) => {
        found.1 += process.megabytes;
        found.2 += i64::from(process.browser);
      }
      None => scopes.push((scope.clone(), process.megabytes, i64::from(process.browser))),
    }
  }

  let mut scopes: Vec<ScopeUse> = scopes
    .into_iter()
    .map(|(scope, megabytes, browsers)| ScopeUse {
      scope,
      megabytes: megabytes.round() as i64,
      browsers,
    })
    .collect();

  scopes.sort_by_key(|scope| std::cmp::Reverse(scope.megabytes));

  let mut largest: Vec<&Process> = processes.iter().collect();

  largest.sort_by(|a, b| b.megabytes.total_cmp(&a.megabytes));

  Standing {
    reading,
    alerts: Vec::new(),
    browsers: processes.iter().filter(|process| process.browser).count() as i64,
    scopes,
    largest: largest
      .into_iter()
      .take(LARGEST)
      .map(|process| Largest {
        pid: process.pid,
        name: process.name.clone(),
        megabytes: process.megabytes.round() as i64,
        scope: process.scope.clone(),
      })
      .collect(),
  }
}

/// The proc root the machine is read from: the one `BMO_PROC_ROOT` names, `/proc` otherwise.
pub fn proc_root() -> PathBuf {
  std::env::var_os(PROC_ROOT_VARIABLE).map_or_else(|| PathBuf::from("/proc"), PathBuf::from)
}

/// This machine read now, the data root's disk among it, each process's scope read under `scope_variable`.
pub fn here(proc_root: &Path, data_root: &Path, scope_variable: Option<&str>, now_ms: i64) -> Standing {
  judge(
    read_machine(proc_root, now_ms, Some(data_root)),
    &read_processes(proc_root, scope_variable),
  )
}

/// The standing the minute look last kept, where it kept one.
pub fn kept(store: &Store) -> store::Result<Option<Standing>> {
  Ok(store.setting(STANDING_KEY)?.and_then(|text| serde_json::from_str(&text).ok()))
}

/// What one look at the machine came to: the standing kept, and whether its alerts moved since the one kept
/// before, an alert starting or ending.
pub struct Looked {
  pub standing: Standing,
  pub alerts_moved: bool,
}

/// One look at the machine standing as it was read: its alerts told against the store's limits, and the
/// standing kept for the next look and for every reader.
pub fn look(store: &Store, mut standing: Standing, now_ms: i64) -> store::Result<Looked> {
  let limits = Limits::read(store)?;
  let causes = |standing: &Standing| standing.alerts.iter().map(Alert::cause).collect::<Vec<_>>();
  let before = kept(store)?.map(|kept| causes(&kept)).unwrap_or_default();

  standing.alerts = memory_alert(store, &standing, &limits, now_ms)?
    .into_iter()
    .chain(disk_alert(&standing, &limits))
    .collect();
  store.set_setting(STANDING_KEY, &json!(standing).to_string())?;

  Ok(Looked {
    alerts_moved: causes(&standing) != before,
    standing,
  })
}

fn number(store: &Store, key: &str) -> store::Result<Option<i64>> {
  Ok(store.setting(key)?.and_then(|value| value.trim().parse::<i64>().ok()))
}

// Memory `full` at or above the alert line once it has stood for the alert minutes, counted from the first
// look that found it there.
fn memory_alert(
  store: &Store,
  standing: &Standing,
  limits: &Limits,
  now_ms: i64,
) -> store::Result<Option<Alert>> {
  let alert = limits.alert_memory_full_pct;
  let Some(full) = memory_full(standing.reading.as_ref()).filter(|&full| full >= alert) else {
    store.remove_setting(PRESSURE_SINCE_KEY)?;

    return Ok(None);
  };
  let since = number(store, PRESSURE_SINCE_KEY)?;

  if since.is_none() {
    store.set_setting(PRESSURE_SINCE_KEY, &now_ms.to_string())?;
  }

  // The first reading over the line already covers the minute before it.
  let stood_ms = since.map_or(0, |since| now_ms - since) + WATCH_MS;

  if (stood_ms as f64) < limits.alert_minutes * 60_000.0 {
    return Ok(None);
  }

  Ok(Some(Alert::Memory {
    full_pct: full,
    alert_pct: alert,
    minutes: (stood_ms as f64 / 60_000.0).round() as i64,
  }))
}

// The disk the data root stands on under its line.
fn disk_alert(standing: &Standing, limits: &Limits) -> Option<Alert> {
  let line = limits.alert_disk_free_gb;
  let free = standing.reading.as_ref()?.disk_free_gb.filter(|&free| free < line)?;

  Some(Alert::Disk {
    free_gb: round_to(free, 10.0),
    limit_gb: line,
  })
}
