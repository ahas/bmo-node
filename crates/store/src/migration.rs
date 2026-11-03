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

// A hub keyed by an id of its own, never given again once used, with a label beside its address; a node
// reached through a hub named by the connection type's own code; the disk's line kept as the alert line it
// now is, and the launch limits and the machine look's old marks of what it told let go of.
const SECOND: &str = r#"
CREATE TABLE hubs_keyed (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  url TEXT NOT NULL UNIQUE,
  token_file TEXT NOT NULL,
  label TEXT,
  created_at INTEGER NOT NULL
);
INSERT INTO hubs_keyed (url, token_file, created_at) SELECT url, token_file, created_at FROM hubs ORDER BY created_at, url;
DROP TABLE hubs;
ALTER TABLE hubs_keyed RENAME TO hubs;
UPDATE nodes SET kind = 'hub' WHERE kind = 'jake';
UPDATE settings SET key = 'machine_limit:alert_disk_free_gb' WHERE key = 'machine_limit:launch_disk_free_gb';
DELETE FROM settings WHERE key IN ('machine_limit:launch_memory_full_pct', 'machine_limit:launch_memory_available_gb',
  'machine_limit:launch_load_per_thread', 'machine_pressure_told', 'machine_disk_told');
"#;

// Who held each machine's memory, one sample per machine per minute.
const THIRD: &str = r#"
CREATE TABLE memory_samples (
  machine TEXT NOT NULL,
  at INTEGER NOT NULL,
  sample TEXT NOT NULL,
  PRIMARY KEY (machine, at)
);
CREATE INDEX memory_samples_at ON memory_samples (at);
"#;

/// Every step not yet run, run, and the version moved to the last.
pub fn settle(connection: &mut Connection, now: i64) -> rusqlite::Result<()> {
  let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

  if version < 1 {
    let unit = connection.transaction()?;

    unit.execute_batch(FIRST)?;
    unit.execute(
      "INSERT INTO hubs (url, token_file, created_at) VALUES (?1, ?2, ?3)",
      params![HUB_SEED.0, HUB_SEED.1, now],
    )?;
    unit.pragma_update(None, "user_version", 1)?;
    unit.commit()?;
  }

  if version < 2 {
    let unit = connection.transaction()?;

    unit.execute_batch(SECOND)?;
    unit.pragma_update(None, "user_version", 2)?;
    unit.commit()?;
  }

  if version < 3 {
    let unit = connection.transaction()?;

    unit.execute_batch(THIRD)?;
    unit.pragma_update(None, "user_version", 3)?;
    unit.commit()?;
  }

  Ok(())
}
