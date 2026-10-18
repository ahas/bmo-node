//! What the probe printed, turned into figures (docs/hardware.md, 「The probe」): section by section,
//! never guessing a value the machine did not print, a figure it cannot find none. The sections are
//! the probe's own syntax — a line `@@<name>` heading the raw text under it — and each section's text is
//! the kernel's or a tool's own, read line by line.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Serialize, Serializer};

use super::js::{number, parse_float, round, round_to};
use super::storage::{StorageDevice, read_linux_storage, windows_storage};
use super::windows::read_windows;

/// How long the probe run once waits between its two readings, which a reading without its own
/// stretch is taken across.
const PROBE_SAMPLE_SECONDS: f64 = 0.5;

/// The parts of a live reading that hardly change, printed only every so many readings.
const LIVE_SELDOM: [&str; 10] =
  ["host", "kernel", "version", "os", "model", "layout", "blk", "blkdev", "df", "services"];

/// What the probe printed, section by section: each head's lines, a later section of a name standing in
/// place of an earlier one.
pub type Sections = BTreeMap<String, Vec<String>>;

static HEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@@([a-z0-9]+)$").expect("the head pattern"));

pub fn sections(text: &str) -> Sections {
  let mut found = Sections::new();
  let mut current: Option<String> = None;

  for line in text.split('\n') {
    if let Some(head) = HEAD.captures(line.trim()) {
      let name = head[1].to_owned();

      found.insert(name.clone(), vec![]);
      current = Some(name);
    } else if let Some(name) = &current
      && !line.trim().is_empty()
    {
      found.entry(name.clone()).or_default().push(line.to_owned());
    }
  }

  found
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuThread {
  pub cpu: f64,
  pub package: Option<f64>,
  pub core: Option<f64>,
  /// The CPUs this one shares its L3 with, as the kernel lists them.
  pub l3: Option<String>,
  pub l3_size: Option<String>,
  /// Share of the sample this CPU was busy, 0 to 1.
  pub usage: Option<f64>,
  pub mhz: Option<f64>,
}

impl CpuThread {
  fn new(cpu: f64) -> Self {
    Self {
      cpu,
      package: None,
      core: None,
      l3: None,
      l3_size: None,
      usage: None,
      mhz: None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sensor {
  pub chip: String,
  /// The device the chip belongs to, where the kernel names one.
  pub device: Option<String>,
  pub label: String,
  pub celsius: f64,
  /// The critical temperature the chip names for this input, or the highest where it names no critical
  /// one (docs/hardware.md, 「Temperature limits」).
  pub limit_celsius: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Fan {
  pub chip: String,
  pub label: String,
  pub rpm: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
  pub index: f64,
  pub name: String,
  pub usage: Option<f64>,
  pub celsius: Option<f64>,
  pub mhz: Option<f64>,
  pub max_mhz: Option<f64>,
  pub memory_mhz: Option<f64>,
  pub watts: Option<f64>,
  pub watt_limit: Option<f64>,
  pub memory_used_mb: Option<f64>,
  pub memory_total_mb: Option<f64>,
  pub fan_percent: Option<f64>,
  /// The temperature the GPU slows itself down at, and whether thermal slowdown — the hardware's or the
  /// driver's — holds its clocks down now; none where the driver did not say.
  pub limit_celsius: Option<f64>,
  pub throttling: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServiceCount {
  pub running: f64,
  pub failed: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Memory {
  pub total_kb: f64,
  pub available_kb: f64,
  pub swap_total_kb: f64,
  pub swap_free_kb: f64,
}

/// Package power over the sample: watts, or the counter refusing the account.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Watts {
  Read(f64),
  Denied,
}

impl Serialize for Watts {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    match self {
      Self::Read(watts) => serializer.serialize_f64(*watts),
      Self::Denied => serializer.serialize_str("denied"),
    }
  }
}

/// Everything the probe printed, as figures.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
  pub host: Option<String>,
  pub kernel: Option<String>,
  pub wsl: bool,
  pub os: Option<String>,
  /// The Windows a WSL machine runs on, by edition and build.
  pub host_os: Option<String>,
  /// Whether a WSL machine's Windows has its sensor library in place and opened.
  pub sensor_library: Option<&'static str>,
  pub uptime_seconds: Option<f64>,
  pub load: Option<[f64; 3]>,
  pub memory: Option<Memory>,
  pub cpu_model: Option<String>,
  pub cpu_usage: Option<f64>,
  pub clock_source: Option<&'static str>,
  pub threads: Vec<CpuThread>,
  pub cpu_watts: Option<Watts>,
  pub sensors: Vec<Sensor>,
  pub fans: Vec<Fan>,
  pub gpus: Vec<Gpu>,
  pub storage: Vec<StorageDevice>,
  pub services: (Option<ServiceCount>, Option<ServiceCount>),
}

fn lines<'a>(found: &'a Sections, name: &str) -> &'a [String] {
  found.get(name).map_or(&[], Vec::as_slice)
}

/// A section's first line, trimmed, where it holds anything.
fn first<'a>(found: &'a Sections, name: &str) -> Option<&'a str> {
  lines(found, name).first().map(|line| line.trim()).filter(|line| !line.is_empty())
}

