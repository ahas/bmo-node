//! The old archive's node tables moved into the store (TODO #1923): the mapping, the seeded hub written
//! over, the refusal of a store already holding rows, the dry run, and the count-and-sample compare.

use std::path::Path;

use store::Store;

fn write(folder: &Path, tables: [(&str, &str, &[&str]); 5]) {
  let mut manifest = Vec::new();
  for (source, key, rows) in tables {
    manifest.push(format!(
      r#"{{"source":"{source}","file":"{source}.ndjson","count":{},"key":[{key}]}}"#,
      rows.len()
    ));
    std::fs::write(folder.join(format!("{source}.ndjson")), rows.join("\n") + "\n")
      .expect("the rows are written");
  }
  let said = format!(
    r#"{{"snapshot":"1:2:","readAt":"2026-10-21T17:00:00+09:00","tables":[{}]}}"#,
    manifest.join(",")
  );
  std::fs::write(folder.join("manifest.json"), said).expect("the manifest is written");
}

const NODE: &str = r#"{"name":"jake-wsl","label":"jake_wsl","kind":"jake","host":null,"port":null,"user":null,"key_path":null,"token_file":null,"position":2,"state":"connected","detail":null,"checked_at":1792569579,"connected_at":1792569579,"created_at":1790048843,"updated_at":1790048843,"role":null,"shell":null,"agent":"jake_wsl","hub_url":"http://127.0.0.1:8791","rowid":1}"#;
const HUB: &str = r#"{"url":"http://127.0.0.1:8791","token_file":"~/.local/share/jake/hub.env","created_at":1790048843,"rowid":1}"#;
const SETTING: &str = r#"{"key":"hardware_cpu_limit:local","value":"95","rowid":1}"#;

fn full(folder: &Path) {
  write(
    folder,
    [
      ("nodes", "\"name\"", &[NODE]),
      ("node_hubs", "\"url\"", &[HUB]),
      (
        "hardware_samples",
        "\"machine\",\"at\"",
        &[r#"{"machine":"local","at":60,"sample":"{}","rowid":1}"#],
      ),
      (
        "hardware_rollups",
        "\"machine\",\"at\"",
        &[r#"{"machine":"local","at":0,"tally":"{}","rowid":1}"#],
      ),
      ("settings", "\"key\"", &[SETTING]),
    ],
  );
}

#[test]
fn the_tables_are_moved_over_the_seeded_hub_and_compared() {
  let dir = tempfile::tempdir().expect("a scratch folder");
  full(dir.path());

  let database = dir.path().join("node/node.db");
  let report = store::import::run(&database, dir.path(), false).expect("the import runs");

  assert_eq!(report.tables.iter().map(|table| table.written).collect::<Vec<_>>(), [1, 1, 1, 1, 1]);
  assert_eq!(report.sample.checked, 5);
  assert_eq!(report.sample.failures, Vec::<String>::new());

  let store = Store::open(&database).expect("the store opens");
  let nodes = store.nodes().expect("the nodes are read");
  assert_eq!(nodes.len(), 1);
  assert_eq!(nodes[0].kind.name(), "hub");
  let hubs = store.hubs().expect("the hubs are read");
  assert_eq!(hubs.len(), 1);
  assert_eq!(hubs[0].url, "http://127.0.0.1:8791");
  assert_eq!(hubs[0].created_at, 1790048843);
}

#[test]
fn a_dry_run_writes_nothing() {
  let dir = tempfile::tempdir().expect("a scratch folder");
  full(dir.path());

  let database = dir.path().join("node/node.db");
  store::import::run(&database, dir.path(), true).expect("the dry run runs");

  let store = Store::open(&database).expect("the store opens");
  assert!(store.nodes().expect("the nodes are read").is_empty());
  assert_eq!(store.hubs().expect("the hubs are read").len(), 1);
}

#[test]
fn a_store_holding_rows_is_refused() {
  let dir = tempfile::tempdir().expect("a scratch folder");
  full(dir.path());

  let database = dir.path().join("node/node.db");
  store::import::run(&database, dir.path(), false).expect("the first import runs");
  let refused = store::import::run(&database, dir.path(), false).expect_err("the second is refused");
  assert!(refused.contains("already holds rows"), "{refused}");
  assert!(refused.contains("nodes holds 1"), "{refused}");
}

#[test]
fn a_store_holding_a_hub_the_export_does_not_name_is_refused() {
  let dir = tempfile::tempdir().expect("a scratch folder");
  write(
    dir.path(),
    [
      ("nodes", "\"name\"", &[]),
      (
        "node_hubs",
        "\"url\"",
        &[r#"{"url":"http://elsewhere:8791","token_file":"/tmp/hub.env","created_at":1,"rowid":1}"#],
      ),
      ("hardware_samples", "\"machine\",\"at\"", &[]),
      ("hardware_rollups", "\"machine\",\"at\"", &[]),
      ("settings", "\"key\"", &[]),
    ],
  );

  let database = dir.path().join("node/node.db");
  let refused = store::import::run(&database, dir.path(), false).expect_err("the store is refused");
  assert!(refused.contains("hubs holds 1 row(s) the export does not name"), "{refused}");
}
