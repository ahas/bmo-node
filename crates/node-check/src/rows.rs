//! A node row as a check reads it, and the target it describes.

use policy::nodes::NodeKind;
use serde::{Deserialize, Serialize};

use crate::NodeState;

/// The port an ssh row that names none is logged in to.
pub const DEFAULT_SSH_PORT: u16 = 22;

/// The parts of a node row a check reads, named as the store hands them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRow {
  pub name: String,
  pub kind: String,
  #[serde(default)]
  pub host: Option<String>,
  #[serde(default)]
  pub port: Option<u16>,
  #[serde(default)]
  pub user: Option<String>,
  #[serde(default)]
  pub key_path: Option<String>,
  #[serde(default)]
  pub agent: Option<String>,
  #[serde(default)]
  pub hub_url: Option<String>,
  /// A jake node's hub's token file.
  #[serde(default)]
  pub token_file: Option<String>,
  pub state: NodeState,
  #[serde(default)]
  pub detail: Option<String>,
}

/// An ssh node's login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
  pub host: String,
  pub port: u16,
  pub user: String,
  pub key_path: Option<String>,
}

/// A jake hub: where its control address is, and the file holding its token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hub {
  pub url: String,
  pub token_file: String,
}

/// What a node is reached by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
  Ssh(SshTarget),
  Jake { hub: Hub, agent: String },
}

impl NodeRow {
  /// The target the row describes, or none where it lacks a field its kind needs.
  pub fn target(&self) -> Option<Target> {
    if self.is_hub() {
      return Some(Target::Jake {
        hub: Hub {
          url: self.hub_url.clone()?,
          token_file: self.token_file.clone()?,
        },
        agent: self.agent.clone()?,
      });
    }

    Some(Target::Ssh(SshTarget {
      host: self.host.clone()?,
      port: self.port.unwrap_or(DEFAULT_SSH_PORT),
      user: self.user.clone()?,
      key_path: self.key_path.clone(),
    }))
  }

  /// Whether the row is a hub node's, reached through its hub.
  fn is_hub(&self) -> bool {
    self.kind == NodeKind::Hub.name()
  }

  /// The hub and agent of a hub node's row, where it names both.
  pub fn jake(&self) -> Option<(&str, &str)> {
    self.is_hub().then_some(())?;

    Some((self.hub_url.as_deref()?, self.agent.as_deref()?))
  }
}