fn read_load(lines: &[String]) -> Option<[f64; 3]> {
  let parts: Vec<Option<f64>> = lines
    .first()
    .map(|line| line.split_whitespace().take(3).map(parse_float).collect())
    .unwrap_or_default();

  match parts.as_slice() {
    [Some(one), Some(five), Some(fifteen)] => Some([*one, *five, *fifteen]),
    _ => None,
  }
}

static MEMORY_LINE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(\w+):\s+(\d+)").expect("the memory pattern"));

fn read_memory(lines: &[String]) -> Option<Memory> {
  let kb: BTreeMap<&str, f64> = lines
    .iter()
    .filter_map(|line| {
      let found = MEMORY_LINE.captures(line)?;

      Some((found.get(1)?.as_str(), found[2].parse().ok()?))
    })
    .collect();

  Some(Memory {
    total_kb: *kb.get("MemTotal")?,
    available_kb: *kb.get("MemAvailable")?,
    swap_total_kb: kb.get("SwapTotal").copied().unwrap_or(0.0),
    swap_free_kb: kb.get("SwapFree").copied().unwrap_or(0.0),
  })
}

/// One `/proc/stat` reading: each CPU's busy and total jiffies, keyed by `cpu` for the whole and
/// `cpu<n>` for each.
fn read_stat(lines: &[String]) -> BTreeMap<String, (f64, f64)> {
  let mut found = BTreeMap::new();

  for line in lines {
    let mut words = line.split_whitespace();
    let Some(name) = words.next() else {
      continue;
    };
    let values: Vec<Option<f64>> = words.map(number).collect();

    if !name.starts_with("cpu") || values.len() < 4 || values.iter().any(Option::is_none) {
      continue;
    }

    let values: Vec<f64> = values.into_iter().flatten().collect();
    let idle = values[3] + values.get(4).copied().unwrap_or(0.0);
    let total: f64 = values.iter().take(8).sum();

    found.insert(name.to_owned(), (total - idle, total));
  }

  found
}

fn usage_between(
  first: &BTreeMap<String, (f64, f64)>,
  second: &BTreeMap<String, (f64, f64)>,
  name: &str,
) -> Option<f64> {
  let (busy_a, total_a) = first.get(name)?;
  let (busy_b, total_b) = second.get(name)?;

  (total_b > total_a).then(|| ((busy_b - busy_a) / (total_b - total_a)).clamp(0.0, 1.0))
}

static TOPOLOGY_LINE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"/cpu(\d+)/(topology/physical_package_id|topology/core_id|cache/index3/shared_cpu_list|cache/index3/size|cpufreq/scaling_cur_freq):(.*)$")
    .expect("the topology pattern")
});

static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("the spaces pattern"));

