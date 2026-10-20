//! The dependency boundary: among other repositories only the base libraries are depended on. Every crate
//! of the workspace is read out of `cargo metadata`, and each dependency it names is held to the boundary.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

/// The base libraries, the only crates of another repository a crate here may depend on.
const BASE_LIBRARIES: &[&str] = &["bmo-chat", "bmo-chat-client", "bmo-ui"];

fn root() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The workspace as Cargo reads it, every member with the dependencies its manifest names.
fn metadata() -> Value {
  let manifest = root().join("Cargo.toml");
  let output = Command::new(env!("CARGO"))
    .args(["metadata", "--format-version", "1", "--no-deps", "--offline", "--manifest-path"])
    .arg(&manifest)
    .stdin(Stdio::null())
    .output()
    .expect("cargo metadata runs");
  assert!(
    output.status.success(),
    "cargo metadata failed: {}",
    String::from_utf8_lossy(&output.stderr)
  );
  serde_json::from_slice(&output.stdout).expect("cargo metadata answers JSON")
}

fn packages(metadata: &Value) -> &[Value] {
  metadata["packages"].as_array().map(Vec::as_slice).unwrap_or_default()
}

/// Why a dependency crosses the boundary, or none where it stays inside it.
fn crossing(dependency: &Value) -> Option<String> {
  let name = dependency["name"].as_str().unwrap_or_default();

  if BASE_LIBRARIES.contains(&name) {
    return None;
  }

  let path = dependency["path"].as_str();

  // A crate of this workspace carries the unit's own name, as its published client and plugin do.
  if path.is_some_and(|path| Path::new(path).starts_with(root())) {
    return None;
  }

  if name.starts_with("bmo-") || name.starts_with("bmo_") {
    return Some(format!("{name} is another repository's crate"));
  }

  if let Some(path) = path {
    return Some(format!("{name} stands outside the repository, at {path}"));
  }

  match dependency["source"].as_str() {
    Some(source) if source.starts_with("git+") => Some(format!("{name} is fetched by git from {source}")),
    _ => None,
  }
}

#[test]
fn no_crate_depends_on_another_repository_but_the_base_libraries() {
  let metadata = metadata();
  let mut crossings = Vec::new();

  for package in packages(&metadata) {
    let owner = package["name"].as_str().unwrap_or_default();

    for dependency in package["dependencies"].as_array().into_iter().flatten() {
      if let Some(why) = crossing(dependency) {
        crossings.push(format!("{owner}: {why}"));
      }
    }
  }

  assert!(crossings.is_empty(), "dependencies across the boundary: {crossings:#?}");
}

#[test]
fn every_crate_under_crates_is_read() {
  let metadata = metadata();
  let read: Vec<PathBuf> = packages(&metadata)
    .iter()
    .filter_map(|package| package["manifest_path"].as_str().map(PathBuf::from))
    .collect();
  let mut missing = Vec::new();

  for entry in std::fs::read_dir(root().join("crates")).expect("the folder reads").flatten() {
    let manifest = entry.path().join("Cargo.toml");

    if manifest.exists() && !read.contains(&manifest) {
      missing.push(manifest);
    }
  }

  assert!(
    missing.is_empty(),
    "crates outside the workspace, which the boundary does not read: {missing:?}"
  );
}

#[test]
fn a_crossing_is_named() {
  let outside = serde_json::json!({ "name": "elsewhere", "source": null, "path": "/elsewhere/crate" });
  let other = serde_json::json!({ "name": "bmo-job", "source": null, "path": null });
  let fetched =
    serde_json::json!({ "name": "fetched", "source": "git+https://example.com/fetched", "path": null });
  let base = serde_json::json!({ "name": "bmo-chat", "source": null, "path": "/elsewhere/bmo-chat" });
  let own =
    serde_json::json!({ "name": "bmo-node-client", "source": null, "path": root().join("crates/client") });
  let registry = serde_json::json!({ "name": "serde", "source": "registry+https://github.com/rust-lang/crates.io-index", "path": null });

  assert!(crossing(&outside).is_some());
  assert!(crossing(&other).is_some());
  assert!(crossing(&fetched).is_some());
  assert_eq!(crossing(&base), None);
  assert_eq!(crossing(&own), None);
  assert_eq!(crossing(&registry), None);
}
