//! The service's loops: the node check pass once a minute, the hardware recorder's live runs and its minute
//! pass, and this machine's minute look (docs/nodes.md, docs/hardware.md, docs/machine.md).

use std::sync::Arc;
use std::time::Duration;

use bmo_node_client::events::Change;
use bmo_node_client::hardware::HardwareView;
use hardware::live::Hands;
use hardware::series::live_reading;
use hardware::view::history_sample;
use hardware::{LOCAL_MACHINE, Listen, Recorder};
use node_check::pass::check_nodes;
use tokio::task::JoinHandle;

use crate::Service;
use crate::events::Events;
use crate::machines::publish_changed;

/// How often each loop runs.
const MINUTE: Duration = Duration::from_secs(60);

/// Every node checked, and whatever moved the list since its events last told it published: a check's
/// state, or a node added, changed or removed by a route, the command or a hub.
pub async fn node_pass(service: &Service) -> Result<(), store::StoreError> {
  check_nodes(&*service.store, &service.machine, None).await?;

  service.settle_nodes()
}

/// One look at this machine: read, its alerts told and kept, and `machine.changed` published where an
/// alert started or ended.
pub async fn machine_look(service: &Service) -> Result<(), String> {
  let now = hardware::now_ms();
  let standing =
    hardware::machine::here(&service.proc_root, &service.data_root, service.scope_variable.as_deref(), now);
  let looked = hardware::machine::look(&service.store, standing, now).map_err(|why| why.to_string())?;

  if looked.alerts_moved {
    publish_changed(service, LOCAL_MACHINE).await.map_err(|reply| reply.0.error)?;
  }

  Ok(())
}

/// Each live reading published as `reading.created` as the recorder takes it.
struct Readings(Events);

impl Listen for Readings {
  fn reading(&self, machine: &str, at: i64, view: &HardwareView) {
    self.0.publish(Change::ReadingCreated(live_reading(machine, at, &history_sample(view))));
  }
}

/// The loops started, each on a task of its own, and the service marked ready.
pub fn start(service: Arc<Service>) -> Result<Vec<JoinHandle<()>>, String> {
  let hands = Hands {
    stream: service.road.clone(),
    now_ms: Arc::new(hardware::now_ms),
  };
  let listen: Arc<dyn Listen> = Arc::new(Readings(service.events.clone()));
  let recorder = Arc::new(Recorder::listened(service.store.clone(), &service.record, hands, Some(listen))?);
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
      if let Err(why) = machine_look(&looking).await {
        eprintln!("bmo-node: the machine look failed: {why}");
      }

      tokio::time::sleep(MINUTE).await;
    }
  }));

  service.set_ready();

  Ok(tasks)
}
