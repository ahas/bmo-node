//! What a node's last check can come to, and the tone every list shows each in (docs/nodes.md, 「The
//! states」). A state is handed by its code; a caller words it.

use std::collections::BTreeMap;

use serde::Deserialize;

pub use bmo_node_client::nodes::{NodeState, Ownership, Tone};

/// How a state reads at a glance, as the policy's node states give it.
pub trait StateTone {
  /// How the state reads on a shared node.
  fn tone(self) -> Tone;
  /// How the state reads on a node of that ownership: a personal node's own tone where the state declares
  /// one, the state's tone otherwise.
  fn tone_on(self, ownership: Ownership) -> Tone;
}

impl StateTone for NodeState {
  fn tone(self) -> Tone {
    NODE_STATES[&self].tone
  }

  fn tone_on(self, ownership: Ownership) -> Tone {
    let declared = &NODE_STATES[&self];

    match ownership {
      Ownership::Shared => declared.tone,
      Ownership::Personal => declared.personal_tone.unwrap_or(declared.tone),
    }
  }
}

/// One state as the file declares it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
  pub tone: Tone,
  /// How the state reads on a personal node, where it reads otherwise than on a shared one.
  #[serde(default)]
  pub personal_tone: Option<Tone>,
}

crate::part!(pub NODE_STATES: BTreeMap<NodeState, Declared> = "node-states.toml");
