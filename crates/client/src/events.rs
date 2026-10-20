//! The events the service publishes on its one stream (docs/api.md, 「Events」): each named by its resource
//! and its change, numbered by the service, its data the member as a GET answers it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::hardware::Reading;
use crate::machines::Machine;
use crate::nodes::{Node, NodeKey};
use crate::page::page;

/// A kind of event: a resource and its change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum EventKind {
  #[serde(rename = "node.created")]
  NodeCreated,
  #[serde(rename = "node.changed")]
  NodeChanged,
  #[serde(rename = "node.deleted")]
  NodeDeleted,
  #[serde(rename = "machine.changed")]
  MachineChanged,
  #[serde(rename = "reading.created")]
  ReadingCreated,
}

impl EventKind {
  /// Every kind.
  pub const ALL: [Self; 5] = [
    Self::NodeCreated,
    Self::NodeChanged,
    Self::NodeDeleted,
    Self::MachineChanged,
    Self::ReadingCreated,
  ];

  /// The kind as a stream names it.
  pub fn name(self) -> &'static str {
    match self {
      Self::NodeCreated => "node.created",
      Self::NodeChanged => "node.changed",
      Self::NodeDeleted => "node.deleted",
      Self::MachineChanged => "machine.changed",
      Self::ReadingCreated => "reading.created",
    }
  }

  /// The kind a name names, or none where it names none.
  pub fn of_name(name: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|kind| kind.name() == name)
  }
}

/// One event as the stream carries it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Event {
  /// The service's event number, which only grows.
  pub id: i64,
  pub kind: EventKind,
  /// When it was published, in epoch milliseconds.
  pub at: i64,
  /// The member as a GET answers it, or its key alone for a deletion; the document's `x-events` gives the
  /// schema of each kind's.
  #[schemars(with = "Map<String, Value>")]
  pub data: Value,
}

impl Event {
  /// The change the event carries, read as its kind's member.
  pub fn change(&self) -> Result<Change, serde_json::Error> {
    let data = self.data.clone();

    Ok(match self.kind {
      EventKind::NodeCreated => Change::NodeCreated(serde_json::from_value(data)?),
      EventKind::NodeChanged => Change::NodeChanged(serde_json::from_value(data)?),
      EventKind::NodeDeleted => Change::NodeDeleted(serde_json::from_value(data)?),
      EventKind::MachineChanged => Change::MachineChanged(Box::new(serde_json::from_value(data)?)),
      EventKind::ReadingCreated => Change::ReadingCreated(serde_json::from_value(data)?),
    })
  }
}

page!(EventPage, Event, "A page of events, in the order they were published.");

/// A change to publish, with the member it carries.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
  NodeCreated(Node),
  NodeChanged(Node),
  NodeDeleted(NodeKey),
  MachineChanged(Box<Machine>),
  ReadingCreated(Reading),
}

impl Change {
  pub fn kind(&self) -> EventKind {
    match self {
      Self::NodeCreated(_) => EventKind::NodeCreated,
      Self::NodeChanged(_) => EventKind::NodeChanged,
      Self::NodeDeleted(_) => EventKind::NodeDeleted,
      Self::MachineChanged(_) => EventKind::MachineChanged,
      Self::ReadingCreated(_) => EventKind::ReadingCreated,
    }
  }

  /// The member as the event's data carries it.
  pub fn data(&self) -> Value {
    let data = match self {
      Self::NodeCreated(node) | Self::NodeChanged(node) => serde_json::to_value(node),
      Self::NodeDeleted(key) => serde_json::to_value(key),
      Self::MachineChanged(machine) => serde_json::to_value(machine),
      Self::ReadingCreated(reading) => serde_json::to_value(reading),
    };

    data.unwrap_or(Value::Null)
  }

  /// The machine the change belongs to, where it belongs to one.
  pub fn machine_name(&self) -> Option<&str> {
    match self {
      Self::MachineChanged(machine) => Some(&machine.name),
      Self::ReadingCreated(reading) => Some(&reading.machine_name),
      _ => None,
    }
  }
}

/// Where a stream starts and what it carries.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventsQuery {
  /// The number of the last event already read; the stream starts at the moment it is opened where none is
  /// given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// The comma-separated kinds; every kind where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub kinds: Option<String>,
  /// Keeps the `machine.changed` and `reading.created` events of this machine alone.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub machine_name: Option<String>,
  /// The most events a page answers, for a reader asking `Accept: application/json`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
}
