//! The store's tables, made where they are missing (docs/store.md, 「The tables」). The schema's version is
//! the database's own `user_version`; each step runs once, in order, inside one transaction.

use rusqlite::{Connection, params};

/// The hub every store starts with: the local hub and the environment file its token stands in. It is
/// written once, when the table is made, so a hub removed stays removed.
pub const HUB_SEED: (&str, &str) = ("http://127.0.0.1:8791", "~/.local/share/jake/hub.env");

const FIRST: &str = r#"
CREATE TABLE nodes (
  name TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  kind TEXT NOT NULL,
  host TEXT,
  port INTEGER,
  user TEXT,
  key_path TEXT,
  shell TEXT,
  agent TEXT,
  hub_url TEXT,
  role TEXT,
  position INTEGER NOT NULL,
  state TEXT NOT NULL DEFAULT 'unknown',
  detail TEXT,
  checked_at INTEGER,
  connected_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE hubs (
  url TEXT PRIMARY KEY,
  token_file TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE hardware_samples (
  machine TEXT NOT NULL,
  at INTEGER NOT NULL,
  sample TEXT NOT NULL,
  PRIMARY KEY (machine, at)
);
CREATE INDEX hardware_samples_at ON hardware_samples (at);
CREATE TABLE hardware_rollups (
  machine TEXT NOT NULL,
  at INTEGER NOT NULL,
  tally TEXT NOT NULL,
  PRIMARY KEY (machine, at)
);
CREATE INDEX hardware_rollups_at ON hardware_rollups (at);
CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

/// Every step not yet run, run, and the version moved to the last.
pub fn settle(connection: &mut Connection, now: i64) -> rusqlite::Result<()> {
  let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

  if version >= 1 {
    return Ok(());
  }

  let unit = connection.transaction()?;

  unit.execute_batch(FIRST)?;
  unit.execute(
    "INSERT INTO hubs (url, token_file, created_at) VALUES (?1, ?2, ?3)",
    params![HUB_SEED.0, HUB_SEED.1, now],
  )?;
  unit.pragma_update(None, "user_version", 1)?;
  unit.commit()
}
