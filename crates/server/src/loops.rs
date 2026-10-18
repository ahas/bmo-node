//! The service's loops: the node check pass once a minute, the hardware recorder's live runs and its minute
//! pass, and this machine's minute look (docs/nodes.md, docs/hardware.md, docs/machine.md).

use std::sync::Arc;
use std::time::Duration;

use hardware::Recorder;
use hardware::live::Hands;
use node_check::pass::check_nodes;
use serde_json::json;
use tokio::task::JoinHandle;

use crate::Service;
use crate::events::NODES_CHANGED;

/// How often each loop runs.
const MINUTE: Duration = Duration::from_secs(60);

/// Every node checked, and `nodes-changed` published where a state moved or the list's shape moved since
/// the pass before: a node added, removed, renamed, relabelled or moved, by a route, the command or a hub.
pub async fn node_pass(service: &Service) -> Result<bool, store::StoreError> {
  let checked = check_nodes(&*service.store, &service.machine, None).await?;
  let shape: Vec<(String, String, i64)> = service
    .store
    .nodes()?
    .into_iter()
    .map(|node| (node.name, node.label, node.position))
    .collect();
  let moved = service.shape_moved(shape);
  let changed = checked.changed || moved;

  if changed {
    service.events.publish(NODES_CHANGED, json!({}));
  }

  Ok(changed)
}

/// One look at this machine: read, judged, kept and its pressures told.
pub fn machine_look(service: &Service) -> Result<(), store::StoreError> {
  let now = hardware::now_ms();
  let standing = hardware::machine::here(&service.store, &service.proc_root, &service.data_root, now)?;

  hardware::machine::look(&service.store, &standing, now, &service.events)
}

/// The loops started, each on a task of its own, and the service marked ready.
pub fn start(service: Arc<Service>) -> Result<Vec<JoinHandle<()>>, String> {
  let hands = Hands {
    stream: service.road.clone(),
    now_ms: Arc::new(hardware::now_ms),
  };
  let recorder = Arc::new(Recorder::with_hands(service.store.clone(), &service.record, hands)?);
  let mut tasks = Vec::new();

  recorder.follow();

  let checking = service.clone();

  tasks.push(tokio::spawn(async move {
    loop {
      if let Err(why) = node_pass(&checking).await {
        eprintln!("bmo-node: the node pass failed: {why}");
      }

      tokio::time::sleep(MINUTE).await;
    }
  }));

  tasks.push(tokio::spawn(async move {
    loop {
      if let Err(why) = recorder.pass(store::now_seconds()) {
        eprintln!("bmo-node: the hardware pass failed: {why}");
      }

      tokio::time::sleep(MINUTE).await;
    }
  }));

  let looking = service.clone();

  tasks.push(tokio::spawn(async move {
    loop {
      if let Err(why) = machine_look(&looking) {
        eprintln!("bmo-node: the machine look failed: {why}");
      }

      tokio::time::sleep(MINUTE).await;
    }
  }));

  service.set_ready();

  Ok(tasks)
}
