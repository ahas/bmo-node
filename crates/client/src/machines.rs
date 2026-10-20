//! The machines (docs/api.md, 「Machines」; docs/machine.md): a machine with its latest reading, its tracking
//! and CPU limit, and this machine's standing as the minute look last read it; and the service's health.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::present;
use crate::hardware::HardwareView;
use crate::page::page;

/// How a machine is reached: this machine itself, or a node of either connection type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MachineKind {
  Local,
  Ssh,
  Hub,
}

/// A machine as every route and event answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
  /// `local` for the service's own machine, a node's name for a node.
  pub name: String,
  pub label: String,
  pub kind: MachineKind,
  /// Whether the machine's live tracking is on; a machine not tracked has neither a reading nor a failure.
  pub tracking: bool,
  /// The CPU limit set for the machine, in °C.
  pub cpu_limit: Option<f64>,
  /// When the reading or the failure came, in epoch milliseconds; null where the machine is not tracked.
  pub read_at: Option<i64>,
  /// The latest reading; null where the machine was not read.
  pub hardware: Option<HardwareView>,
  /// What was met where the machine could not be read.
  pub failure: Option<String>,
  /// The service's own machine as the minute look last read it; null for any other machine.
  pub standing: Option<Standing>,
}

page!(
  MachinePage,
  Machine,
  "A page of machines: this machine, then every node in the list's order."
);

/// A change to a machine, as a JSON merge patch: only the fields sent change, and null clears `cpuLimit`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MachineChanges {
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  #[schemars(with = "bool")]
  pub tracking: Option<bool>,
  /// Above 0 °C and below 127 °C.
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  pub cpu_limit: Option<Option<f64>>,
}

/// The share of a stretch some or all tasks stood stalled, in percent, over 10, 60 and 300 seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PressureLine {
  pub avg10: f64,
  pub avg60: f64,
  pub avg300: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pressure {
  pub some: PressureLine,
  /// Null for a resource the kernel keeps no `full` line for.
  pub full: Option<PressureLine>,
}

/// The machine as its kernel reads, every reading that cannot be had null rather than a nought.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KernelReading {
  /// When it was read, in epoch milliseconds.
  pub at: i64,
  pub load1: f64,
  pub threads: i64,
  pub mem_total_gb: f64,
  pub mem_available_gb: f64,
  pub swap_total_gb: f64,
  pub swap_free_gb: f64,
  /// The free space of the data root's disk, for a writer that is not the root user.
  pub disk_free_gb: Option<f64>,
  pub memory: Option<Pressure>,
  pub cpu: Option<Pressure>,
}

/// A pressure standing on the machine now (docs/machine.md, 「The standing and its alerts」).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "cause", rename_all = "lowercase")]
pub enum Alert {
  /// Memory `full` at or above the alert line, standing for the alert minutes.
  #[serde(rename_all = "camelCase")]
  Memory {
    /// The memory `full` over the last minute, in percent.
    full_pct: f64,
    /// The alert line, in percent.
    alert_pct: f64,
    /// How long it has stood.
    minutes: i64,
  },
  /// The data root's disk under its line.
  #[serde(rename_all = "camelCase")]
  Disk { free_gb: f64, limit_gb: f64 },
}

impl Alert {
  /// What the alert is about, which tells one alert from another.
  pub fn cause(&self) -> &'static str {
    match self {
      Self::Memory { .. } => "memory",
      Self::Disk { .. } => "disk",
    }
  }
}

/// What the processes of one scope hold of the machine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScopeUse {
  /// The caller's own scope, as the process's environment names it.
  pub scope: String,
  pub megabytes: i64,
  pub browsers: i64,
}

/// One of the largest processes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Largest {
  pub pid: i64,
  pub name: String,
  pub megabytes: i64,
  pub scope: Option<String>,
}

/// The service's own machine as the minute look read it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Standing {
  /// Null where the machine could not be read, which raises no alert.
  pub reading: Option<KernelReading>,
  /// Each pressure that stands now.
  pub alerts: Vec<Alert>,
  /// How many browser main processes run.
  pub browsers: i64,
  /// What each scope holds, largest first.
  pub scopes: Vec<ScopeUse>,
  /// The five largest processes.
  pub largest: Vec<Largest>,
}

impl Standing {
  /// The standing of a machine nobody has read yet.
  pub fn unread() -> Self {
    Self {
      reading: None,
      alerts: Vec::new(),
      browsers: 0,
      scopes: Vec::new(),
      largest: Vec::new(),
    }
  }
}

/// Whether the service stands: ready once its store is open and migrated and its loops have started.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Health {
  pub ready: bool,
  /// Why it is not ready; null when it is.
  pub reason: Option<String>,
  pub version: String,
}
