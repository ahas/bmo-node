//! A workspace binary as a test starts it: built once per test binary by the cargo running the tests,
//! into the target directory and the profile the test binary itself was built in.
//!
//! Cargo names a binary's path to the tests of its own package only, and every test stands in the root
//! package, so a test builds the binary it starts and finds it where cargo puts it.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

static BUILT: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

// The directory the test binary was built into, `<target>/<profile>/deps`, gives the target directory and
// the profile.
fn profile_dir() -> PathBuf {
  let exe = std::env::current_exe().expect("the test binary has a path");
  let deps = exe.parent().expect("the test binary stands in a directory");

  deps.parent().expect("the deps directory stands in its profile's").to_owned()
}

fn profile(dir: &Path) -> String {
  match dir.file_name().and_then(|name| name.to_str()) {
    Some("debug") | None => "dev".to_owned(),
    Some(name) => name.to_owned(),
  }
}

/// The path of `binary` of the workspace package `package`, built now where this test binary has not
/// built it yet.
pub fn built(package: &str, binary: &str) -> PathBuf {
  let mut built = BUILT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
  let built = built.get_or_insert_with(HashMap::new);

  if let Some(path) = built.get(binary) {
    return path.clone();
  }

  let dir = profile_dir();
  let target = dir.parent().expect("the profile's directory stands in the target directory");
  let status = Command::new(env!("CARGO"))
    .args(["build", "--quiet", "--package", package, "--bin", binary, "--profile"])
    .arg(profile(&dir))
    .arg("--target-dir")
    .arg(target)
    .arg("--manifest-path")
    .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
    .status()
    .expect("cargo starts");

  assert!(status.success(), "{binary} of {package} did not build");

  let path = dir.join(binary);

  built.insert(binary.to_owned(), path.clone());
  path
}
