//! The node service: its routes, its loops and the `bmo-node` command (docs/api.md).

pub mod cli;
pub mod config;
pub mod events;
mod loops;
pub mod reply;
mod routes;
pub mod this_machine;
pub mod views;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use hardware::live::{Hands, Road};
use serde_json::Value;
use store::Store;
use tokio::sync::OnceCell;

pub use loops::{machine_look, node_pass, start};

use crate::events::Events;
use crate::this_machine::ThisMachine;

/// A reading taken once, kept for one period and joined while it is under way.
type Once = Arc<OnceCell<(i64, Result<Value, String>)>>;

/// Everything a route and a loop stand on.
pub struct Service {
  pub store: Arc<Store>,
  pub token: String,
  /// The shared data root, whose disk the machine's room is read for.
  pub data_root: PathBuf,
  /// The live record's file.
  pub record: PathBuf,
  /// The proc root this machine is read from.
  pub proc_root: PathBuf,
  /// How this machine is shown.
  pub label: String,
  /// The OpenSSH, the home and the HTTP client a check runs with.
  pub machine: node_check::Machine,
  /// The road a machine is read over.
  pub road: Road,
  pub here: ThisMachine,
  pub events: Events,
  ready: AtomicBool,
  once: Mutex<HashMap<String, (i64, Once)>>,
  /// The list's shape at the last pass, which a pass holds the list to.
  shape: Mutex<Option<Vec<(String, String, i64)>>>,
}

impl Service {
  /// The service over its store, every other part this machine's own.
  pub fn new(
    store: Arc<Store>,
    token: impl Into<String>,
    data_root: PathBuf,
    label: Option<String>,
  ) -> Self {
    let here = ThisMachine::read();

    Self {
      record: store::live_record_path(&data_root),
      proc_root: hardware::machine::proc_root(),
      label: label
        .or_else(|| here.host_name.clone())
        .unwrap_or_else(|| hardware::LOCAL_MACHINE.to_owned()),
      machine: node_check::Machine::local(),
      road: Hands::default().stream,
      store,
      token: token.into(),
      data_root,
      here,
      events: Events::default(),
      ready: AtomicBool::new(false),
      once: Mutex::new(HashMap::new()),
      shape: Mutex::new(None),
    }
  }

  /// Whether the list's shape moved since the pass before, the first pass reading as no move; the shape
  /// kept for the next.
  pub(crate) fn shape_moved(&self, shape: Vec<(String, String, i64)>) -> bool {
    let mut held = self.shape.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let moved = held.as_ref().is_some_and(|before| *before != shape);

    *held = Some(shape);
    moved
  }

  /// Whether the store stands and the loops have started.
  pub fn ready(&self) -> bool {
    self.ready.load(Ordering::SeqCst)
  }

  pub fn set_ready(&self) {
    self.ready.store(true, Ordering::SeqCst);
  }

  /// The reading taken once of a machine under way or taken within the last second, or a new one.
  pub(crate) fn once_for(&self, machine: &str, now_ms: i64) -> Once {
    let mut held = self.once.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    match held.get(machine) {
      Some((at, cell)) if cell.get().is_none() || now_ms - at < 1_000 => cell.clone(),
      _ => {
        let cell: Once = Arc::default();

        held.insert(machine.to_owned(), (now_ms, cell.clone()));
        cell
      }
    }
  }
}

/// The service's routes over its parts.
pub fn app(service: Arc<Service>) -> Router {
  routes::router(service)
}