/// The threads in the order the kernel numbered them, the CPU's model, and where the clocks came from.
fn read_threads(found: &Sections, wsl: bool) -> (Vec<CpuThread>, Option<String>, Option<&'static str>) {
  let mut threads: Vec<CpuThread> = Vec::new();
  let mut model: Option<String> = None;
  let mut processor: Option<f64> = None;
  let mut info_mhz: Vec<(f64, f64)> = Vec::new();

  fn thread(threads: &mut Vec<CpuThread>, cpu: f64) -> &mut CpuThread {
    match threads.iter().position(|one| one.cpu == cpu) {
      Some(at) => &mut threads[at],
      None => {
        threads.push(CpuThread::new(cpu));
        threads.last_mut().expect("the thread just made")
      }
    }
  }

  for line in lines(found, "cpuinfo") {
    let mut parts = line.split(':').map(str::trim);
    let key = parts.next().unwrap_or_default();
    let value = parts.next();

    match key {
      "processor" => {
        processor = value.and_then(parse_float);

        if let Some(cpu) = processor {
          thread(&mut threads, cpu);
        }
      }
      "model name" if model.is_none() && value.is_some_and(|value| !value.is_empty()) => {
        model = value.map(|value| SPACES.replace_all(value, " ").into_owned());
      }
      "cpu MHz" => {
        if let (Some(cpu), Some(mhz)) = (processor, value.and_then(parse_float)) {
          match info_mhz.iter_mut().find(|(one, _)| *one == cpu) {
            Some(known) => known.1 = mhz,
            None => info_mhz.push((cpu, mhz)),
          }
        }
      }
      _ => {}
    }
  }

  let mut from_cpufreq = false;

  for line in lines(found, "topology") {
    let Some(matched) = TOPOLOGY_LINE.captures(line) else {
      continue;
    };
    let Ok(cpu) = matched[1].parse::<f64>() else {
      continue;
    };
    let value = matched[3].trim();
    let one = thread(&mut threads, cpu);

    match &matched[2] {
      "topology/physical_package_id" => one.package = parse_float(value),
      "topology/core_id" => one.core = parse_float(value),
      "cache/index3/shared_cpu_list" => one.l3 = Some(value.to_owned()),
      "cache/index3/size" => one.l3_size = Some(value.to_owned()),
      _ => {
        if let Some(khz) = parse_float(value) {
          one.mhz = Some(round(khz / 1000.0));
          from_cpufreq = true;
        }
      }
    }
  }

  let mut source = from_cpufreq.then_some("cpufreq");

  // WSL writes a fixed figure into /proc/cpuinfo, which would read as a clock that never moves.
  if !from_cpufreq && !wsl && !info_mhz.is_empty() {
    source = Some("cpuinfo");

    for (cpu, mhz) in info_mhz {
      thread(&mut threads, cpu).mhz = Some(round(mhz));
    }
  }

  let before = read_stat(lines(found, "stat1"));
  let after = read_stat(lines(found, "stat2"));

  for one in &mut threads {
    one.usage = usage_between(&before, &after, &format!("cpu{}", super::js::shown(one.cpu)));
  }

  threads.sort_by(|a, b| a.cpu.total_cmp(&b.cpu));

  (threads, model, source)
}

/// Package power across the stretch the two energy readings stand apart: the `interval` section's
/// seconds where the output says it, as the live run's does, and the probe's own wait otherwise.
fn read_watts(found: &Sections) -> Option<Watts> {
  let first = lines(found, "rapl1").first()?.trim();
  let second = lines(found, "rapl2").first()?.trim();
  let seconds = lines(found, "interval")
    .first()
    .and_then(|line| parse_float(line))
    .unwrap_or(PROBE_SAMPLE_SECONDS);

  if seconds <= 0.0 {
    return None;
  }

  if first == "denied" || second == "denied" {
    return Some(Watts::Denied);
  }

  let (a, b) = (parse_float(first)?, parse_float(second)?);

  // The counter wraps; a sample across the wrap says nothing.
  (b >= a).then(|| Watts::Read(round_to((b - a) / 1_000_000.0 / seconds, 10.0)))
}

/// A board input nothing is wired to reads nought, -128 or a byte's top; such a figure is no
/// temperature.
pub fn plausible_celsius(celsius: f64) -> bool {
  celsius > 0.0 && celsius < 127.0
}

static CHIP_HEAD: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^chip (\S+) (\S*) ?(\S*)$").expect("the chip pattern"));
static CHIP_READING: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"^(.*)/(temp|fan)(\d+)_(input|label|crit|max):(.*)$").expect("the reading pattern")
});

#[derive(Default)]
struct Entry {
  input: Option<f64>,
  label: Option<String>,
  crit: Option<f64>,
  max: Option<f64>,
}

