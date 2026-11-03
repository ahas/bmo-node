//! The live record (docs/hardware.md, 「Live」): every machine's latest reading in full, or the failure
//! its run met, and one row of figures per machine per second, in a SQLite file of its own in node's
//! folder (`store::live_record_path`), which the hardware routes read. It is written on a thread of its
//! own, since a commit waits for the disk and a reading lands every second; it is read over a connection
//! that only reads.

use std::path::Path;
use std::sync::mpsc;

use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Map, Value};

use super::memory::MemorySample;
use super::view::{HardwareView, history_sample};

/// How far back the record reaches (docs/hardware.md, 「Live」): the readings of a span under a minute are
/// drawn out of it.
pub const KEEP_SECONDS: i64 = 15 * 60;

/// The most the record's rows of figures may hold, as the text they are kept in, in step with its
/// reach; the oldest go first.
pub const MOST_BYTES: i64 = KEEP_SECONDS * 16 * 1024;

/// The record as its writer opens it, its tables made where they are missing.
fn open(path: &Path) -> rusqlite::Result<Connection> {
  let record = Connection::open(path)?;

  store::wait_for_locks(&record)?;
  record.pragma_update(None, "journal_mode", "WAL")?;
  record.pragma_update(None, "synchronous", "NORMAL")?;
  record.execute_batch(
    "CREATE TABLE IF NOT EXISTS live_latest (machine TEXT PRIMARY KEY, at INTEGER NOT NULL, view TEXT, failure TEXT);
     CREATE TABLE IF NOT EXISTS live_samples (machine TEXT NOT NULL, at INTEGER NOT NULL, sample TEXT NOT NULL, PRIMARY KEY (machine, at));
     CREATE INDEX IF NOT EXISTS live_samples_at ON live_samples (at);
     CREATE TABLE IF NOT EXISTS live_memory (machine TEXT NOT NULL, at INTEGER NOT NULL, sample TEXT NOT NULL, PRIMARY KEY (machine, at));
     CREATE INDEX IF NOT EXISTS live_memory_at ON live_memory (at);",
  )?;

  Ok(record)
}

enum Write {
  Reading {
    machine: String,
    at: i64,
    view: String,
    sample: String,
  },
  Failure {
    machine: String,
    at: i64,
    detail: String,
  },
  Memory {
    machine: String,
    at: i64,
    sample: String,
  },
  Prune {
    now: i64,
    keep_seconds: i64,
    most_bytes: i64,
  },
  /// Answered once everything handed over before it is written.
  Written(mpsc::Sender<()>),
}

fn write(record: &mut Connection, write: Write) -> rusqlite::Result<()> {
  match write {
    Write::Reading {
      machine,
      at,
      view,
      sample,
    } => {
      let unit = record.transaction()?;

      unit.execute(
        "INSERT OR REPLACE INTO live_latest (machine, at, view, failure) VALUES (?1, ?2, ?3, NULL)",
        params![machine, at, view],
      )?;
      unit.execute(
        "INSERT OR REPLACE INTO live_samples (machine, at, sample) VALUES (?1, ?2, ?3)",
        params![machine, at, sample],
      )?;
      unit.commit()
    }
    Write::Failure {
      machine,
      at,
      detail,
    } => record
      .execute(
        "INSERT OR REPLACE INTO live_latest (machine, at, view, failure) VALUES (?1, ?2, NULL, ?3)",
        params![machine, at, detail],
      )
      .map(drop),
    Write::Memory {
      machine,
      at,
      sample,
    } => record
      .execute(
        "INSERT OR REPLACE INTO live_memory (machine, at, sample) VALUES (?1, ?2, ?3)",
        params![machine, at, sample],
      )
      .map(drop),
    Write::Written(answer) => {
      let _ = answer.send(());

      Ok(())
    }
    Write::Prune {
      now,
      keep_seconds,
      most_bytes,
    } => {
      record.execute("DELETE FROM live_samples WHERE at < ?1", params![now - keep_seconds])?;
      record.execute("DELETE FROM live_memory WHERE at < ?1", params![now - keep_seconds])?;

      let cut: Option<i64> = record
        .query_row(
          "SELECT at FROM (SELECT at, SUM(length(sample)) OVER (ORDER BY at DESC, machine) AS held FROM live_samples) WHERE held > ?1 ORDER BY at DESC LIMIT 1",
          params![most_bytes],
          |row| row.get(0),
        )
        .map(Some)
        .or_else(|why| if why == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(why) })?;

      if let Some(cut) = cut {
        record.execute("DELETE FROM live_samples WHERE at <= ?1", params![cut])?;
      }

      Ok(())
    }
  }
}

/// The record's writer: every write handed to its thread, in the order handed, so a commit waiting on a
/// busy disk never holds what hands it over.
#[derive(Clone)]
pub struct Writer {
  writes: mpsc::Sender<Write>,
}

