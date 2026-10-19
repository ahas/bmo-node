//! This machine's minute look (docs/machine.md): the kernel's own figures, the processes worth naming with
//! the agent place each runs under, the launch holds on the four hardware limits, the standing a console
//! shows, and memory pressure that stands and the disk under its line, each told once as a
//! `machine-pressure` event. Which job a place serves, and what to do about any of it, is the caller's.
//!
//! The kernel's files are its own and their shapes are its own, so they are read here line by line:
//! `/proc/pressure/*` as `some`/`full` lines of `name=value` pairs, `/proc/meminfo` as `Name: value kB`.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};

use serde_json::json;

pub use bmo_node_client::events::{Event, MachinePressure};
use store::Store;
use store::settings::{MachineLimit, leading_float};

use super::js::{round_to, shown};
pub use bmo_node_client::machine::{Largest, Place, Pressure, PressureLine, Reading, Standing};

/// How often the watch looks.
pub const WATCH_MS: i64 = 60_000;

/// Where the minute look keeps the last standing it read.
pub const STANDING_KEY: &str = "machine_standing";

// Whether the disk standing under the launch line has been told, for as long as it stays there.
const DISK_TOLD_KEY: &str = "machine_disk_told";
// Since when memory pressure has stood over the alert line, and whether that stretch has been told.
const PRESSURE_SINCE_KEY: &str = "machine_pressure_since";
const PRESSURE_TOLD_KEY: &str = "machine_pressure_told";

/// The variable an agent's pane names its place under, which every process under it inherits.
pub const PLACE_VARIABLE: &str = "BMO_AGENT_PLACE";

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

/// A figure to a tenth, written without a point where it is whole.
fn tenth(value: f64) -> String {
  shown(round_to(value, 10.0))
}

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
pub fn read_machine(root: &Path, now_ms: i64, disk: Option<&Path>) -> Option<Reading> {
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

  Some(Reading {
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
  /// The agent place its environment names, by the place's own name.
  pub place: Option<String>,
}

fn status_kilobytes(status: &str, name: &str) -> f64 {
  status
    .lines()
    .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
    .and_then(|rest| rest.split_whitespace().next()?.parse::<f64>().ok())
    .unwrap_or(0.0)
}

/// Every process worth naming — every browser main process, and every process holding 50 MB or more in
/// memory and swap — with the agent place its environment names. A process gone between the listing
/// and its reading is left out.
pub fn read_processes(root: &Path) -> Vec<Process> {
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
      place: place_of(&root.join(&pid).join("environ")),
      name,
      megabytes,
      browser,
    });
  }

  found
}

// The agent place a process's environment names, by the place directory's own name.
fn place_of(environ: &Path) -> Option<String> {
  let environ = std::fs::read(environ).ok()?;
  let prefix = format!("{PLACE_VARIABLE}=");
  let entry = environ.split(|&byte| byte == 0).find(|pair| pair.starts_with(prefix.as_bytes()))?;
  let place = String::from_utf8_lossy(&entry[prefix.len()..]).trim_end_matches('/').to_owned();
  let own = place.rsplit('/').next().unwrap_or_default();

  (!own.is_empty()).then(|| own.to_owned())
}

/// The memory pressure the limits are held to: the share of the last minute every task stood stalled.
pub fn memory_full(reading: Option<&Reading>) -> Option<f64> {
  reading?.memory.as_ref()?.full.as_ref().map(|full| full.avg60)
}

/// The six limits as the store holds them (docs/machine.md, 「The limits」).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
  pub launch_memory_full_pct: f64,
  pub launch_memory_available_gb: f64,
  pub launch_load_per_thread: f64,
  pub launch_disk_free_gb: f64,
  pub alert_memory_full_pct: f64,
  pub alert_minutes: f64,
}

