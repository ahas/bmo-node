//! The old archive's node tables moved into this store (docs/store.md, 「The tables」, and the migration
//! plan's table map for TODO #1923): one export directory read, this store written, nothing else touched.
//!
//! The export is the one the item's export hand wrote under
//! `workbench/worker/items/1923/exports/node/`: `manifest.json` with each source table's exact count and
//! key, and one NDJSON file a table. The import reads only that directory, refuses a store already holding
//! rows of its tables, and runs whole inside one transaction, so a dry run and a failed run leave the store
//! as it stood.
//!
//! Two shapes differ from the archive and are settled here rather than by the service's migrations, which
//! have already run by the time a row is read in:
//!
//! - The archive's `node_hubs` becomes `hubs`. The store seeds one hub as its table is made — the same hub
//!   the first migration wrote, and the one the export carries — so that row is written over in place and
//!   the store is refused only where it holds a hub the export does not name.
//! - An old node of kind `jake` is `hub` here, the word the store's own migration gives the rows it
//!   rewrites.
//!
//! The archive's nodes also carry a `token_file` the store's node row has no column for; it stays carried
//! by the hub rows instead.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::{Store, StoreError};

/// One table the import moves: its source name, its target name, and its columns.
pub struct TableMap {
  pub source: &'static str,
  pub target: &'static str,
  pub columns: &'static [&'static str],
}

/// Every table this import moves.
pub const TABLES: &[TableMap] = &[
  TableMap {
    source: "nodes",
    target: "nodes",
    columns: &[
      "name",
      "label",
      "kind",
      "host",
      "port",
      "user",
      "key_path",
      "shell",
      "agent",
      "hub_url",
      "role",
      "position",
      "state",
      "detail",
      "checked_at",
      "connected_at",
      "created_at",
      "updated_at",
    ],
  },
  TableMap {
    source: "node_hubs",
    target: "hubs",
    columns: &["url", "token_file", "created_at"],
  },
  TableMap {
    source: "hardware_samples",
    target: "hardware_samples",
    columns: &["machine", "at", "sample"],
  },
  TableMap {
    source: "hardware_rollups",
    target: "hardware_rollups",
    columns: &["machine", "at", "tally"],
  },
  TableMap {
    source: "settings",
    target: "settings",
    columns: &["key", "value"],
  },
];

/// One export table's entry in the manifest.
#[derive(Clone, Debug)]
pub struct TableEntry {
  pub file: String,
  pub count: u64,
  pub key: Vec<String>,
}

/// The export directory's manifest.
#[derive(Clone, Debug)]
pub struct Manifest {
  pub snapshot: String,
  pub read_at: String,
  pub tables: HashMap<String, TableEntry>,
}

/// One value written to the store.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
  Null,
  Text(String),
}

/// One mapped row.
pub type Mapped = Vec<(&'static str, Cell)>;

/// One table of the report.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
  pub source: String,
  pub exported: u64,
  pub written: u64,
}

/// What an import run did.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
  pub dry_run: bool,
  pub snapshot: String,
  pub read_at: String,
  pub tables: Vec<Moved>,
  pub sample: Sample,
}

impl Report {
  /// The report as the command prints it.
  pub fn lines(&self) -> Vec<String> {
    let mut lines = vec![format!(
      "{} at snapshot {}:",
      if self.dry_run { "dry run" } else { "import" },
      self.snapshot
    )];
    for table in &self.tables {
      lines.push(format!("  {}: {} exported, {} written", table.source, table.exported, table.written));
    }
    lines.push(format!("{} rows sampled, {} failures", self.sample.checked, self.sample.failures.len()));
    lines.extend(self.sample.failures.iter().map(|failure| format!("  {failure}")));

    lines
  }
}

/// The count-and-sample compare's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
  pub checked: usize,
  pub failures: Vec<String>,
}

fn cell(value: Option<&Value>) -> Cell {
  match value {
    None | Some(Value::Null) => Cell::Null,
    Some(Value::String(said)) => Cell::Text(said.clone()),
    Some(Value::Bool(flag)) => Cell::Text(flag.to_string()),
    Some(Value::Number(number)) => Cell::Text(number.to_string()),
    Some(other) => Cell::Text(other.to_string()),
  }
}

