//! The nodes and the hubs (docs/api.md, 「Nodes」 and 「Hubs」): a node as every route answers it, its
//! connection and its login, and the bodies that register, change and remove nodes and hubs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::present;

/// A node's connection type: logged into over SSH, or a jake agent reached through its hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
  Ssh,
  Jake,
}

impl NodeKind {
  /// Every connection type.
  pub const ALL: [Self; 2] = [Self::Ssh, Self::Jake];

  /// The connection type a code names, or none where it names none.
  pub fn of_code(code: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|kind| kind.name() == code)
  }

  /// The code the store's column and every answer hold.
  pub fn name(self) -> &'static str {
    match self {
      Self::Ssh => "ssh",
      Self::Jake => "jake",
    }
  }
}

/// One state a node's check comes to (docs/nodes.md, 「The states」).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
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

  /// The code the store's column and every answer hold.
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
}

/// How a state reads at a glance: reached, not reached, or not yet asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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

/// The language an ssh node's login answers a command in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LoginShell {
  #[default]
  Posix,
  Powershell,
}

impl LoginShell {
  /// Every login shell.
  pub const ALL: [Self; 2] = [Self::Posix, Self::Powershell];

  /// The shell a code names, or none where it names none.
  pub fn of_code(code: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|shell| shell.name() == code)
  }

  /// The code the store's column and every answer hold.
  pub fn name(self) -> &'static str {
    match self {
      Self::Posix => "posix",
      Self::Powershell => "powershell",
    }
  }
}

/// A node as every route answers it. It holds no credential, only where one is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Node {
  pub name: String,
  pub label: String,
  pub kind: NodeKind,
  /// A jake node's agent, by the name its hub knows it by.
  pub agent: Option<String>,
  /// Where it is reached: `user@host`, `:port` after it where the port is not 22 and ` (PowerShell)` where
  /// the login answers in PowerShell; `agent @ hub` for a jake node.
  pub address: String,
  /// Where the credential is kept: an ssh node's key path, or the token file of a jake node's hub.
  pub credential: Option<String>,
  /// The services the node is reserved for, joined by `+`.
  pub role: Option<String>,
  pub state: NodeState,
  pub tone: Tone,
  /// The last check's line.
  pub detail: Option<String>,
  /// When the last check was made, in epoch milliseconds.
  pub checked_at: Option<i64>,
  /// When the node was last found connected, in epoch milliseconds.
  pub connected_at: Option<i64>,
  /// The last check found the node connected and was made within the link window.
  pub up: bool,
  /// An ssh node whose host names the service's own machine.
  pub this_machine: bool,
  /// An ssh node other than the service's own machine, whose login is not PowerShell's and whose role is
  /// none or names `jobs`.
  pub takes_jobs: bool,
}

/// Nodes in the list's order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodeList {
  pub nodes: Vec<Node>,
}

/// How much of the node list to read: the nodes after the one named, at most `limit` of them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodeListQuery {
  /// The name of the last node already read.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<String>,
  /// The most nodes to answer.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
}

/// Which nodes to check now: the one named, or every node.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckNodes {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
}

/// The one form a node is reached by, whatever reaches it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
  Ssh(SshTarget),
  Jake(JakeTarget),
}

/// An ssh node's connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshTarget {
  pub host: String,
  pub port: u16,
  pub user: String,
  /// The private key the login offers, as the row holds it; a leading `~/` is the home of the account that
  /// reads it.
  pub key_path: Option<String>,
  pub shell: LoginShell,
  /// The OpenSSH options of a batch login, each given after `-o`, the options a key brings among them where
  /// the node names a key; the same for `ssh`, `sftp` and `scp`.
  pub options: Vec<String>,
  pub role: Option<String>,
}

/// A jake node's connection: its agent and the hub it is reached through.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JakeTarget {
  pub hub_url: String,
  pub token_file: String,
  pub agent: String,
  pub role: Option<String>,
}

/// The arguments that open a batch SSH login to a node, ahead of the command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Login {
  pub args: Vec<String>,
}

/// A new ssh node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewNode {
  pub name: String,
  pub host: String,
  pub user: String,
  /// 22 where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub port: Option<i64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub key_path: Option<String>,
  /// `posix` where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub shell: Option<LoginShell>,
  /// The name where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub label: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub role: Option<String>,
}

/// A change to a node: only the fields sent change, and null clears `keyPath` and `role`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeChanges {
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  #[schemars(with = "String")]
  pub label: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  pub role: Option<Option<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  #[schemars(with = "String")]
  pub host: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  #[schemars(with = "String")]
  pub user: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  #[schemars(with = "i64")]
  pub port: Option<i64>,
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  pub key_path: Option<Option<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  #[schemars(with = "LoginShell")]
  pub shell: Option<LoginShell>,
}

/// The node a removal took away, by its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Removed {
  pub removed: String,
}

/// A jake hub: its control address and the file its token is kept in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Hub {
  pub url: String,
  pub token_file: String,
}

/// The hub to remove, by its control address.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HubQuery {
  pub url: String,
}

/// A hub removed, and the jake nodes reached through it, removed with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HubRemoved {
  pub url: String,
  pub removed: Vec<String>,
}
