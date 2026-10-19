//! The hardware routes (docs/api.md, 「Hardware」; docs/hardware.md): a machine's latest reading, its live
//! record's readings, one span of its history, and its tracking switch and CPU limit.

use axum::Json;
use axum::body::Bytes;
use axum::extract::Path;
use axum::http::Uri;
use bmo_node_client::ErrorCode;
use bmo_node_client::hardware::{
  CpuLimit, CpuLimitSet, HardwareView, HistoryQuery, HistoryRange, LatestReading, LiveQuery, LiveRange,
  MachineKind, Tracking, TrackingSet,
};
use hardware::once::{ONCE_WAIT, read_once};
use hardware::probe::plausible_celsius;
use hardware::record::{KEEP_SECONDS, Reader};
use hardware::roads::Source;
use hardware::series::{HISTORY_FIRST_SPAN, MS_PER_SECOND, read_span, span};
use hardware::view::hardware_view;
use hardware::{LOCAL_MACHINE, follow_tracking, source_of};
use policy::nodes::NodeKind;

use super::{Shared, now_ms, now_seconds};
use crate::Service;
use crate::reply::{Answer, Reply, body, query};

/// How fresh the recorder's latest must be for a read to answer it rather than read the machine itself.
const FRESH_SECONDS: i64 = 10;

/// A machine the monitor reads: its name, how it is shown, its kind and its road, none where its row lacks
/// a field its kind needs.
struct Machine {
  name: String,
  label: String,
  kind: MachineKind,
  source: Option<Source>,
}

/// This machine, or a listed node; any other name is refused rather than kept as a setting nothing reads.
fn machine(service: &Service, name: &str) -> Result<Machine, Reply> {
  if name == LOCAL_MACHINE {
    return Ok(Machine {
      name: LOCAL_MACHINE.into(),
      label: service.label.clone(),
      kind: MachineKind::Local,
      source: Some(Source::Local),
    });
  }

  let node = service
    .store
    .node(name)?
    .ok_or_else(|| Reply::new(ErrorCode::NoNode, format!("no machine named {name}")))?;

  Ok(Machine {
    source: source_of(&node),
    kind: match node.kind {
      NodeKind::Ssh => MachineKind::Ssh,
      NodeKind::Jake => MachineKind::Jake,
    },
    label: node.label,
    name: node.name,
  })
}

fn reader(service: &Service) -> Option<Reader> {
  Reader::open(&service.record)
}

pub async fn latest(service: Shared, Path(name): Path<String>) -> Answer<LatestReading> {
  let machine = machine(&service, &name)?;
  let tracking = service.store.tracking(&machine.name)?;
  let cpu_limit = service.store.cpu_limit(&machine.name)?;
  let answer = |hardware: Option<HardwareView>, failure: Option<String>, read_at: i64| {
    Json(LatestReading {
      name: machine.name.clone(),
      label: machine.label.clone(),
      kind: machine.kind,
      hardware,
      failure,
      read_at,
      tracking,
      cpu_limit,
    })
  };
  let now = now_seconds();

  if !tracking {
    return Ok(answer(None, None, now_ms()));
  }

  let latest = reader(&service).and_then(|reader| reader.latest(&machine.name).ok().flatten());

  if let Some(latest) = latest.filter(|latest| now - latest.at <= FRESH_SECONDS) {
    let view = latest.view.as_deref().and_then(|view| serde_json::from_str::<HardwareView>(view).ok());
    let read_at = latest.at * MS_PER_SECOND;

    return Ok(match view {
      Some(view) => answer(Some(view), None, read_at),
      None => answer(
        None,
        Some(latest.failure.unwrap_or_else(|| "the machine could not be read".into())),
        read_at,
      ),
    });
  }

  let Some(source) = machine.source.clone() else {
    return Ok(answer(None, Some("the row is missing a field its kind needs".into()), now_ms()));
  };
  let cell = service.once_for(&machine.name, now_ms());
  let road = service.road.clone();
  let (read_at, read) = cell
    .get_or_init(|| async move {
      let read = read_once(&road, source, ONCE_WAIT).await.map(|snapshot| hardware_view(&snapshot));

      (now_ms(), read)
    })
    .await
    .clone();

  Ok(match read {
    Ok(view) => answer(Some(view), None, read_at),
    Err(failure) => answer(None, Some(failure), read_at),
  })
}

pub async fn live(service: Shared, Path(name): Path<String>, uri: Uri) -> Answer<LiveRange> {
  let asked: LiveQuery = query(&uri)?;
  let machine = machine(&service, &name)?;
  let reach = now_seconds() - KEEP_SECONDS;
  let since = asked.after.map_or(reach, |after| reach.max(after.div_euclid(MS_PER_SECOND)));
  let metrics: Vec<String> = asked
    .metrics
    .map(|metrics| metrics.split(',').filter(|one| !one.is_empty()).map(str::to_owned).collect())
    .unwrap_or_default();
  let range = match reader(&service) {
    Some(reader) => reader
      .range(&machine.name, since, &metrics)
      .map_err(|why| Reply::failed(why.to_string()))?,
    None => LiveRange::default(),
  };
  let Some(limit) = asked.limit.map(|limit| limit as usize) else {
    return Ok(Json(range));
  };
  let taken = range.at.len().min(limit);
  // The first after a cursor, or the last where none is given.
  let skip = if asked.after.is_some() { 0 } else { range.at.len() - taken };
  let cut = |column: Vec<Option<f64>>| column.into_iter().skip(skip).take(taken).collect();

  Ok(Json(LiveRange {
    at: range.at.into_iter().skip(skip).take(taken).collect(),
    series: range.series.into_iter().map(|(key, column)| (key, cut(column))).collect(),
  }))
}

pub async fn history(service: Shared, Path(name): Path<String>, uri: Uri) -> Answer<HistoryRange> {
  let asked: HistoryQuery = query(&uri)?;
  let machine = machine(&service, &name)?;
  let asked = asked.span.as_deref().unwrap_or(HISTORY_FIRST_SPAN);
  let (seconds, point) =
    span(asked).ok_or_else(|| Reply::bad_request(format!("{asked} is not a span the monitor draws")))?;
  let range =
    read_span(&service.store, reader(&service).as_ref(), &machine.name, seconds, point, now_seconds())
      .map_err(|why| Reply::failed(why.to_string()))?;

  Ok(Json(range))
}

pub async fn tracking(service: Shared, Path(name): Path<String>, bytes: Bytes) -> Answer<Tracking> {
  let asked: TrackingSet = body(&bytes)?;
  let machine = machine(&service, &name)?;

  service.store.set_tracking(&machine.name, asked.on)?;
  follow_tracking();

  Ok(Json(Tracking {
    on: service.store.tracking(&machine.name)?,
    machine: machine.name,
  }))
}

pub async fn cpu_limit(service: Shared, Path(name): Path<String>, bytes: Bytes) -> Answer<CpuLimit> {
  let asked: CpuLimitSet = body(&bytes)?;

  if asked.celsius.is_some_and(|one| !plausible_celsius(one)) {
    return Err(Reply::bad_request("a limit is above 0 °C and below 127 °C"));
  }

  let machine = machine(&service, &name)?;

  service
    .store
    .set_cpu_limit(&machine.name, asked.celsius.map(hardware::js::shown).as_deref())?;

  Ok(Json(CpuLimit {
    celsius: service.store.cpu_limit(&machine.name)?,
    machine: machine.name,
  }))
}
