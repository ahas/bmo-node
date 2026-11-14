//! A run (docs/api.md, 「Runs」): one command a caller runs on a hub node for a person whose own node it is,
//! and the command's end, answered in the shape the hosted tool answers it.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::nodes::LoginShell;

/// A shell a hub's agent runs a command in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RunShell {
  Powershell,
  Wsl,
  Linux,
}

impl RunShell {
  /// The code the hub's exec and every answer hold.
  pub fn name(self) -> &'static str {
    match self {
      Self::Powershell => "powershell",
      Self::Wsl => "wsl",
      Self::Linux => "linux",
    }
  }

  /// The shell a node's host takes a command in where a run names none: PowerShell on a Windows host, the
  /// Linux shell otherwise.
  pub fn of_host(shell: Option<LoginShell>) -> Self {
    match shell {
      Some(LoginShell::Powershell) => Self::Powershell,
      Some(LoginShell::Posix) | None => Self::Linux,
    }
  }
}

/// One command to run on a node, for the person whose account is named.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunAsk {
  /// The person the caller acts for, as the account's id written out; the node must be theirs.
  pub account: String,
  pub command: String,
  /// The shell the node's host takes a command in where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub shell: Option<RunShell>,
  /// The folder the command runs in.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub cwd: Option<String>,
  /// Variables laid over the agent's environment.
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
  pub env: BTreeMap<String, String>,
  /// The WSL distribution a `wsl` command runs in.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub distro: Option<String>,
}

/// A command that ran to its end: how it ended and the last part of what it said.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Run {
  /// The node it ran on.
  pub node: String,
  /// The shell it ran in.
  pub shell: String,
  /// Its exit code; null where the hub reports none.
  pub exit_code: Option<i64>,
  /// The signal that ended it, where one did.
  pub signal: Option<String>,
  /// The last 64 KiB of its standard output.
  pub stdout: String,
  /// The last 64 KiB of its standard error.
  pub stderr: String,
  /// Whether more standard output came than `stdout` keeps.
  pub stdout_cut: bool,
  /// Whether more standard error came than `stderr` keeps.
  pub stderr_cut: bool,
}
