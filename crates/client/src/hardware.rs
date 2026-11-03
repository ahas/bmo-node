//! The hardware monitor (docs/api.md, 「Machines」; docs/hardware.md): a machine's latest reading as a screen
//! draws it, one group per piece of hardware, and its past readings, each figure a band over a stretch.
//! Every figure is what the machine answered or none.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::page::page;

/// One temperature input of a chip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fan {
  pub chip: String,
  pub label: String,
  pub rpm: f64,
}

/// Package power over the sample: watts, or the counter refusing the account, the code `denied`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Watts {
  Read(f64),
  Denied,
}

/// The code a power the account may not read is answered as.
const DENIED: &str = "denied";

impl Serialize for Watts {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    match self {
      Self::Read(watts) => serializer.serialize_f64(*watts),
      Self::Denied => serializer.serialize_str(DENIED),
    }
  }
}

impl<'de> Deserialize<'de> for Watts {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Sent {
      Read(f64),
      Code(String),
    }

    match Sent::deserialize(deserializer)? {
      Sent::Read(watts) => Ok(Self::Read(watts)),
      Sent::Code(code) if code == DENIED => Ok(Self::Denied),
      Sent::Code(code) => Err(serde::de::Error::custom(format!("{code} is not a power"))),
    }
  }
}

impl JsonSchema for Watts {
  fn schema_name() -> Cow<'static, str> {
    "Watts".into()
  }

  fn json_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
      "description": "Package power over the sample in watts, or `denied` where the account may not read the counter.",
      "oneOf": [{ "type": "number" }, { "const": DENIED }],
    })
  }
}

/// One thread's share busy, 0 to 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ThreadUse {
  pub cpu: f64,
  pub usage: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoreView {
  pub label: String,
  /// Share busy across the core's threads, 0 to 1.
  pub usage: Option<f64>,
  pub mhz: Option<f64>,
  pub celsius: Option<f64>,
  pub threads: Vec<ThreadUse>,
}

/// A group of cores around their shared cache, or a part of a die that is not a core.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Item {
  Cluster {
    label: String,
    cache: String,
    left: Vec<CoreView>,
    right: Vec<CoreView>,
  },
  Part {
    label: String,
    off: bool,
  },
}

/// One die on the package.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DieView {
  pub label: String,
  /// `compute` or `io`.
  pub role: String,
  pub celsius: Option<f64>,
  /// Which way its items stand: `column`, one above the other, or `row`, side by side.
  pub direction: String,
  pub items: Vec<Item>,
}

/// The chip as it physically stands (docs/hardware.md, 「The chip」).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChipView {
  pub label: String,
  pub celsius: Option<f64>,
  /// The package's rows of dies, top to bottom; a null is a place on the package with no die in it.
  pub rows: Vec<Vec<Option<DieView>>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CpuView {
  pub model: Option<String>,
  pub thread_count: usize,
  pub usage: Option<f64>,
  /// The package's temperature: the chip's own package sensor, or the hottest of the CPU's sensors; and
  /// the limit that sensor's chip names, none where it names none (「Temperature limits」).
  pub celsius: Option<f64>,
  pub limit_celsius: Option<f64>,
  /// The mean effective clock across the threads that answered one, and the fastest of them.
  pub mhz: Option<f64>,
  pub peak_mhz: Option<f64>,
  pub watts: Option<Watts>,
  pub chip: Option<ChipView>,
  pub sensors: Vec<Sensor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryView {
  pub used_kb: Option<f64>,
  pub total_kb: Option<f64>,
  pub share: Option<f64>,
  pub swap_used_kb: Option<f64>,
  pub swap_total_kb: Option<f64>,
  pub swap_share: Option<f64>,
  /// The machine's `Shmem`, which `MemAvailable` does not count as available.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub shmem_kb: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GpuView {
  pub index: f64,
  pub name: String,
  pub usage: Option<f64>,
  pub celsius: Option<f64>,
  /// The temperature it slows down at, and whether thermal slowdown holds its clocks down now.
  pub limit_celsius: Option<f64>,
  pub throttling: Option<bool>,
  pub mhz: Option<f64>,
  pub max_mhz: Option<f64>,
  pub memory_mhz: Option<f64>,
  pub watts: Option<f64>,
  pub watt_limit: Option<f64>,
  pub watt_share: Option<f64>,
  pub memory_used_mb: Option<f64>,
  pub memory_total_mb: Option<f64>,
  pub memory_share: Option<f64>,
  pub fan_percent: Option<f64>,
}

/// One mounted volume.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageVolume {
  pub mount: String,
  pub fs_type: Option<String>,
  pub total_kb: f64,
  pub used_kb: f64,
  pub available_kb: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VolumeView {
  #[serde(flatten)]
  pub volume: StorageVolume,
  pub share: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageView {
  /// Unique within the machine: the kernel's name, `win<n>` for a Windows disk, `net:<source>` for a
  /// network share.
  pub key: String,
  pub name: String,
  /// What the device is, as the probe tells it (docs/hardware.md, 「Storage」).
  pub kind: Option<String>,
  pub size_bytes: Option<f64>,
  pub celsius: Option<f64>,
  pub limit_celsius: Option<f64>,
  pub volumes: Vec<VolumeView>,
  /// Across its volumes; none where none of it is mounted.
  pub used_kb: Option<f64>,
  pub total_kb: Option<f64>,
  pub share: Option<f64>,
}

/// Everything the board reports that belongs to no other piece: its own temperatures and its fans.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoardView {
  pub sensors: Vec<Sensor>,
  pub fans: Vec<Fan>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ServicesView {
  pub running: Option<f64>,
  pub failed: Option<Vec<String>>,
}

/// One machine's reading as a screen draws it: one group per piece of hardware.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HardwareView {
  pub host: Option<String>,
  pub kernel: Option<String>,
  pub os: Option<String>,
  pub host_os: Option<String>,
  pub wsl: bool,
  pub sensor_library: Option<String>,
  pub uptime_seconds: Option<f64>,
  pub load: Option<[f64; 3]>,
  pub cpu: CpuView,
  pub memory: MemoryView,
  pub gpus: Vec<GpuView>,
  pub storage: Vec<StorageView>,
  pub board: BoardView,
  pub services: ServicesView,
}

/// One figure over a stretch: its lowest, its highest and its mean.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Band {
  pub min: f64,
  pub max: f64,
  pub mean: f64,
}

