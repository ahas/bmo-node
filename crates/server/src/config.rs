//! The service's configuration (docs/api.md, 「Configuration」): a TOML file read at start and held for
//! the service's life.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use policy::nodes::{MonitoredAccounts, PersonalCallers, is_node_name};
use serde::Deserialize;

use crate::owners::OwnersRead;
use crate::stop::DRAIN;

/// A caller with a token of its own, which makes a run as itself (docs/api.md, 「Runs」).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallerConfig {
  /// The file holding the caller's bearer token.
  pub token_file: String,
  /// Whether every personal hub node is written with the caller as it joins the list.
  #[serde(default)]
  pub personal_nodes: bool,
}

/// Where a node's owners are read for a run (docs/nodes.md, 「A person's node」).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnersConfig {
  /// The address asked, `{nodeName}` and `{caller}` standing for the node's and the caller's names.
  pub url: String,
  /// The file holding the bearer token presented there.
  pub token_file: String,
}

/// A caller named by its own token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
  pub name: String,
  pub token: String,
}

/// A path as a setting writes it, `~/` standing for the home.
pub fn home_path(path: &str) -> PathBuf {
  match path.strip_prefix("~/") {
    Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
    None => PathBuf::from(path),
  }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
  /// Where the service answers, as `host:port`.
  pub listen: String,
  /// The bearer token every caller presents.
  pub token: String,
  /// The shared data root; node's own files stand in its `node/` folder.
  pub data_root: String,
  /// How this machine is shown, where none is set the system's host name.
  #[serde(default)]
  pub machine_label: Option<String>,
  /// The environment variable a process's scope is read from; where none is set, no process has a scope.
  #[serde(default)]
  pub scope_variable: Option<String>,
  /// How long a call under way is given to finish once the service is told to stop, in milliseconds.
  #[serde(default)]
  pub drain_ms: Option<u64>,
  /// The callers with a token of their own, by name; none makes a run or moves a file where none is set.
  #[serde(default)]
  pub callers: BTreeMap<String, CallerConfig>,
  /// Where a node's owners are read for a run or a file transfer; nothing is let through where none is set.
  #[serde(default)]
  pub owners: Option<OwnersConfig>,
  /// The accounts whose personal nodes the hardware monitor reads; none is monitored where none is set.
  #[serde(default)]
  pub monitored_accounts: MonitoredAccounts,
}

impl Config {
  /// The configuration a file holds.
  pub fn read(path: &Path) -> Result<Self, String> {
    let text = std::fs::read_to_string(path)
      .map_err(|why| format!("the configuration {} could not be read: {why}", path.display()))?;

    toml::from_str(&text)
      .map_err(|why| format!("the configuration {} does not read: {why}", path.display()))
  }

  /// How long a call under way is given to finish once the service is told to stop.
  pub fn drain(&self) -> Duration {
    self.drain_ms.map_or(DRAIN, Duration::from_millis)
  }

  /// The data root, `~/` standing for the home.
  pub fn data_root(&self) -> PathBuf {
    home_path(&self.data_root)
  }

  /// Every caller with its token read out of its file, or why the service does not start: a name the rule
  /// refuses, a file that cannot be read or holds no token, or a token that is the service's own or another
  /// caller's.
  pub fn callers(&self) -> Result<Vec<Caller>, String> {
    let mut callers: Vec<Caller> = Vec::new();

    for (name, caller) in &self.callers {
      if !is_node_name(name) {
        return Err(format!(
          "the caller \"{name}\" is not lowercase letters, digits and hyphens of at most 32 characters"
        ));
      }

      let path = home_path(&caller.token_file);
      let token = std::fs::read_to_string(&path)
        .map_err(|why| {
          format!("the caller {name}'s token file {} could not be read: {why}", path.display())
        })?
        .trim()
        .to_owned();

      if token.is_empty() {
        return Err(format!("the caller {name}'s token file {} holds no token", path.display()));
      }

      if token == self.token || callers.iter().any(|other| other.token == token) {
        return Err(format!(
          "the caller {name}'s token is the service's own or another caller's; each caller holds its own"
        ));
      }

      callers.push(Caller {
        name: name.clone(),
        token,
      });
    }

    Ok(callers)
  }

  /// The callers every personal hub node is written with as it joins the list (docs/nodes.md, 「A node's
  /// callers」), or why the service does not start: a name the rule refuses.
  pub fn personal_callers(&self) -> Result<PersonalCallers, String> {
    PersonalCallers::try_from(
      self
        .callers
        .iter()
        .filter(|(_, caller)| caller.personal_nodes)
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>(),
    )
  }

  /// Where a node's owners are read, where the configuration names it.
  pub fn owners_read(&self) -> Option<OwnersRead> {
    self.owners.as_ref().map(|owners| OwnersRead {
      url: owners.url.clone(),
      token_file: home_path(&owners.token_file),
    })
  }

  /// The address a caller on this machine reaches the service at: an unspecified host as the loopback.
  pub fn local_address(&self) -> String {
    let (host, port) = self.listen.rsplit_once(':').unwrap_or((self.listen.as_str(), ""));
    let host = match host {
      "0.0.0.0" | "" => "127.0.0.1",
      "[::]" | "::" => "[::1]",
      other => other,
    };

    format!("http://{host}:{port}")
  }
}
