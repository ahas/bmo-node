//! How the node service logs in to an ssh node (docs/nodes.md, 「How a node's state is known」): the options
//! of a login nobody is at and of a person's, the check's own command, and how a login's end is read.

use serde::Deserialize;

use crate::node_states::NodeState;

/// The OpenSSH options of one kind of login, each given to it after `-o`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
  pub options: Vec<String>,
  /// The options a key the row names brings with it.
  pub key_options: Vec<String>,
}

/// The check's own login: what it runs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
  pub command: String,
}

/// One state a login's end reads as, and the phrases of OpenSSH's that say it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ended {
  pub state: NodeState,
  pub said: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshPolicy {
  pub batch: Login,
  pub person: Login,
  pub check: Check,
  pub ended: Vec<Ended>,
}

impl SshPolicy {
  /// The state a login that ended unsuccessfully met, read off what it said on its standard error:
  /// the first whose phrase it holds, in any case; none where it holds no phrase.
  pub fn ended_state(&self, said: &str) -> Option<NodeState> {
    let said = said.to_lowercase();

    self
      .ended
      .iter()
      .find(|ended| ended.said.iter().any(|phrase| said.contains(&phrase.to_lowercase())))
      .map(|ended| ended.state)
  }
}

crate::part!(pub SSH: SshPolicy = "ssh.toml");
