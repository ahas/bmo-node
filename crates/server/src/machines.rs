//! A machine as every route and event answers it (docs/api.md, 「Machines」): this machine or a listed node,
//! with its tracking, its CPU limit, its latest reading and, for this machine, its standing.

use bmo_node_client::events::Change;
use bmo_node_client::hardware::HardwareView;
use bmo_node_client::machines::{Machine, MachineKind, Standing};
use hardware::once::read_once;
use hardware::record::Reader;
use hardware::roads::Source;
use hardware::series::MS_PER_SECOND;
use hardware::view::hardware_view;
use hardware::{LOCAL_MACHINE, source_of};
use policy::nodes::NodeKind;

use crate::Service;
use crate::reply::Reply;

/// How fresh the recorder's latest must be for a read to answer it rather than read the machine itself.
const FRESH_SECONDS: i64 = 10;

/// A machine the monitor reads: its name, how it is shown, its kind and its road, none where its row lacks
/// a field its kind needs.
pub struct Known {
  pub name: String,
  pub label: String,
  pub kind: MachineKind,
  pub source: Option<Source>,
}

fn here(service: &Service) -> Known {
  Known {
    name: LOCAL_MACHINE.into(),
    label: service.label.clone(),
    kind: MachineKind::Local,
    source: Some(Source::Local),
  }
}

fn of_node(node: store::nodes::Node) -> Known {
  Known {
    source: source_of(&node),
    kind: match node.kind {
      NodeKind::Ssh => MachineKind::Ssh,
      NodeKind::Hub => MachineKind::Hub,
    },
    label: node.label,
    name: node.name,
  }
}

/// This machine, or a listed shared node; any other name, a personal node's among them, is refused rather
/// than kept as a setting nothing reads (docs/hardware.md, 「Which machines」).
pub fn known(service: &Service, name: &str) -> Result<Known, Reply> {
  if name == LOCAL_MACHINE {
    return Ok(here(service));
  }

  let node = service
    .store
    .node(name)?
    .filter(store::nodes::Node::monitored)
    .ok_or_else(|| Reply::not_found(format!("no machine named {name}")))?;

  Ok(of_node(node))
}

/// Every machine the monitor knows: this machine, then every shared node in the list's order.
pub fn every(service: &Service) -> Result<Vec<Known>, Reply> {
  let mut machines = vec![here(service)];

  machines.extend(service.store.nodes()?.into_iter().filter(store::nodes::Node::monitored).map(of_node));

  Ok(machines)
}

/// How a machine's reading is had where the recorder holds nothing fresh of it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Read {
  /// The machine read itself with the probe run once.
  Now,
  /// The recorder's latest as it stands, however old.
  Recorded,
}

/// A machine as the routes answer it.
pub async fn machine(service: &Service, known: Known, read: Read) -> Result<Machine, Reply> {
  let tracking = service.store.tracking(&known.name)?;
  let standing = if known.kind == MachineKind::Local {
    Some(hardware::machine::kept(&service.store)?.unwrap_or_else(Standing::unread))
  } else {
    None
  };
  let mut machine = Machine {
    tracking,
    cpu_limit: service.store.cpu_limit(&known.name)?,
    read_at: None,
    hardware: None,
    failure: None,
    standing,
    kind: known.kind,
    label: known.label.clone(),
    name: known.name.clone(),
  };

  if !tracking {
    return Ok(machine);
  }

  let latest = Reader::open(&service.record).and_then(|reader| reader.latest(&known.name).ok().flatten());
  let fresh = |at: i64| store::now_seconds() - at <= FRESH_SECONDS;

  if let Some(latest) = latest.filter(|latest| read == Read::Recorded || fresh(latest.at)) {
    let view = latest.view.as_deref().and_then(|view| serde_json::from_str::<HardwareView>(view).ok());

    machine.read_at = Some(latest.at * MS_PER_SECOND);
    match view {
      Some(view) => machine.hardware = Some(view),
      None => {
        machine.failure = Some(latest.failure.unwrap_or_else(|| "the machine could not be read".into()));
      }
    }

    return Ok(machine);
  }

  if read == Read::Recorded {
    return Ok(machine);
  }

  let Some(source) = known.source else {
    machine.read_at = Some(hardware::now_ms());
    machine.failure = Some("the row is missing a field its kind needs".into());

    return Ok(machine);
  };
  let cell = service.once_for(&known.name, hardware::now_ms());
  let road = service.road.clone();
  let (read_at, read) = cell
    .get_or_init(|| async move {
      let read = read_once(&road, source).await.map(|snapshot| hardware_view(&snapshot));

      (hardware::now_ms(), read)
    })
    .await
    .clone();

  machine.read_at = Some(read_at);
  match read {
    Ok(view) => machine.hardware = Some(view),
    Err(failure) => machine.failure = Some(failure),
  }

  Ok(machine)
}

/// A machine's change published, the machine as the recorder holds it.
pub async fn publish_changed(service: &Service, name: &str) -> Result<(), Reply> {
  let changed = machine(service, known(service, name)?, Read::Recorded).await?;

  service.events.publish(Change::MachineChanged(Box::new(changed)));

  Ok(())
}
