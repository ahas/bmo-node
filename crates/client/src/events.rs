//! The events the service publishes on its stream (docs/api.md, 「Events」): each named by its kind, its
//! data the event's JSON.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::machine::{Largest, Place, Reading};

/// A kind of event, as a stream names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
  NodesChanged,
  MachinePressure,
}

impl EventKind {
  /// Every kind.
  pub const ALL: [Self; 2] = [Self::NodesChanged, Self::MachinePressure];

  /// The kind as a stream names it.
  pub fn name(self) -> &'static str {
    match self {
      Self::NodesChanged => "nodes-changed",
      Self::MachinePressure => "machine-pressure",
    }
  }

  /// The kind a name names, or none where it names none.
  pub fn of_name(name: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|kind| kind.name() == name)
  }
}

/// Which kinds of event a stream carries.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventsQuery {
  /// The comma-separated kinds: `nodes-changed`, `machine-pressure`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub kinds: Option<String>,
}

/// A pass moved a node's state, or the list changed; a consumer showing the nodes reads them again.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodesChanged {}

/// A pressure on the service's machine, told once (docs/machine.md).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "cause", rename_all = "lowercase")]
pub enum MachinePressure {
  /// Memory `full` at or above the alert line, standing for the alert minutes.
  #[serde(rename_all = "camelCase")]
  Memory {
    /// The memory `full` over the last minute, in percent.
    full_pct: f64,
    /// The alert line, in percent.
    alert_pct: f64,
    /// How long it has stood.
    minutes: i64,
    reading: Option<Reading>,
    places: Vec<Place>,
    largest: Vec<Largest>,
  },
  /// The data root's disk under the launch line.
  #[serde(rename_all = "camelCase")]
  Disk {
    free_gb: f64,
    limit_gb: f64,
    reading: Option<Reading>,
  },
}

/// One event read off the stream.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
  NodesChanged(NodesChanged),
  MachinePressure(MachinePressure),
}

impl Event {
  pub fn kind(&self) -> EventKind {
    match self {
      Self::NodesChanged(_) => EventKind::NodesChanged,
      Self::MachinePressure(_) => EventKind::MachinePressure,
    }
  }

  /// The event's data as the stream carries it.
  pub fn data(&self) -> Value {
    let data = match self {
      Self::NodesChanged(data) => serde_json::to_value(data),
      Self::MachinePressure(data) => serde_json::to_value(data),
    };

    data.unwrap_or(Value::Null)
  }

  /// An event of a kind read with its data, none where the kind is not one of these.
  pub fn read(kind: &str, data: &str) -> Option<Result<Self, serde_json::Error>> {
    let read = match EventKind::of_name(kind)? {
      EventKind::NodesChanged => serde_json::from_str(data).map(Self::NodesChanged),
      EventKind::MachinePressure => serde_json::from_str(data).map(Self::MachinePressure),
    };

    Some(read)
  }
}