/// A limit hwmon gives in thousandths of a degree, as a temperature; a figure no part could run at is no
/// limit.
fn hwmon_limit(millidegrees: Option<f64>) -> Option<f64> {
  millidegrees
    .filter(|one| plausible_celsius(one / 1000.0))
    .map(|one| round(one / 100.0) / 10.0)
}

struct Chip {
  path: String,
  name: String,
  device: Option<String>,
  temps: Vec<(String, Entry)>,
  fans: Vec<(String, Entry)>,
}

fn entry<'a>(table: &'a mut Vec<(String, Entry)>, index: &str) -> &'a mut Entry {
  match table.iter().position(|(one, _)| one == index) {
    Some(at) => &mut table[at].1,
    None => {
      table.push((index.to_owned(), Entry::default()));
      &mut table.last_mut().expect("the entry just made").1
    }
  }
}

fn read_hwmon(lines: &[String]) -> (Vec<Sensor>, Vec<Fan>) {
  let mut chips: Vec<Chip> = Vec::new();

  for line in lines {
    if let Some(head) = CHIP_HEAD.captures(line.trim()) {
      let chip = Chip {
        path: head[1].to_owned(),
        name: Some(&head[2]).filter(|name| !name.is_empty()).unwrap_or("hwmon").to_owned(),
        device: Some(&head[3]).filter(|device| !device.is_empty()).map(|device| (*device).to_owned()),
        temps: vec![],
        fans: vec![],
      };

      match chips.iter().position(|one| one.path == chip.path) {
        Some(at) => chips[at] = chip,
        None => chips.push(chip),
      }

      continue;
    }

    let Some(reading) = CHIP_READING.captures(line) else {
      continue;
    };
    let Some(chip) = chips.iter_mut().find(|chip| chip.path == reading[1]) else {
      continue;
    };
    let table = if &reading[2] == "temp" { &mut chip.temps } else { &mut chip.fans };
    let one = entry(table, &reading[3]);

    match &reading[4] {
      "label" => one.label = Some(reading[5].trim().to_owned()),
      "crit" => one.crit = parse_float(&reading[5]),
      "max" => one.max = parse_float(&reading[5]),
      _ => one.input = parse_float(&reading[5]),
    }
  }

  let mut sensors = Vec::new();
  let mut fans = Vec::new();

  for chip in &chips {
    for (index, one) in &chip.temps {
      let Some(input) = one.input.filter(|input| plausible_celsius(input / 1000.0)) else {
        continue;
      };

      sensors.push(Sensor {
        chip: chip.name.clone(),
        device: chip.device.clone(),
        label: one.label.clone().unwrap_or_else(|| format!("temp{index}")),
        celsius: round(input / 100.0) / 10.0,
        limit_celsius: hwmon_limit(one.crit.or(one.max)),
      });
    }

    for (index, one) in &chip.fans {
      let Some(input) = one.input else {
        continue;
      };

      fans.push(Fan {
        chip: chip.name.clone(),
        label: one.label.clone().unwrap_or_else(|| format!("fan{index}")),
        rpm: input,
      });
    }
  }

  (sensors, fans)
}

/// Whether one of the slowdown columns says it holds the clocks down; none where the driver did not say.
fn slowdown(cell: Option<&&str>) -> Option<bool> {
  match cell {
    Some(&"Active") => Some(true),
    Some(&"Not Active") => Some(false),
    _ => None,
  }
}

/// An element's first child of a name.
fn child<'a, 'input>(node: roxmltree::Node<'a, 'input>, name: &str) -> Option<roxmltree::Node<'a, 'input>> {
  node.children().find(|one| one.has_tag_name(name))
}

/// The temperature each GPU slows down at, in the order nvidia-smi lists them, out of its XML report;
/// none where there is no report or it does not say.
fn report_limits(lines: &[String]) -> Vec<Option<f64>> {
  if lines.is_empty() {
    return vec![];
  }

  let text = lines.join("\n");
  let options = roxmltree::ParsingOptions {
    allow_dtd: true,
    ..Default::default()
  };
  let Ok(report) = roxmltree::Document::parse_with_options(&text, options) else {
    return vec![];
  };
  let root = report.root_element();

  if !root.has_tag_name("nvidia_smi_log") {
    return vec![];
  }

  root
    .children()
    .filter(|one| one.has_tag_name("gpu"))
    .map(|gpu| {
      child(gpu, "temperature")
        .and_then(|temperature| child(temperature, "gpu_temp_slow_threshold"))
        .and_then(|threshold| threshold.text())
        .and_then(|text| parse_float(text.trim()))
    })
    .collect()
}

