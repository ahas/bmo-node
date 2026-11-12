//! The service's configuration (docs/api.md, 「Configuration」): a TOML file read at start and held for
//! the service's life.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::stop::DRAIN;

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
    match self.data_root.strip_prefix("~/") {
      Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
      None => PathBuf::from(&self.data_root),
    }
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