impl Limits {
  /// Each limit as its setting holds it, the default where none that is a positive number is set.
  pub fn read(store: &Store) -> store::Result<Self> {
    Ok(Self {
      launch_memory_full_pct: store.machine_limit(MachineLimit::LaunchMemoryFullPct)?,
      launch_memory_available_gb: store.machine_limit(MachineLimit::LaunchMemoryAvailableGb)?,
      launch_load_per_thread: store.machine_limit(MachineLimit::LaunchLoadPerThread)?,
      launch_disk_free_gb: store.machine_limit(MachineLimit::LaunchDiskFreeGb)?,
      alert_memory_full_pct: store.machine_limit(MachineLimit::AlertMemoryFullPct)?,
      alert_minutes: store.machine_limit(MachineLimit::AlertMinutes)?,
    })
  }

  pub fn defaults() -> Self {
    let value = MachineLimit::default_value;

    Self {
      launch_memory_full_pct: value(MachineLimit::LaunchMemoryFullPct),
      launch_memory_available_gb: value(MachineLimit::LaunchMemoryAvailableGb),
      launch_load_per_thread: value(MachineLimit::LaunchLoadPerThread),
      launch_disk_free_gb: value(MachineLimit::LaunchDiskFreeGb),
      alert_memory_full_pct: value(MachineLimit::AlertMemoryFullPct),
      alert_minutes: value(MachineLimit::AlertMinutes),
    }
  }
}

/// Every one of the four launch limits a reading meets, each named with its figure and its limit; nothing
/// where the machine could not be read (docs/api.md, 「Agency's calls」).
pub fn held(reading: Option<&Reading>, limits: &Limits) -> Vec<String> {
  let Some(reading) = reading else {
    return vec![];
  };
  let mut held = Vec::new();

  if let Some(full) = memory_full(Some(reading)).filter(|&full| full >= limits.launch_memory_full_pct) {
    held.push(format!("memory pressure {}% >= {}%", tenth(full), shown(limits.launch_memory_full_pct)));
  }

  if reading.mem_available_gb < limits.launch_memory_available_gb {
    held.push(format!(
      "available memory {}GB < {}GB",
      tenth(reading.mem_available_gb),
      shown(limits.launch_memory_available_gb)
    ));
  }

  if reading.load1 / reading.threads as f64 >= limits.launch_load_per_thread {
    held.push(format!(
      "load {} >= {} threads x {}",
      tenth(reading.load1),
      reading.threads,
      shown(limits.launch_load_per_thread)
    ));
  }

  if let Some(free) = reading.disk_free_gb.filter(|&free| free < limits.launch_disk_free_gb) {
    held.push(format!("disk free {}GB < {}GB", tenth(free), shown(limits.launch_disk_free_gb)));
  }

  held
}

/// The machine judged against the limits: what is held, and what each place and the largest processes
/// hold.
pub fn judge(reading: Option<Reading>, processes: &[Process], limits: &Limits) -> Standing {
  let mut places: Vec<(String, f64, i64)> = Vec::new();

  for process in processes {
    let Some(place) = &process.place else {
      continue;
    };

    match places.iter_mut().find(|(held, ..)| held == place) {
      Some(found) => {
        found.1 += process.megabytes;
        found.2 += i64::from(process.browser);
      }
      None => places.push((place.clone(), process.megabytes, i64::from(process.browser))),
    }
  }

  let mut places: Vec<Place> = places
    .into_iter()
    .map(|(place, megabytes, browsers)| Place {
      place,
      megabytes: megabytes.round() as i64,
      browsers,
    })
    .collect();

  places.sort_by_key(|place| std::cmp::Reverse(place.megabytes));

  let mut largest: Vec<&Process> = processes.iter().collect();

  largest.sort_by(|a, b| b.megabytes.total_cmp(&a.megabytes));

  Standing {
    held: held(reading.as_ref(), limits),
    reading,
    browsers: processes.iter().filter(|process| process.browser).count() as i64,
    places,
    largest: largest
      .into_iter()
      .take(LARGEST)
      .map(|process| Largest {
        pid: process.pid,
        name: process.name.clone(),
        megabytes: process.megabytes.round() as i64,
        place: process.place.clone(),
      })
      .collect(),
  }
}

