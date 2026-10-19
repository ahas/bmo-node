//! What a node's last check can come to, and the tone every list shows each in (docs/nodes.md, 「The
//! states」). A state is handed by its code; a caller words it.

use std::collections::BTreeMap;

use serde::Deserialize;

pub use bmo_node_client::nodes::{NodeState, Tone};

/// How a state reads at a glance, as the policy's node states give it.
pub trait StateTone {
  fn tone(self) -> Tone;
}

impl StateTone for NodeState {
  fn tone(self) -> Tone {
    NODE_STATES[&self].tone
  }
}

/// One state as the file declares it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
  pub tone: Tone,
}

crate::part!(pub NODE_STATES: BTreeMap<NodeState, Declared> = "node-states.toml");