/// The CPU's figures over a stretch.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CpuFigures {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub use_pct: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub celsius: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub watts: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mhz: Option<Band>,
}

/// Memory's figures over a stretch.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryFigures {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub used_gb: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub available_gb: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub shmem_gb: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub swap_used_gb: Option<Band>,
}

/// One GPU's figures over a stretch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GpuFigures {
  pub index: i64,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub use_pct: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub celsius: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub watts: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mhz: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub vram_used_gb: Option<Band>,
}

/// One storage device's figures over a stretch, by the device's key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageFigures {
  pub key: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub used_gb: Option<Band>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub celsius: Option<Band>,
}

/// One fan's figures over a stretch, by its chip and label.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FanFigures {
  pub chip: String,
  pub label: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub rpm: Option<Band>,
}

/// One stretch of a machine's recorded figures, one group per piece of hardware.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
  pub machine_name: String,
  /// The stretch's start, in epoch milliseconds.
  pub at: i64,
  /// The stretch's width.
  pub span_ms: i64,
  pub cpu: CpuFigures,
  pub memory: MemoryFigures,
  pub gpus: Vec<GpuFigures>,
  pub storage: Vec<StorageFigures>,
  pub fans: Vec<FanFigures>,
}

page!(ReadingPage, Reading, "A page of a machine's readings, oldest first.");

/// Which of a machine's readings to read: the stretches of `spanMs` starting after `after` and before
/// `before`, at most `limit` of them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadingsQuery {
  /// In epoch milliseconds; the reach of the rows the span is read from where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// In epoch milliseconds; now where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub before: Option<i64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
  /// Each stretch's width: whole seconds under a minute, whole minutes from a minute up; 1000 where none
  /// is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub span_ms: Option<i64>,
}

/// One process in a look at who holds a machine's memory, in binary megabytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMemory {
  pub pid: i64,
  pub name: String,
  /// The unit the process's control group names.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub unit: Option<String>,
  pub rss_mb: i64,
  /// Its `RssShmem`: what it has mapped of shared memory now, which may be far less than its unit is charged.
  pub shmem_mb: i64,
  pub swap_mb: i64,
}

/// One unit's control group in a look at who holds a machine's memory, in binary megabytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnitMemory {
  pub unit: String,
  pub current_mb: i64,
  pub anon_mb: i64,
  /// The shared memory charged to the unit, whether or not a process has it mapped.
  pub shmem_mb: i64,
  pub file_mb: i64,
  pub swap_mb: i64,
}

/// One look at who held a machine's memory (docs/hardware.md, 「Who holds the memory」).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
  pub machine_name: String,
  /// The second the look was taken at, in epoch milliseconds.
  pub at: i64,
  pub available_gb: Option<f64>,
  pub shmem_gb: Option<f64>,
  pub swap_used_gb: Option<f64>,
  /// The largest processes by resident memory and swap together.
  pub processes: Vec<ProcessMemory>,
  /// The largest units by memory and swap together.
  pub units: Vec<UnitMemory>,
}

page!(MemorySamplePage, MemorySample, "A page of a machine's memory samples, oldest first.");

/// Which of a machine's memory samples to read.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemorySamplesQuery {
  /// In epoch milliseconds; the seven days' reach where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// In epoch milliseconds; now where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub before: Option<i64>,
  /// 500 where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
}
