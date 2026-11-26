//! Whether each node answers (docs/nodes.md, 「How a node's state is known」): one login that runs
//! nothing for an ssh node, the agents its hub says are attached for a jake node. The service and the
//! command call it as a library, and a pass writes what it answers to the node's store. A credential is
//! read for the one request that needs it and is never part of what a check answers.

pub mod hub;
pub mod hub_exec;
pub mod hub_files;
pub mod pass;
pub mod rows;
pub mod ssh;
mod store_rows;

pub use store_rows::check_row;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

// How often a hub's connection asks whether the hub is still there once it has gone quiet, and how many
// asks in a row may go unanswered before the connection is taken for broken: the same as an ssh login's
// keep-alive (`policy/files/ssh.toml`).
const KEEP_ALIVE: Duration = Duration::from_secs(15);
const KEEP_ALIVE_MISSES: u32 = 3;

/// A client for a hub, whose requests wait for the hub's answer and whose connections carry a keep-alive
/// that tells a hub gone.
fn hub_client() -> reqwest::Client {
  reqwest::Client::builder()
    .tcp_keepalive(KEEP_ALIVE)
    .tcp_keepalive_interval(KEEP_ALIVE)
    .tcp_keepalive_retries(KEEP_ALIVE_MISSES)
    .build()
    .unwrap_or_default()
}

/// The checks still out on a machine, each by what it asks: a node, or a hub. A whole pass leaves to a
/// check still out what it asks.
#[derive(Clone, Default)]
pub struct Out(Arc<Mutex<HashSet<String>>>);

/// One check out, given back when it is dropped.
pub struct Asking {
  out: Out,
  key: String,
}

impl Out {
  /// The check of `key`, taken out; none where one is out already.
  pub fn take(&self, key: String) -> Option<Asking> {
    let taken = self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(key.clone());

    taken.then(|| Asking {
      out: self.clone(),
      key,
    })
  }
}

impl Drop for Asking {
  fn drop(&mut self) {
    self.out.0.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.key);
  }
}

/// The machine a check runs on: the OpenSSH it starts, the home a row's `~/` stands for, the client it
/// asks a hub with, and the checks it has out.
#[derive(Clone)]
pub struct Machine {
  pub ssh: PathBuf,
  pub home: PathBuf,
  http: reqwest::Client,
  pub out: Out,
}

impl Machine {
  pub fn new(ssh: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Self {
    Self {
      ssh: ssh.into(),
      home: home.into(),
      http: hub_client(),
      out: Out::default(),
    }
  }

  /// The client a hub is asked with.
  pub fn http(&self) -> &reqwest::Client {
    &self.http
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
