//! The node service's own store (docs/store.md): one SQLite file in the data root's `node/` folder holding
//! the node list, the hubs, the hardware history and the settings, and the path of the live record beside
//! it. No other unit reads these files.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use bmo_node_client::ErrorCode;
use policy::nodes::PersonalCallers;
use rusqlite::Connection;

pub mod history;
pub mod import;
mod migration;
pub mod nodes;
pub mod settings;

pub use migration::{BASELINE, HUB_SEED};

/// The folder under the data root everything of the node service's stands in.
pub const FOLDER: &str = "node";

/// The store's own file in that folder.
pub const STORE_FILE: &str = "node.db";

/// The live record's file in that folder (docs/hardware.md, 「Live」).
pub const RECORD_FILE: &str = "hardware-live.db";

/// The node service's folder under a data root.
pub fn folder(data_root: &Path) -> PathBuf {
  data_root.join(FOLDER)
}

/// The store's file under a data root.
pub fn store_path(data_root: &Path) -> PathBuf {
  folder(data_root).join(STORE_FILE)
}

/// The live record's file under a data root.
pub fn live_record_path(data_root: &Path) -> PathBuf {
  folder(data_root).join(RECORD_FILE)
}

/// A request the store refuses, with its code (docs/api.md, 「Wire format」) and an English sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
  pub code: ErrorCode,
  pub message: String,
}

impl Refusal {
  pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
    Self {
      code,
      message: message.into(),
    }
  }
}

/// Why a call to the store did not do what it was asked.
#[derive(Debug)]
pub enum StoreError {
  /// The request is one the store does not take, said as a refusal.
  Refused(Refusal),
  /// The database failed.
  Database(rusqlite::Error),
  /// The store's folder could not be made.
  Folder(std::io::Error),
}

impl StoreError {
  pub fn refused(code: ErrorCode, message: impl Into<String>) -> Self {
    Self::Refused(Refusal::new(code, message))
  }

  /// The refusal this error is, where it is one.
  pub fn refusal(&self) -> Option<&Refusal> {
    match self {
      Self::Refused(refusal) => Some(refusal),
      _ => None,
    }
  }
}

impl fmt::Display for StoreError {
  fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Refused(refusal) => write!(out, "{} ({})", refusal.message, refusal.code),
      Self::Database(error) => write!(out, "the node store failed: {error}"),
      Self::Folder(error) => write!(out, "the node store's folder could not be made: {error}"),
    }
  }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
  fn from(error: rusqlite::Error) -> Self {
    Self::Database(error)
  }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// The store: one connection, used by one caller at a time.
pub struct Store {
  connection: Mutex<Connection>,
  /// The callers a personal hub node is written with as it joins the list.
  personal_callers: PersonalCallers,
}

/// A connection that meets another's lock waits until the lock is given back, however long it is held,
/// looking again a little later each time, up to a tenth of a second apart (docs/store.md).
pub fn wait_for_locks(connection: &Connection) -> rusqlite::Result<()> {
  connection.busy_handler(Some(look_again))
}

fn look_again(tries: i32) -> bool {
  std::thread::sleep(std::time::Duration::from_millis(tries.clamp(0, 99) as u64 + 1));
  true
}

impl Store {
  /// The store at its file under the data root, its folder made and its migrations run.
  pub fn open_in(data_root: &Path) -> Result<Self> {
    std::fs::create_dir_all(folder(data_root)).map_err(StoreError::Folder)?;
    Self::open(&store_path(data_root))
  }

  /// The store at a file, its migrations run.
  pub fn open(path: &Path) -> Result<Self> {
    let connection = Connection::open(path)?;

    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    wait_for_locks(&connection)?;
    Self::settled(connection)
  }

  /// A store held in memory alone, its migrations run.
  pub fn in_memory() -> Result<Self> {
    Self::settled(Connection::open_in_memory()?)
  }

  fn settled(mut connection: Connection) -> Result<Self> {
    migration::settle(&mut connection, now_seconds())?;

    Ok(Self {
      connection: Mutex::new(connection),
      personal_callers: PersonalCallers::default(),
    })
  }

  /// The store writing every personal hub node that joins the list with the callers given
  /// (docs/nodes.md, 「A node's callers」).
  pub fn with_personal_callers(mut self, callers: PersonalCallers) -> Self {
    self.personal_callers = callers;
    self
  }

  /// The connection, held for one caller.
  pub(crate) fn connection(&self) -> MutexGuard<'_, Connection> {
    self.connection.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
  }
}

/// Now, in seconds since the epoch.
pub fn now_seconds() -> i64 {
  SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}