/// Each GPU's line, in the columns `nvidia-smi --query-gpu` gives them in, its slowdowns among them,
/// and the temperature it slows down at: a column of its own where NVML gave it, out of the report
/// otherwise.
pub fn read_gpus(lines: &[String], report: &[String]) -> Vec<Gpu> {
  let limits = report_limits(report);

  lines
    .iter()
    .filter_map(|line| {
      let cells: Vec<&str> = line.split(',').map(str::trim).collect();
      let at = |index: usize| cells.get(index).and_then(|cell| parse_float(cell));

      if cells.len() < 12 {
        return None;
      }

      let index = at(0)?;
      let hardware = slowdown(cells.get(12));
      let driver = slowdown(cells.get(13));
      let reported = (index >= 0.0 && index.fract() == 0.0)
        .then(|| limits.get(index as usize).copied().flatten())
        .flatten();

      Some(Gpu {
        index,
        name: cells[1].to_owned(),
        usage: at(2).map(|usage| usage / 100.0),
        celsius: at(3),
        mhz: at(4),
        max_mhz: at(5),
        memory_mhz: at(6),
        watts: at(7),
        watt_limit: at(8),
        memory_used_mb: at(9),
        memory_total_mb: at(10),
        fan_percent: at(11),
        limit_celsius: at(14).or(reported),
        throttling: (hardware.is_some() || driver.is_some())
          .then(|| hardware == Some(true) || driver == Some(true)),
      })
    })
    .collect()
}

/// The distribution as os-release names it: its pretty name, or its name and version.
fn read_os(lines: &[String]) -> Option<String> {
  let mut fields: BTreeMap<String, String> = BTreeMap::new();

  for line in lines {
    let (key, value) = line.split_once('=').unwrap_or((line.as_str(), line.as_str()));
    let value = value.trim();
    let value = value.strip_prefix('"').and_then(|value| value.strip_suffix('"')).unwrap_or(value);

    fields.insert(key.trim().to_owned(), value.to_owned());
  }

  let named = |key: &str| fields.get(key).map(String::as_str).filter(|value| !value.is_empty());

  named("PRETTY_NAME").map(str::to_owned).or_else(|| {
    let joined = [named("NAME"), named("VERSION_ID")].into_iter().flatten().collect::<Vec<_>>().join(" ");

    (!joined.is_empty()).then_some(joined)
  })
}

static SERVICE_LINE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(system|user) (running|failed) (.+)$").expect("the service pattern"));
static FAILED_MARK: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^[●*]\s*").expect("the failed mark pattern"));

fn read_services(lines: &[String]) -> (Option<ServiceCount>, Option<ServiceCount>) {
  let mut system: Option<ServiceCount> = None;
  let mut user: Option<ServiceCount> = None;

  for line in lines {
    let Some(found) = SERVICE_LINE.captures(line.trim()) else {
      continue;
    };
    let count = if &found[1] == "system" { &mut system } else { &mut user };
    let count = count.get_or_insert_with(ServiceCount::default);

    if &found[2] == "running" {
      count.running = parse_float(&found[3]).unwrap_or(0.0);
    } else {
      count.failed.push(FAILED_MARK.replace(&found[3], "").into_owned());
    }
  }

  (system, user)
}

static WSL_KERNEL: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)microsoft|WSL").expect("the WSL pattern"));

