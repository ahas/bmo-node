//! The store's tables, made where they are missing, from the 1.0.0 baseline (docs/store.md, 「The tables」).
//! The schema's version is the database's own `user_version`: the baseline's is 5, the version an install
//! from the last release before 1.0.0 holds already.

use rusqlite::{Connection, params};

/// The hub every store starts with: the local hub and the environment file its token stands in. It is
/// written once, when the tables are made, so a hub removed stays removed.
pub const HUB_SEED: (&str, &str) = ("http://127.0.0.1:8791", "~/.local/share/jake/hub.env");

/// The baseline's version.
pub const BASELINE: i64 = 5;

/// The tables made where they are missing, with the seed hub; a store of an older version, from a release
/// before 1.0.0 short of its last, refused, and one of a later version too.
pub fn settle(connection: &mut Connection, now: i64) -> rusqlite::Result<()> {
  let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

  if version == 0 {
    let unit = connection.transaction()?;

    unit.execute_batch(include_str!("baseline.sql"))?;
    unit.execute(
      "INSERT INTO hubs (url, token_file, created_at) VALUES (?1, ?2, ?3)",
      params![HUB_SEED.0, HUB_SEED.1, now],
    )?;
    unit.pragma_update(None, "user_version", BASELINE)?;
    unit.commit()?;
  } else if version != BASELINE {
    let why = if version < BASELINE {
      format!(
        "the store's schema is version {version}, from a release before 1.0.0 short of its last; that release \
         brings it to version {BASELINE} first"
      )
    } else {
      format!("the store's schema is version {version}, and this build reads version {BASELINE}")
    };
    return Err(rusqlite::Error::SqliteFailure(
      rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISMATCH),
      Some(why),
    ));
  }

  Ok(())
}
