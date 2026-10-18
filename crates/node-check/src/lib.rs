//! Whether each node answers (docs/nodes.md, 「How a node's state is known」): one login that runs
//! nothing for an ssh node, the agents its hub says are attached for a jake node. The service and the
//! command call it as a library, and a pass writes what it answers to the node's store. A credential is
//! read for the one request that needs it and is never part of what a check answers.

pub mod hub;
pub mod pass;
pub mod rows;
pub mod ssh;
mod store_rows;

pub use store_rows::check_row;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub use policy::node_states::NodeState;
use serde::{Deserialize, Serialize};

/// What a check met: the state, and a line saying why where there is one to say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Met {
  pub state: NodeState,
  pub detail: Option<String>,
}

impl Met {
  pub fn new(state: NodeState, detail: impl Into<String>) -> Self {
    Self {
      state,
      detail: Some(detail.into()),
    }
  }

  pub fn connected() -> Self {
    Self {
      state: NodeState::Connected,
      detail: None,
    }
  }

  /// The check as a row records it, made at `at`.
  pub fn at(self, at: i64) -> NodeCheck {
    NodeCheck {
      state: self.state,
      detail: self.detail,
      at,
    }
  }
}

/// One check's outcome as a row records it, `at` in seconds since the epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCheck {
  pub state: NodeState,
  pub detail: Option<String>,
  pub at: i64,
}

/// The machine a check runs on: the OpenSSH it starts, the home a row's `~/` stands for, and the
/// client it asks a hub with.
#[derive(Clone)]
pub struct Machine {
  pub ssh: PathBuf,
  pub home: PathBuf,
  http: reqwest::Client,
}

impl Machine {
  pub fn new(ssh: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Self {
    Self {
      ssh: ssh.into(),
      home: home.into(),
      http: reqwest::Client::new(),
    }
  }

  /// This process's own: the PATH's OpenSSH and the home it runs under.
  pub fn local() -> Self {
    Self::new("ssh", std::env::var_os("HOME").unwrap_or_default())
  }

  /// A path as a row writes it, `~/` standing for the home.
  pub fn expand_home(&self, path: &str) -> PathBuf {
    match path {
      "~" => self.home.clone(),
      _ => match path.strip_prefix("~/") {
        Some(rest) => self.home.join(rest),
        None => PathBuf::from(path),
      },
    }
  }
}

/// Now, in the seconds since the epoch the store's node columns keep.
pub fn now_seconds() -> i64 {
  SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

// The most of a line of OpenSSH's or a hub's a detail carries.
const DETAIL_MOST: usize = 200;

/// The last line of what was said that holds anything, cut to what a detail carries.
pub(crate) fn last_line(text: &str) -> Option<String> {
  text
    .lines()
    .map(str::trim)
    .rfind(|line| !line.is_empty())
    .map(|line| line.chars().take(DETAIL_MOST).collect())
}