/// Everything the probe printed, as figures. Output that never reached its end marker was cut off, and
/// is refused rather than read as a machine with half its parts missing. On WSL the machine's own
/// figures are the Windows side's where that side answered (docs/hardware.md, 「The Windows side」).
pub fn read_sections(found: &Sections) -> Option<Snapshot> {
  if !found.contains_key("end") {
    return None;
  }

  if found.contains_key("windows") {
    return Some(read_windows_machine(found));
  }

  let wsl = WSL_KERNEL.is_match(&format!(
    "{} {}",
    first(found, "kernel").unwrap_or(""),
    first(found, "version").unwrap_or("")
  ));
  let (threads, model, source) = read_threads(found, wsl);
  let (sensors, fans) = read_hwmon(lines(found, "hwmon"));
  let storage =
    read_linux_storage(lines(found, "blk"), lines(found, "blkdev"), lines(found, "df"), &sensors, wsl);
  let snapshot = Snapshot {
    host: first(found, "host").map(str::to_owned),
    kernel: first(found, "kernel").map(str::to_owned),
    wsl,
    os: read_os(lines(found, "os")),
    host_os: None,
    sensor_library: None,
    uptime_seconds: first(found, "uptime")
      .and_then(|line| line.split_whitespace().next())
      .and_then(parse_float),
    load: read_load(lines(found, "loadavg")),
    memory: read_memory(lines(found, "meminfo")),
    cpu_model: model,
    cpu_usage: usage_between(&read_stat(lines(found, "stat1")), &read_stat(lines(found, "stat2")), "cpu"),
    clock_source: source,
    threads,
    cpu_watts: read_watts(found),
    sensors,
    fans,
    gpus: read_gpus(lines(found, "gpu"), lines(found, "gpureport")),
    storage,
    services: read_services(lines(found, "services")),
  };
  let Some(windows) = wsl.then(|| read_windows(lines(found, "win"))).flatten() else {
    return Some(snapshot);
  };
  let overlaid: Vec<CpuThread> = windows
    .threads
    .iter()
    .enumerate()
    .map(|(index, one)| CpuThread {
      usage: one.usage,
      mhz: one.mhz,
      ..snapshot.threads.get(index).cloned().unwrap_or_else(|| CpuThread::new(index as f64))
    })
    .collect();
  let swap = snapshot
    .memory
    .as_ref()
    .map_or((0.0, 0.0), |memory| (memory.swap_total_kb, memory.swap_free_kb));
  let mut storage = snapshot.storage.clone();

  storage.extend(windows_storage(&windows.disks));

  Some(Snapshot {
    host_os: windows.os.clone(),
    sensor_library: windows.library,
    uptime_seconds: windows.uptime_seconds.or(snapshot.uptime_seconds),
    memory: match windows.memory {
      None => snapshot.memory.clone(),
      Some((total_kb, available_kb)) => Some(Memory {
        total_kb,
        available_kb,
        swap_total_kb: swap.0,
        swap_free_kb: swap.1,
      }),
    },
    cpu_usage: windows.cpu_usage.or(snapshot.cpu_usage),
    clock_source: overlaid.iter().any(|one| one.mhz.is_some()).then_some("windows"),
    threads: if overlaid.is_empty() { snapshot.threads.clone() } else { overlaid },
    cpu_watts: windows.cpu_watts.map(Watts::Read).or(snapshot.cpu_watts),
    sensors: windows.sensors.clone(),
    fans: windows.fans.clone(),
    storage,
    ..snapshot
  })
}

/// A Windows machine read in its own PowerShell (docs/hardware.md, 「A Windows machine」): every figure
/// is the Windows side's, the GPUs are NVML's in the columns the POSIX probe reads, and what Windows has
/// none of — the load average and systemd's services — is unreadable. Windows numbers a core's logical
/// processors next to each other, so each one's core is its place divided by the threads per core.
fn read_windows_machine(found: &Sections) -> Snapshot {
  let windows = read_windows(lines(found, "win"));
  let cpu = windows.as_ref().and_then(|windows| windows.cpu.clone());
  let per_core = cpu.as_ref().and_then(|cpu| match (cpu.cores, cpu.logical) {
    (Some(cores), Some(logical)) if cores != 0.0 && logical != 0.0 => Some(round(logical / cores).max(1.0)),
    _ => None,
  });
  let threads: Vec<CpuThread> = windows
    .as_ref()
    .map(|windows| windows.threads.as_slice())
    .unwrap_or_default()
    .iter()
    .enumerate()
    .map(|(index, one)| CpuThread {
      cpu: index as f64,
      package: per_core.map(|_| 0.0),
      core: per_core.map(|per_core| (index as f64 / per_core).floor()),
      l3: None,
      l3_size: None,
      usage: one.usage,
      mhz: one.mhz,
    })
    .collect();

  Snapshot {
    host: first(found, "host").map(str::to_owned),
    kernel: windows.as_ref().and_then(|windows| windows.version.clone()),
    wsl: false,
    os: windows.as_ref().and_then(|windows| windows.os.clone()),
    host_os: None,
    sensor_library: windows.as_ref().and_then(|windows| windows.library),
    uptime_seconds: windows.as_ref().and_then(|windows| windows.uptime_seconds),
    load: None,
    memory: windows.as_ref().and_then(|windows| {
      let (total_kb, available_kb) = windows.memory?;
      let (swap_total_kb, swap_free_kb) = windows.swap.unwrap_or((0.0, 0.0));

      Some(Memory {
        total_kb,
        available_kb,
        swap_total_kb,
        swap_free_kb,
      })
    }),
    cpu_model: cpu.map(|cpu| cpu.model),
    cpu_usage: windows.as_ref().and_then(|windows| windows.cpu_usage),
    clock_source: threads.iter().any(|one| one.mhz.is_some()).then_some("windows"),
    threads,
    cpu_watts: windows.as_ref().and_then(|windows| windows.cpu_watts).map(Watts::Read),
    sensors: windows.as_ref().map(|windows| windows.sensors.clone()).unwrap_or_default(),
    fans: windows.as_ref().map(|windows| windows.fans.clone()).unwrap_or_default(),
    gpus: read_gpus(lines(found, "gpu"), &[]),
    storage: windows_storage(windows.as_ref().map(|windows| windows.disks.as_slice()).unwrap_or_default()),
    services: (None, None),
  }
}

