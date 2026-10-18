//! The node service's policy files and their readers (docs/nodes.md, docs/hardware.md). Each file stands
//! under `files/`, is compiled in, and is read once on first use; a file that does not read as its type is a
//! fault of the build, which its own test meets before anything runs on it.

use serde::de::DeserializeOwned;

pub mod node_states;
pub mod nodes;
pub mod ssh;
pub mod windows_sensors;

/// One part read from its file's text.
pub fn read<T: DeserializeOwned>(file: &str, text: &str) -> T {
  toml::from_str(text).unwrap_or_else(|error| panic!("the node policy's {file} does not read: {error}"))
}

/// A part of the policy, compiled in from its file under `files/` and read once on first use.
macro_rules! part {
  ($vis:vis $name:ident: $type:ty = $file:literal) => {
    $vis static $name: std::sync::LazyLock<$type> = std::sync::LazyLock::new(|| {
      $crate::read($file, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/files/", $file)))
    });
  };
}

pub(crate) use part;
