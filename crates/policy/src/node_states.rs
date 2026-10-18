//! What a node's last check can come to, and the tone every list shows each in (docs/nodes.md, 「The
//! states」). A state is handed by its code; the web console words it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One state a node's check comes to, named as the store's rows and every answer name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeState {
  Connected,
  Disconnected,
  Auth,
  Hostkey,
  Unreachable,
  Error,
  Unknown,
}

impl NodeState {
  /// Every state, in the order the lists name them.
  pub const ALL: [Self; 7] = [
    Self::Connected,
    Self::Disconnected,
    Self::Auth,
    Self::Hostkey,
    Self::Unreachable,
    Self::Error,
    Self::Unknown,
  ];

  /// A row's state as its column holds it, `unknown` where the column holds none of them.
  pub fn of_column(column: &str) -> Self {
    Self::of_code(column).unwrap_or(Self::Unknown)
  }

  /// The state a code names, or none where it names no state.
  pub fn of_code(code: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|state| state.name() == code)
  }

  /// The code the column and every answer hold.
  pub fn name(self) -> &'static str {
    match self {
      Self::Connected => "connected",
      Self::Disconnected => "disconnected",
      Self::Auth => "auth",
      Self::Hostkey => "hostkey",
      Self::Unreachable => "unreachable",
      Self::Error => "error",
      Self::Unknown => "unknown",
    }
  }

  /// How the state reads at a glance.
  pub fn tone(self) -> Tone {
    NODE_STATES[&self].tone
  }
}

/// How a state reads at a glance: reached, not reached, or not yet asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
  Good,
  Bad,
  Unknown,
}

impl Tone {
  /// The code every answer holds.
  pub fn name(self) -> &'static str {
    match self {
      Self::Good => "good",
      Self::Bad => "bad",
      Self::Unknown => "unknown",
    }
  }
}

/// One state as the file declares it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
  pub tone: Tone,
}

crate::part!(pub NODE_STATES: BTreeMap<NodeState, Declared> = "node-states.toml");
