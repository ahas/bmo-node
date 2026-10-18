//! The service's configuration (docs/api.md, 「Configuration」): a TOML file read at start and held for
//! the service's life.

use std::path::{Path, PathBuf};

use serde::Deserialize;

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
}

impl Config {
  /// The configuration a file holds.
  pub fn read(path: &Path) -> Result<Self, String> {
    let text = std::fs::read_to_string(path)
      .map_err(|why| format!("the configuration {} could not be read: {why}", path.display()))?;

    toml::from_str(&text)
      .map_err(|why| format!("the configuration {} does not read: {why}", path.display()))
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