/// The proc root the machine is read from: the one `BMO_PROC_ROOT` names, `/proc` otherwise.
pub fn proc_root() -> PathBuf {
  std::env::var_os(PROC_ROOT_VARIABLE).map_or_else(|| PathBuf::from("/proc"), PathBuf::from)
}

/// Where a machine-pressure event goes; the server publishes it, a test keeps it.
pub trait Publish: Send + Sync {
  fn publish(&self, event: Event);
}

/// This machine read now and judged against the store's limits, the data root's disk among it.
pub fn here(store: &Store, proc_root: &Path, data_root: &Path, now_ms: i64) -> store::Result<Standing> {
  let limits = Limits::read(store)?;

  Ok(judge(
    read_machine(proc_root, now_ms, Some(data_root)),
    &read_processes(proc_root),
    &limits,
  ))
}

/// The standing the minute look last kept, where it kept one.
pub fn kept(store: &Store) -> store::Result<Option<Standing>> {
  Ok(store.setting(STANDING_KEY)?.and_then(|text| serde_json::from_str(&text).ok()))
}

/// One look at the machine standing as it was read: the standing kept for the console, memory pressure
/// that stood for the alert minutes told once, and the disk under the launch line told once.
pub fn look(store: &Store, standing: &Standing, now_ms: i64, publish: &dyn Publish) -> store::Result<()> {
  let limits = Limits::read(store)?;

  store.set_setting(STANDING_KEY, &json!(standing).to_string())?;
  watch_pressure(store, standing, &limits, now_ms, publish)?;
  watch_disk(store, standing, &limits, publish)
}

fn number(store: &Store, key: &str) -> store::Result<Option<i64>> {
  Ok(store.setting(key)?.and_then(|value| value.trim().parse::<i64>().ok()))
}

fn watch_pressure(
  store: &Store,
  standing: &Standing,
  limits: &Limits,
  now_ms: i64,
  publish: &dyn Publish,
) -> store::Result<()> {
  let alert = limits.alert_memory_full_pct;
  let Some(full) = memory_full(standing.reading.as_ref()).filter(|&full| full >= alert) else {
    store.remove_setting(PRESSURE_SINCE_KEY)?;
    store.remove_setting(PRESSURE_TOLD_KEY)?;

    return Ok(());
  };
  let since = number(store, PRESSURE_SINCE_KEY)?;

  if since.is_none() {
    store.set_setting(PRESSURE_SINCE_KEY, &now_ms.to_string())?;
  }

  // The first reading over the line already covers the minute before it.
  let stood_ms = since.map_or(0, |since| now_ms - since) + WATCH_MS;

  if (stood_ms as f64) < limits.alert_minutes * 60_000.0 || store.setting(PRESSURE_TOLD_KEY)?.is_some() {
    return Ok(());
  }

  store.set_setting(PRESSURE_TOLD_KEY, &now_ms.to_string())?;
  publish.publish(Event::MachinePressure(MachinePressure::Memory {
    full_pct: full,
    alert_pct: alert,
    minutes: (stood_ms as f64 / 60_000.0).round() as i64,
    reading: standing.reading.clone(),
    places: standing.places.clone(),
    largest: standing.largest.clone(),
  }));

  Ok(())
}

// The disk the data root stands on under the launch line, told once for the stretch it stays there.
fn watch_disk(
  store: &Store,
  standing: &Standing,
  limits: &Limits,
  publish: &dyn Publish,
) -> store::Result<()> {
  let line = limits.launch_disk_free_gb;
  let Some(free) = standing
    .reading
    .as_ref()
    .and_then(|reading| reading.disk_free_gb)
    .filter(|&free| free < line)
  else {
    store.remove_setting(DISK_TOLD_KEY)?;

    return Ok(());
  };

  if store.setting(DISK_TOLD_KEY)?.is_some() {
    return Ok(());
  }

  store.set_setting(DISK_TOLD_KEY, "1")?;
  publish.publish(Event::MachinePressure(MachinePressure::Disk {
    free_gb: round_to(free, 10.0),
    limit_gb: line,
    reading: standing.reading.clone(),
  }));

  Ok(())
}
