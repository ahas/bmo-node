//! The node service: its routes, its loops and the `bmo-node` command (docs/api.md).

pub mod cli;
pub mod config;
pub mod events;
mod loops;
pub mod machines;
pub mod owners;
pub mod reply;
mod routes;
pub mod stop;
pub mod this_machine;
pub mod views;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use axum::Router;
use bmo_node_client::events::Change;
use bmo_node_client::hardware::HardwareView;
use bmo_node_client::nodes::{Node as NodeView, NodeKey};
use hardware::live::{Hands, Road};
use store::Store;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

pub use loops::{machine_look, node_pass, start};

use crate::config::Caller;
use crate::events::Events;
use crate::owners::OwnersRead;
use crate::this_machine::ThisMachine;

/// A reading taken once, with when it was taken in milliseconds, kept for one period and joined while it
/// is under way.
type Once = Arc<OnceCell<(i64, Result<HardwareView, String>)>>;

/// Everything a route and a loop stand on.
pub struct Service {
  pub store: Arc<Store>,
  pub token: String,
  /// The shared data root, whose disk the machine look reads.
  pub data_root: PathBuf,
  /// The live record's file.
  pub record: PathBuf,
  /// The proc root this machine is read from.
  pub proc_root: PathBuf,
  /// How this machine is shown.
  pub label: String,
  /// The environment variable a process's scope is read from, where one is configured.
  pub scope_variable: Option<String>,
  /// The callers with a token of their own, which make a run as themselves.
  pub callers: Vec<Caller>,
  /// Where a node's owners are read for a run, where the configuration names it.
  pub owners: Option<OwnersRead>,
  /// The OpenSSH, the home and the HTTP client a check runs with.
  pub machine: node_check::Machine,
  /// The road a machine is read over.
  pub road: Road,
  pub here: ThisMachine,
  pub events: Events,
  /// Cancelled once the service is told to stop, which ends every event stream (docs/api.md, 「Stopping」).
  pub(crate) stopping: CancellationToken,
  ready: AtomicBool,
  once: Mutex<HashMap<String, (i64, Once)>>,
  /// Every node as its events last told it, which the next settling is told against.
  seen: Mutex<BTreeMap<String, NodeView>>,
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
    let seen = Mutex::new(node_views(&store, &here).unwrap_or_default());

    Self {
      record: store::live_record_path(&data_root),
      proc_root: hardware::machine::proc_root(),
      label: label
        .or_else(|| here.host_name.clone())
        .unwrap_or_else(|| hardware::LOCAL_MACHINE.to_owned()),
      scope_variable: None,
      callers: Vec::new(),
      owners: None,
      machine: node_check::Machine::local(),
      road: Hands::default().stream,
      store,
      token: token.into(),
      data_root,
      here,
      events: Events::default(),
      stopping: CancellationToken::new(),
      ready: AtomicBool::new(false),
      once: Mutex::new(HashMap::new()),
      seen,
    }
  }

  /// The service reading each process's scope from the variable named.
  pub fn with_scope_variable(mut self, variable: Option<String>) -> Self {
    self.scope_variable = variable.filter(|variable| !variable.is_empty());
    self
  }

  /// The service taking a run from the callers given, their owners read where `owners` names.
  pub fn with_runs(mut self, callers: Vec<Caller>, owners: Option<OwnersRead>) -> Self {
    self.callers = callers;
    self.owners = owners;
    self
  }

  /// The node list told against the one its events last told: each node added, moved or gone since
  /// published as its own event (docs/api.md, 「Events」).
  pub fn settle_nodes(&self) -> store::Result<()> {
    let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
    let now = node_views(&self.store, &self.here)?;

    for (name, view) in &now {
      match seen.get(name) {
        None => self.events.publish(Change::NodeCreated(view.clone())),
        Some(before) if views::moved(before, view) => {
          self.events.publish(Change::NodeChanged(view.clone()))
        }
        Some(_) => {}
      }
    }

    for name in seen.keys().filter(|name| !now.contains_key(*name)) {
      self.events.publish(Change::NodeDeleted(NodeKey { name: name.clone() }));
    }

    *seen = now;

    Ok(())
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
    let mut held = self.once.lock().unwrap_or_else(PoisonError::into_inner);

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

/// Every node as the routes answer it now, by its name.
fn node_views(store: &Store, here: &ThisMachine) -> store::Result<BTreeMap<String, NodeView>> {
  let now = store::now_seconds();

  Ok(
    store
      .nodes()?
      .iter()
      .map(|node| (node.name.clone(), views::view(node, here, now)))
      .collect(),
  )
}

/// The service's routes over its parts.
pub fn app(service: Arc<Service>) -> Router {
  routes::router(service)
}