/// One source row written in its target's columns, with the node kind's old word settled.
pub fn row(table: &TableMap, old: &Value) -> Mapped {
  table
    .columns
    .iter()
    .map(|column| {
      let mut cell = cell(old.get(*column));
      if table.target == "nodes"
        && *column == "kind"
        && let Cell::Text(kind) = &cell
        && kind == "jake"
      {
        cell = Cell::Text("hub".to_owned());
      }
      (*column, cell)
    })
    .collect()
}

/// The export directory's manifest.
pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
  let path = dir.join("manifest.json");
  let said = std::fs::read_to_string(&path)
    .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
  let document: Value =
    serde_json::from_str(&said).map_err(|error| format!("{} is not JSON: {error}", path.display()))?;
  let mut tables = HashMap::new();
  let list = document.get("tables").and_then(Value::as_array).ok_or("the manifest names no tables")?;
  for table in list {
    let source = table
      .get("source")
      .and_then(Value::as_str)
      .ok_or("a manifest table names no source")?
      .to_owned();
    let file = table
      .get("file")
      .and_then(Value::as_str)
      .ok_or("a manifest table names no file")?
      .to_owned();
    let count = table.get("count").and_then(Value::as_u64).ok_or("a manifest table names no count")?;
    let key: Vec<String> = table
      .get("key")
      .and_then(Value::as_array)
      .map(|keys| keys.iter().filter_map(Value::as_str).map(str::to_owned).collect())
      .unwrap_or_default();
    tables.insert(source, TableEntry { file, count, key });
  }

  Ok(Manifest {
    snapshot: document.get("snapshot").and_then(Value::as_str).unwrap_or_default().to_owned(),
    read_at: document.get("readAt").and_then(Value::as_str).unwrap_or_default().to_owned(),
    tables,
  })
}

/// One table's rows, each checked against the manifest's count as it is read.
pub fn rows(dir: &Path, manifest: &Manifest, source: &str) -> Result<Vec<Value>, String> {
  let entry = manifest
    .tables
    .get(source)
    .ok_or_else(|| format!("the manifest has no table {source}"))?;
  let path = dir.join(&entry.file);
  let said = std::fs::read_to_string(&path)
    .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
  let mut out = Vec::new();
  for (number, line) in said.lines().enumerate() {
    if line.trim().is_empty() {
      continue;
    }
    let row: Value = serde_json::from_str(line)
      .map_err(|error| format!("{} line {} is not JSON: {error}", path.display(), number + 1))?;
    out.push(row);
  }
  if out.len() as u64 != entry.count {
    return Err(format!(
      "{}: the manifest says {} rows and the file holds {}",
      path.display(),
      entry.count,
      out.len()
    ));
  }

  Ok(out)
}

/// The export read and mapped, before anything is written.
pub struct Prepared {
  pub manifest: Manifest,
  pub tables: Vec<(&'static str, &'static str, Vec<Mapped>)>,
}

/// Reads the export and maps every row to its target's shape.
pub fn prepare(export: &Path) -> Result<Prepared, String> {
  let manifest = read_manifest(export)?;
  let mut tables = Vec::new();
  for table in TABLES {
    let old = rows(export, &manifest, table.source)?;
    let mapped: Vec<_> = old.iter().map(|old| row(table, old)).collect();
    tables.push((table.source, table.target, mapped));
  }

  Ok(Prepared { manifest, tables })
}

/// The store's error as the import's sentence.
fn failed(error: StoreError) -> String {
  error.to_string()
}

