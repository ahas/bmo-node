//! The nodes, the node checks and the hubs (docs/api.md, 「Nodes」, 「Node checks」 and 「Hubs」): a node as
//! every route answers it, its target, and the bodies that register, change, check and remove nodes and
//! hubs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::present;
use crate::page::page;

/// A node's connection type: logged into over SSH, or an agent reached through its hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
  Ssh,
  Hub,
}

impl NodeKind {
  /// Every connection type.
  pub const ALL: [Self; 2] = [Self::Ssh, Self::Hub];

  /// The connection type a code names, or none where it names none.
  pub fn of_code(code: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|kind| kind.name() == code)
  }

  /// The code the store's column and every answer hold.
  pub fn name(self) -> &'static str {
    match self {
      Self::Ssh => "ssh",
      Self::Hub => "hub",
    }
  }
}

/// Whose a node is (docs/nodes.md, 「A node's ownership」): a machine of the platform, run for everyone, or one
/// person's own PC, which the hardware monitor never reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Ownership {
  #[default]
  Shared,
  Personal,
}

impl Ownership {
  /// Every ownership.
  pub const ALL: [Self; 2] = [Self::Shared, Self::Personal];

  /// The ownership a code names, or none where it names none.
  pub fn of_code(code: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|ownership| ownership.name() == code)
  }

  /// The code the store's column and every answer hold.
  pub fn name(self) -> &'static str {
    match self {
      Self::Shared => "shared",
      Self::Personal => "personal",
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

/// A node as every route and event answers it. It holds no credential, only where one is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Node {
  pub name: String,
  pub label: String,
  pub kind: NodeKind,
  /// A hub node's hub.
  pub hub_id: Option<i64>,
  /// The name a hub node's hub knows its agent by.
  pub agent_name: Option<String>,
  /// Where it is reached: `user@host`, `:port` after it where the port is not 22 and ` (PowerShell)` where
  /// the login answers in PowerShell; `agent @ hub address` for a hub node.
  pub address: String,
  /// Where the credential is kept: an ssh node's key path, or the token file of a hub node's hub.
  pub credential: Option<String>,
  /// A machine of the platform, or a person's own PC.
  pub ownership: Ownership,
  /// The account a personal node belongs to, its id as a decimal number; none on a shared node.
  pub account: Option<String>,
  /// The services the node is reserved for, joined by `+`.
  pub role: Option<String>,
  /// The callers the node lets make a run on it, by name; none where it lets none.
  pub callers: Vec<String>,
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
}

page!(NodePage, Node, "A page of nodes, in the list's order.");

/// Which page of the node list to read: the nodes after the one `after` names, at most `limit` of them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodesQuery {
  /// The name of the last node already read.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<String>,
  /// The most nodes to answer.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
}

/// What a node's removal asks beside the removal.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodeRemoveQuery {
  /// Whether a hub node's agent is sent a leave through its hub first, so it uninstalls itself and departs; the
  /// node is removed only once its hub took the leave.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub leave: Option<bool>,
}

/// A removed node, by its key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodeKey {
  pub name: String,
}

/// How to reach a node, whatever reaches it; the fields of the other connection type are null.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Target {
  pub node_name: String,
  pub kind: NodeKind,
  pub role: Option<String>,
  pub host: Option<String>,
  pub port: Option<u16>,
  pub user: Option<String>,
  /// The private key the login offers, as the row holds it; a leading `~/` is the home of the account that
  /// reads it.
  pub key_path: Option<String>,
  /// The language the node takes a command in: an ssh node's login shell, or a hub node's host's as its hub says
  /// the agent offers shells, `posix` until a pass has read them.
  pub shell: Option<LoginShell>,
  /// The OpenSSH options of a batch login, each given after `-o`, the options a key brings among them where
  /// the node names a key; the same for `ssh`, `sftp` and `scp`. Empty for a hub node.
  pub ssh_options: Vec<String>,
  /// The same login laid out as `ssh` arguments ahead of the command. Empty for a hub node.
  pub ssh_args: Vec<String>,
  pub hub_id: Option<i64>,
  /// The hub's control address.
  pub hub_address: Option<String>,
  /// The file the hub's control token is kept in.
  pub hub_token_file: Option<String>,
  /// The name the hub knows the node's agent by.
  pub agent_name: Option<String>,
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

/// A change to a node, as a JSON merge patch: only the fields sent change, and null clears `keyPath`,
/// `role` and `account`. A node made `shared` loses its account.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeChanges {
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  #[schemars(with = "String")]
  pub label: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  pub role: Option<Option<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  #[schemars(with = "Ownership")]
  pub ownership: Option<Ownership>,
  #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
  pub account: Option<Option<String>>,
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

/// Which nodes to check now: those named, or every node where none is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeCheckAsk {
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub node_names: Vec<String>,
}

/// The nodes a check reached, each as it stands after its check.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodeCheck {
  pub items: Vec<Node>,
}

/// A hub: its control address and the file its token is kept in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Hub {
  pub id: i64,
  /// The hub's control address.
  pub address: String,
  /// The file whose `JAKE_CONTROL_TOKEN` line holds the control token.
  pub token_file: String,
  pub label: Option<String>,
  /// When it was listed, in epoch milliseconds.
  pub created_at: i64,
}

page!(HubPage, Hub, "A page of hubs, in the order they were listed.");

/// A hub to list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewHub {
  pub address: String,
  pub token_file: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub label: Option<String>,
}
