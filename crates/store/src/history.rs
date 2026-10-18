//! The hardware history's rows (docs/hardware.md, 「History」): one row per machine per minute in
//! `hardware_samples`, one per half hour in `hardware_rollups`, each its figures as JSON text. What the rows
//! hold, and when they are written and pruned, is the hardware crate's; the store keeps them.

use rusqlite::params;

use crate::{Result, Store};

/// One row: its machine, the start of the stretch it covers, and its figures as JSON text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
  pub machine: String,
  pub at: i64,
  pub figures: String,
}

/// The half hour a moment falls in, as SQL over the column given.
fn grain_of(column: &str, width: i64) -> String {
  format!("({column} - ({column} % {width}))")
}

impl Store {
  /// The start of a machine's last minute row, or none where it has none.
  pub fn last_sample_at(&self, machine: &str) -> Result<Option<i64>> {
    Ok(self.connection().query_row(
      "SELECT MAX(at) FROM hardware_samples WHERE machine = ?1",
      [machine],
      |row| row.get(0),
    )?)
  }

  /// Minute rows of one machine written together, each replacing a row of its minute.
  pub fn put_samples(&self, machine: &str, rows: &[(i64, String)]) -> Result<()> {
    let mut connection = self.connection();
    let unit = connection.transaction()?;

    for (at, figures) in rows {
      unit.execute(
        "INSERT INTO hardware_samples (machine, at, sample) VALUES (?1, ?2, ?3)
         ON CONFLICT (machine, at) DO UPDATE SET sample = excluded.sample",
        params![machine, at, figures],
      )?;
    }

    unit.commit()?;

    Ok(())
  }

  /// A machine's minute rows from `from` up to `until`, the oldest first.
  pub fn samples(&self, machine: &str, from: i64, until: i64) -> Result<Vec<Row>> {
    self.rows("hardware_samples", "sample", machine, from, until)
  }

  /// A machine's minute rows from `from` up to `until` whose stretch of `width` seconds has no rolled-up
  /// row yet, the oldest first.
  pub fn samples_not_rolled_up(
    &self,
    machine: &str,
    from: i64,
    until: i64,
    width: i64,
  ) -> Result<Vec<Row>> {
    let connection = self.connection();
    let mut query = connection.prepare(&format!(
      "SELECT s.machine, s.at, s.sample FROM hardware_samples s
       WHERE s.machine = ?1 AND s.at >= ?2 AND s.at < ?3
       AND NOT EXISTS (SELECT 1 FROM hardware_rollups r WHERE r.machine = s.machine AND r.at = {grain})
       ORDER BY s.at",
      grain = grain_of("s.at", width)
    ))?;
    let rows = query
      .query_map(params![machine, from, until], |row| {
        Ok(Row {
          machine: row.get(0)?,
          at: row.get(1)?,
          figures: row.get(2)?,
        })
      })?
      .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
  }

  /// A machine's half-hour rows from `from` up to `until`, the oldest first.
  pub fn rollups(&self, machine: &str, from: i64, until: i64) -> Result<Vec<Row>> {
    self.rows("hardware_rollups", "tally", machine, from, until)
  }

  fn rows(&self, table: &str, column: &str, machine: &str, from: i64, until: i64) -> Result<Vec<Row>> {
    let connection = self.connection();
    let mut query = connection.prepare(&format!(
      "SELECT machine, at, {column} FROM {table} WHERE machine = ?1 AND at >= ?2 AND at < ?3 ORDER BY at"
    ))?;
    let rows = query
      .query_map(params![machine, from, until], |row| {
        Ok(Row {
          machine: row.get(0)?,
          at: row.get(1)?,
          figures: row.get(2)?,
        })
      })?
      .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
  }

  /// The stretches of `width` seconds closed before `closed` that hold minute rows and no rolled-up row
  /// yet, each by its machine and start, the oldest first and at most `limit` of them.
  pub fn rollups_due(&self, closed: i64, width: i64, limit: i64) -> Result<Vec<(String, i64)>> {
    let connection = self.connection();
    let grain = grain_of("s.at", width);
    let mut query = connection.prepare(&format!(
      "SELECT s.machine, {grain} AS grain FROM hardware_samples s
       WHERE s.at < ?1 AND NOT EXISTS (SELECT 1 FROM hardware_rollups r WHERE r.machine = s.machine AND r.at = {grain})
       GROUP BY s.machine, grain ORDER BY grain, s.machine LIMIT ?2"
    ))?;
    let due = query
      .query_map(params![closed, limit], |row| Ok((row.get(0)?, row.get(1)?)))?
      .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(due)
  }

  /// Rolled-up rows written together, each replacing a row of its stretch.
  pub fn put_rollups(&self, rows: &[Row]) -> Result<()> {
    let mut connection = self.connection();
    let unit = connection.transaction()?;

    for row in rows {
      unit.execute(
        "INSERT INTO hardware_rollups (machine, at, tally) VALUES (?1, ?2, ?3)
         ON CONFLICT (machine, at) DO UPDATE SET tally = excluded.tally",
        params![row.machine, row.at, row.figures],
      )?;
    }

    unit.commit()?;

    Ok(())
  }

  /// Rolled-up rows older than `rollups_before` removed, and minute rows older than it whatever became of
  /// them, or older than `rolled_samples_before` where their stretch of `width` seconds is rolled up; how
  /// many rows went.
  pub fn prune_history(
    &self,
    rollups_before: i64,
    rolled_samples_before: i64,
    width: i64,
  ) -> Result<usize> {
    let connection = self.connection();
    let rollups = connection.execute("DELETE FROM hardware_rollups WHERE at < ?1", [rollups_before])?;
    let samples = connection.execute(
      &format!(
        "DELETE FROM hardware_samples WHERE at < ?1 OR (at < ?2 AND EXISTS (SELECT 1 FROM hardware_rollups r
         WHERE r.machine = hardware_samples.machine AND r.at = {grain}))",
        grain = grain_of("hardware_samples.at", width)
      ),
      params![rollups_before, rolled_samples_before],
    )?;

    Ok(rollups + samples)
  }
}