/// Refuses a store that already holds rows of any table this import writes. The hub seed row is the
/// store's own, so it is allowed where it is the hub the export names, and written over.
pub fn refused_if_holding(store: &Store, prepared: &Prepared) -> Result<(), String> {
  let connection = store.connection();
  let mut holding = Vec::new();
  for table in TABLES {
    let count: i64 = connection
      .query_row(&format!("SELECT COUNT(*) FROM \"{}\"", table.target), [], |row| row.get(0))
      .map_err(|error| format!("{} could not be counted: {error}", table.target))?;
    if count == 0 {
      continue;
    }
    if table.target == "hubs" {
      let exported: Vec<String> = prepared
        .tables
        .iter()
        .find(|(source, _, _)| *source == "node_hubs")
        .map(|(_, _, rows)| {
          rows
            .iter()
            .filter_map(|row| {
              row.iter().find(|(column, _)| *column == "url").and_then(|(_, cell)| match cell {
                Cell::Text(url) => Some(url.clone()),
                Cell::Null => None,
              })
            })
            .collect()
        })
        .unwrap_or_default();
      let held: Vec<String> = connection
        .prepare("SELECT url FROM hubs")
        .and_then(|mut statement| {
          statement.query_map([], |row| row.get(0))?.collect::<Result<Vec<String>, _>>()
        })
        .map_err(|error| format!("hubs could not be read: {error}"))?;
      if count == 1 && exported.len() == 1 && held == exported {
        continue;
      }
      holding.push(format!("hubs holds {count} row(s) the export does not name"));
      continue;
    }
    holding.push(format!("{} holds {count}", table.target));
  }
  if holding.is_empty() {
    Ok(())
  } else {
    Err(format!(
      "the store already holds rows of its tables, so nothing was written: {}",
      holding.join(", ")
    ))
  }
}

