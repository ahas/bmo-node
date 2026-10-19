//! The service's own machine (docs/api.md, 「This machine」; docs/machine.md): what it is short of for
//! another launch, and how the minute watch last read it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
pub struct Reading {
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

/// What one agent place holds of the machine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Place {
  pub place: String,
  pub megabytes: i64,
  pub browsers: i64,
}

/// One of the largest processes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Largest {
  pub pid: i64,
  pub name: String,
  pub megabytes: i64,
  pub place: Option<String>,
}

/// The machine as the minute watch read it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Standing {
  /// Null where the machine could not be read, which holds nothing.
  pub reading: Option<Reading>,
  /// What the machine is short of for another launch, each an English reason with its figure.
  pub held: Vec<String>,
  /// How many browser main processes run.
  pub browsers: i64,
  /// What each agent place holds, largest first.
  pub places: Vec<Place>,
  /// The five largest processes.
  pub largest: Vec<Largest>,
}

impl Standing {
  /// The standing of a machine nobody has read yet.
  pub fn unread() -> Self {
    Self {
      reading: None,
      held: Vec::new(),
      browsers: 0,
      places: Vec::new(),
      largest: Vec::new(),
    }
  }
}

/// What the machine is short of for another launch, read now; empty where it has room.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Room {
  pub held: Vec<String>,
}

/// Whether the service stands: its process, its version, and whether its store is open and its loops
/// have started.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Health {
  pub pid: u32,
  pub version: String,
  pub ready: bool,
}
