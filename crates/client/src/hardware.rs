//! The hardware monitor (docs/api.md, 「Hardware」; docs/hardware.md): a machine's latest reading as a screen
//! draws it, one group per piece of hardware, its live record's readings, a span of its history, and its
//! tracking switch and CPU limit. Every figure is what the machine answered or none.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How a machine is reached: this machine itself, or a node of either connection type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MachineKind {
  Local,
  Ssh,
  Jake,
}

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

/// A machine's latest reading, or the failure met reading it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatestReading {
  pub name: String,
  pub label: String,
  pub kind: MachineKind,
  /// Null where the machine was not read.
  pub hardware: Option<HardwareView>,
  /// What was met where the machine could not be read.
  pub failure: Option<String>,
  /// When the reading was taken, in epoch milliseconds.
  pub read_at: i64,
  /// Whether the machine's live tracking is on; a machine not tracked has neither a reading nor a failure.
  pub tracking: bool,
  pub cpu_limit: Option<f64>,
}

/// Which live readings to read: those taken after `after`, at most `limit` of them, of the figures
/// measuring the comma-separated `metrics`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LiveQuery {
  /// The time of the last reading already read, in epoch milliseconds.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// The most readings to answer: the first after `after`, or the last where `after` is not given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub metrics: Option<String>,
}

/// Live readings as columns: when each was taken, in epoch milliseconds, and each figure's value at each
/// of those times, null where that reading did not carry it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LiveRange {
  pub at: Vec<i64>,
  pub series: BTreeMap<String, Vec<Option<f64>>>,
}

/// Which span of the history to read: `5m`, `15m`, `1h`, `6h`, `24h`, `7d` or `30d`, `1h` where none is
/// named.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryQuery {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub span: Option<String>,
}

/// One series across a span: each point's lowest, highest and mean, null where nothing was recorded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryBand {
  pub min: Vec<Option<f64>>,
  pub max: Vec<Option<f64>>,
  pub mean: Vec<Option<f64>>,
}

/// One span of the history: the start of each point's stretch in epoch milliseconds, the width of one,
/// and each figure's band.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRange {
  pub at: Vec<i64>,
  pub point_seconds: i64,
  pub series: BTreeMap<String, HistoryBand>,
}

/// The tracking switch to set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrackingSet {
  pub on: bool,
}

/// A machine's tracking switch as kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tracking {
  pub machine: String,
  pub on: bool,
}

/// The CPU limit to set; null clears it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CpuLimitSet {
  #[serde(default)]
  pub celsius: Option<f64>,
}

/// A machine's CPU limit as kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CpuLimit {
  pub machine: String,
  pub celsius: Option<f64>,
}