fn insert(
  connection: &rusqlite::Connection,
  table: &str,
  row: &[(&'static str, Cell)],
) -> Result<(), String> {
  let columns = row.iter().map(|(column, _)| format!("\"{column}\"")).collect::<Vec<_>>().join(", ");
  let values = (1..=row.len()).map(|position| format!("?{position}")).collect::<Vec<_>>().join(", ");
  let bound: Vec<rusqlite::types::Value> = row
    .iter()
    .map(|(_, cell)| match cell {
      Cell::Null => rusqlite::types::Value::Null,
      Cell::Text(said) => rusqlite::types::Value::Text(said.clone()),
    })
    .collect();
  let sql = if table == "hubs" {
    // The store's seeded hub is the row the export carries: it is written over in place.
    format!(
      "INSERT INTO \"hubs\" ({columns}) VALUES ({values}) \
       ON CONFLICT(url) DO UPDATE SET token_file = excluded.token_file, created_at = excluded.created_at"
    )
  } else {
    format!("INSERT INTO \"{table}\" ({columns}) VALUES ({values})")
  };
  connection
    .execute(&sql, rusqlite::params_from_iter(bound))
    .map_err(|error| format!("{table} could not be written: {error}"))?;

  Ok(())
}

// A stable digest of a row's key, so the same rows are sampled however the file is ordered; FNV-1a.
fn digest(said: &str) -> u64 {
  let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
  for byte in said.as_bytes() {
    hash ^= u64::from(*byte);
    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
  }

  hash
}

fn key_text(row: &[(&'static str, Cell)], key: &[String]) -> String {
  key
    .iter()
    .map(|column| {
      row
        .iter()
        .find(|(held, _)| held == column)
        .map(|(_, cell)| format!("{cell:?}"))
        .unwrap_or_default()
    })
    .collect::<Vec<_>>()
    .join("|")
}

fn sample<'a>(rows: &'a [Mapped], key: &[String]) -> Vec<&'a Mapped> {
  if rows.len() <= 20 {
    return rows.iter().collect();
  }
  let mut ordered: Vec<(u64, &Mapped)> =
    rows.iter().map(|row| (digest(&key_text(row, key)), row)).collect();
  ordered.sort_by_key(|(hash, _)| *hash);
  let mut picked: Vec<&Mapped> = vec![&rows[0], &rows[rows.len() - 1]];
  for (_, row) in ordered {
    if !picked.iter().any(|held| std::ptr::eq(*held, row)) {
      picked.push(row);
    }
    if picked.len() >= 22 {
      break;
    }
  }

  picked
}

fn compare(store: &Store, prepared: &Prepared) -> Sample {
  let mut failures = Vec::new();
  let mut checked = 0;
  let connection = store.connection();
  for (source, target, rows) in &prepared.tables {
    let entry = match prepared.manifest.tables.get(*source) {
      Some(entry) => entry,
      None => continue,
    };
    match connection
      .query_row(&format!("SELECT COUNT(*) FROM \"{target}\""), [], |row| row.get::<_, i64>(0))
    {
      Ok(held) if held as u64 == entry.count => {}
      Ok(held) => {
        failures.push(format!("{source}: the export holds {} rows and {target} holds {held}", entry.count))
      }
      Err(error) => failures.push(format!("{source}: {target} could not be counted: {error}")),
    }
    if entry.key.is_empty() {
      failures.push(format!("{source}: the export records no key to sample by"));
      continue;
    }
    for row in sample(rows, &entry.key) {
      checked += 1;
      let selected = row
        .iter()
        .map(|(column, _)| format!("CAST(\"{column}\" AS TEXT) AS \"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
      let mut sql = format!("SELECT {selected} FROM \"{target}\" WHERE ");
      for (position, column) in entry.key.iter().enumerate() {
        if position > 0 {
          sql.push_str(" AND ");
        }
        sql.push_str(&format!("\"{column}\" = ?{}", position + 1));
      }
      let mut bound = Vec::new();
      for column in &entry.key {
        match row.iter().find(|(held, _)| held == column).map(|(_, cell)| cell) {
          Some(Cell::Null) => {
            failures.push(format!("{source}: the sampled row carries no {column}"));
            bound.clear();
            break;
          }
          Some(Cell::Text(said)) => bound.push(rusqlite::types::Value::Text(said.clone())),
          None => {
            failures.push(format!("{source}: the mapped row carries no {column}"));
            bound.clear();
            break;
          }
        }
      }
      if bound.len() != entry.key.len() {
        continue;
      }
      let read = connection.query_row(&sql, rusqlite::params_from_iter(bound), |held| {
        let mut values = Vec::new();
        for (column, _) in row {
          let value: Option<String> = held.get(*column)?;
          values.push(value);
        }
        Ok(values)
      });
      match read {
        Ok(values) => {
          for ((column, cell), actual) in row.iter().zip(values) {
            let expected = match cell {
              Cell::Null => None,
              Cell::Text(said) => Some(said.clone()),
            };
            if actual != expected {
              failures
                .push(format!("{source} -> {target}: {column} was written {expected:?} but is {actual:?}"));
            }
          }
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
          failures.push(format!("{source} -> {target}: a sampled row is missing"))
        }
        Err(error) => failures.push(format!("{source} -> {target}: the sample could not be read: {error}")),
      }
    }
  }

  Sample { checked, failures }
}

/// Runs the import: reads the export, refuses a store holding rows, writes every row, and compares what it
/// wrote. A dry run rolls the transaction back and reports what would have been written.
pub fn run(database: &Path, export: &Path, dry_run: bool) -> Result<Report, String> {
  let prepared = prepare(export)?;
  // The store's own folder, made where it does not stand, then its tables and migrations.
  if let Some(folder) = database.parent().filter(|folder| !folder.as_os_str().is_empty()) {
    std::fs::create_dir_all(folder)
      .map_err(|error| format!("{} could not be made: {error}", folder.display()))?;
  }
  let store = Store::open(database).map_err(failed)?;
  refused_if_holding(&store, &prepared)?;

  {
    let connection = store.connection();
    let tx = connection
      .unchecked_transaction()
      .map_err(|error| format!("the transaction could not be opened: {error}"))?;
    for (_, target, rows) in &prepared.tables {
      for row in rows {
        insert(&tx, target, row)?;
      }
    }
    if dry_run {
      tx.rollback()
        .map_err(|error| format!("the dry run could not be rolled back: {error}"))?;
    } else {
      tx.commit().map_err(|error| format!("the import could not be committed: {error}"))?;
    }
  }

  let sample = if dry_run {
    Sample {
      checked: 0,
      failures: Vec::new(),
    }
  } else {
    compare(&store, &prepared)
  };

  let tables: Vec<Moved> = prepared
    .tables
    .iter()
    .map(|(source, _target, rows)| Moved {
      source: (*source).to_owned(),
      exported: prepared.manifest.tables.get(*source).map(|entry| entry.count).unwrap_or(0),
      written: rows.len() as u64,
    })
    .collect();

  Ok(Report {
    dry_run,
    snapshot: prepared.manifest.snapshot.clone(),
    read_at: prepared.manifest.read_at.clone(),
    tables,
    sample,
  })
}