/// One live run's readings made whole (docs/hardware.md, 「Live」): each laid under the last of the
/// parts that hardly change, and set against the reading before it for use and power, as everything the
/// probe would have printed run once across that stretch. A run's own state, since a new run starts its
/// stretch afresh.
#[derive(Default)]
pub struct LiveProbe {
  seldom: Option<Sections>,
  /// The GPUs' report, which a run prints in its first reading alone.
  gpu_report: Vec<String>,
  last: Option<(f64, Vec<String>, Vec<String>)>,
}

impl LiveProbe {
  /// A reading of the run, whole; none while the run has not yet given its start and its seldom parts,
  /// or where the reading was cut off or does not move on from the one before.
  pub fn take(&mut self, text: &str) -> Option<Sections> {
    let found = sections(text);
    let uptime = lines(&found, "uptime").first().and_then(|line| parse_float(line))?;

    if !found.contains_key("end") {
      return None;
    }

    if found.contains_key("host") {
      self.seldom = Some(
        LIVE_SELDOM
          .iter()
          .map(|&name| (name.to_owned(), lines(&found, name).to_vec()))
          .collect(),
      );
    }

    if found.contains_key("gpureport") {
      self.gpu_report = lines(&found, "gpureport").to_vec();
    }

    let own = |name: &str| lines(&found, name).to_vec();
    let before = self.last.replace((uptime, own("stat"), own("rapl")))?;
    let seldom = self.seldom.as_ref()?;
    let kept = |name: &str| lines(seldom, name).to_vec();

    if uptime <= before.0 {
      return None;
    }

    let mut whole: Sections = [
      ("host", kept("host")),
      ("kernel", kept("kernel")),
      ("version", kept("version")),
      ("os", kept("os")),
      ("uptime", own("uptime")),
      ("loadavg", own("loadavg")),
      ("meminfo", own("meminfo")),
      ("cpuinfo", [kept("model"), own("cpumhz")].concat()),
      ("topology", [kept("layout"), own("clock")].concat()),
      ("stat1", before.1),
      ("rapl1", before.2),
      ("stat2", own("stat")),
      ("rapl2", own("rapl")),
      ("interval", vec![super::js::shown(round_to(uptime - before.0, 100.0))]),
      ("hwmon", own("hwmon")),
      ("gpu", own("gpu")),
      ("gpureport", self.gpu_report.clone()),
      ("blk", kept("blk")),
      ("blkdev", kept("blkdev")),
      ("df", kept("df")),
      ("win", own("win")),
      ("services", kept("services")),
      ("end", vec![]),
    ]
    .into_iter()
    .map(|(name, lines)| (name.to_owned(), lines))
    .collect();

    // A Windows machine's every reading says it is one, and carries the whole machine in its records.
    if found.contains_key("windows") {
      whole.insert("windows".into(), vec![]);
    }

    Some(whole)
  }
}