impl Writer {
  /// The record made on the spot where it is missing, so a reader may open it at once, and its writing
  /// thread started over it.
  pub fn open(path: &Path) -> Result<Self, String> {
    let mut record = open(path).map_err(|why| format!("{}: {why}", path.display()))?;
    let (writes, written) = mpsc::channel();

    std::thread::Builder::new()
      .name("hardware-live-record".into())
      .spawn(move || {
        for one in written {
          let kind = match &one {
            Write::Reading { .. } => "reading",
            Write::Failure { .. } => "failure",
            Write::Memory { .. } => "memory",
            Write::Prune { .. } => "prune",
            Write::Written(_) => "written",
          };

          if let Err(why) = write(&mut record, one) {
            eprintln!("bmo-node: hardware live record: {kind}: {why}");
          }
        }
      })
      .map_err(|why| why.to_string())?;

    Ok(Self { writes })
  }

  pub fn reading(&self, machine: &str, at: i64, view: &HardwareView) {
    let (Ok(text), Ok(sample)) =
      (serde_json::to_string(view), serde_json::to_string(&history_sample(view)))
    else {
      return;
    };

    let _ = self.writes.send(Write::Reading {
      machine: machine.to_owned(),
      at,
      view: text,
      sample,
    });
  }

  pub fn failure(&self, machine: &str, at: i64, detail: &str) {
    let _ = self.writes.send(Write::Failure {
      machine: machine.to_owned(),
      at,
      detail: detail.to_owned(),
    });
  }

  pub fn memory(&self, machine: &str, at: i64, sample: &MemorySample) {
    let Ok(sample) = serde_json::to_string(sample) else {
      return;
    };

    let _ = self.writes.send(Write::Memory {
      machine: machine.to_owned(),
      at,
      sample,
    });
  }

  /// Rows older than the reach removed, then the oldest beyond the most the record may hold.
  pub fn prune(&self, now: i64, keep_seconds: i64) {
    self.prune_within(now, keep_seconds, MOST_BYTES);
  }

  /// Rows older than the reach removed, then the oldest beyond `most_bytes` of the rows' text.
  pub fn prune_within(&self, now: i64, keep_seconds: i64, most_bytes: i64) {
    let _ = self.writes.send(Write::Prune {
      now,
      keep_seconds,
      most_bytes,
    });
  }

  /// Once everything handed over before this is written; at once where the thread is gone.
  pub fn written(&self) {
    let (answer, answered) = mpsc::channel();

    if self.writes.send(Write::Written(answer)).is_ok() {
      let _ = answered.recv();
    }
  }
}

/// A machine's latest as the record keeps it: the second it came, and its reading in full or the
/// failure its run met.
pub struct Latest {
  pub at: i64,
  pub view: Option<String>,
  pub failure: Option<String>,
}

/// One reading's figures as the record keeps them.
pub struct Sample {
  pub at: i64,
  pub sample: Map<String, Value>,
}

/// The record as a reader opens it; none where no writer has made it yet.
pub struct Reader(Connection);

impl Reader {
  pub fn open(path: &Path) -> Option<Self> {
    let record =
      Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .ok()?;

    store::wait_for_locks(&record).ok()?;

    let made: bool = record
      .query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'live_samples'", [], |_| {
        Ok(true)
      })
      .unwrap_or(false);

    made.then_some(Self(record))
  }

  /// The machines the record holds figures of.
  pub fn machines(&self) -> rusqlite::Result<Vec<String>> {
    let mut asked = self.0.prepare("SELECT DISTINCT machine FROM live_samples")?;

    asked.query_map([], |row| row.get(0))?.collect()
  }

  /// One machine's figures between two seconds, in their order.
  pub fn samples(&self, machine: &str, from: i64, to: i64) -> rusqlite::Result<Vec<Sample>> {
    let mut asked = self.0.prepare(
      "SELECT at, sample FROM live_samples WHERE machine = ?1 AND at >= ?2 AND at < ?3 ORDER BY at",
    )?;
    asked
      .query_map(params![machine, from, to], |row| {
        let text: String = row.get(1)?;

        Ok(Sample {
          at: row.get(0)?,
          sample: serde_json::from_str(&text).unwrap_or_default(),
        })
      })?
      .collect()
  }

  /// One machine's memory samples between two seconds, in their order, each as its JSON text.
  pub fn memory_samples(&self, machine: &str, from: i64, to: i64) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut asked = self.0.prepare(
      "SELECT at, sample FROM live_memory WHERE machine = ?1 AND at >= ?2 AND at < ?3 ORDER BY at",
    )?;

    asked
      .query_map(params![machine, from, to], |row| Ok((row.get(0)?, row.get(1)?)))?
      .collect()
  }

  /// The latest reading of a machine as the record holds it.
  pub fn latest(&self, machine: &str) -> rusqlite::Result<Option<Latest>> {
    self
      .0
      .query_row(
        "SELECT at, view, failure FROM live_latest WHERE machine = ?1",
        params![machine],
        |row| {
          Ok(Latest {
            at: row.get(0)?,
            view: row.get(1)?,
            failure: row.get(2)?,
          })
        },
      )
      .map(Some)
      .or_else(|why| if why == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(why) })
  }
}
