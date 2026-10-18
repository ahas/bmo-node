//! The hardware routes (docs/api.md, 「Hardware」; docs/hardware.md): a machine's latest reading, its live
//! record's readings, one span of its history, and its tracking switch and CPU limit.

use std::collections::HashMap;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query};
use hardware::once::{ONCE_WAIT, read_once};
use hardware::probe::plausible_celsius;
use hardware::record::{KEEP_SECONDS, Reader};
use hardware::roads::Source;
use hardware::series::{HISTORY_FIRST_SPAN, LiveRange, read_span, span};
use hardware::view::hardware_view;
use hardware::{LOCAL_MACHINE, follow_tracking, source_of};
use serde_json::{Value, json};

use super::{Shared, now_ms, now_seconds};
use crate::Service;
use crate::reply::{Answer, Reply, body};

/// How fresh the recorder's latest must be for a read to answer it rather than read the machine itself.
const FRESH_SECONDS: i64 = 10;

/// A machine the monitor reads: its name, how it is shown, its kind and its road, none where its row lacks
/// a field its kind needs.
struct Machine {
  name: String,
  label: String,
  kind: &'static str,
  source: Option<Source>,
}

/// This machine, or a listed node; any other name is refused rather than kept as a setting nothing reads.
fn machine(service: &Service, name: &str) -> Result<Machine, Reply> {
  if name == LOCAL_MACHINE {
    return Ok(Machine {
      name: LOCAL_MACHINE.into(),
      label: service.label.clone(),
      kind: "local",
      source: Some(Source::Local),
    });
  }

  let node = service
    .store
    .node(name)?
    .ok_or_else(|| Reply::refused("no-node", format!("no machine named {name}")))?;

  Ok(Machine {
    source: source_of(&node),
    kind: node.kind.name(),
    label: node.label,
    name: node.name,
  })
}

fn reader(service: &Service) -> Option<Reader> {
  Reader::open(&service.record)
}

pub async fn latest(service: Shared, Path(name): Path<String>) -> Answer {
  let machine = machine(&service, &name)?;
  let tracking = service.store.tracking(&machine.name)?;
  let cpu_limit = service.store.cpu_limit(&machine.name)?;
  let answer = |hardware: Value, failure: Option<String>, read_at: i64| {
    Json(json!({
      "name": machine.name,
      "label": machine.label,
      "kind": machine.kind,
      "hardware": hardware,
      "failure": failure,
      "readAt": read_at,
      "tracking": tracking,
      "cpuLimit": cpu_limit,
    }))
  };
  let now = now_seconds();

  if !tracking {
    return Ok(answer(Value::Null, None, now));
  }

  let latest = reader(&service).and_then(|reader| reader.latest(&machine.name).ok().flatten());

  if let Some(latest) = latest.filter(|latest| now - latest.at <= FRESH_SECONDS) {
    let view = latest.view.as_deref().and_then(|view| serde_json::from_str::<Value>(view).ok());

    return Ok(match view {
      Some(view) => answer(view, None, latest.at),
      None => answer(
        Value::Null,
        Some(latest.failure.unwrap_or_else(|| "the machine could not be read".into())),
        latest.at,
      ),
    });
  }

  let Some(source) = machine.source.clone() else {
    return Ok(answer(Value::Null, Some("the row is missing a field its kind needs".into()), now));
  };
  let cell = service.once_for(&machine.name, now_ms());
  let road = service.road.clone();
  let (read_at, read) = cell
    .get_or_init(|| async move {
      let read = read_once(&road, source, ONCE_WAIT)
        .await
        .map(|snapshot| json!(hardware_view(&snapshot)));

      (now_seconds(), read)
    })
    .await
    .clone();

  Ok(match read {
    Ok(view) => answer(view, None, read_at),
    Err(failure) => answer(Value::Null, Some(failure), read_at),
  })
}

pub async fn live(
  service: Shared,
  Path(name): Path<String>,
  Query(query): Query<HashMap<String, String>>,
) -> Answer {
  let machine = machine(&service, &name)?;
  let reach = now_seconds() - KEEP_SECONDS;
  let since = query
    .get("since")
    .and_then(|since| since.parse::<f64>().ok())
    .filter(|since| since.is_finite());
  let since = since.map_or(reach, |since| reach.max(since.floor() as i64));
  let metrics: Vec<String> = query
    .get("metrics")
    .map(|metrics| metrics.split(',').filter(|one| !one.is_empty()).map(str::to_owned).collect())
    .unwrap_or_default();
  let range = match reader(&service) {
    Some(reader) => reader
      .range(&machine.name, since, &metrics)
      .map_err(|why| Reply::failed(why.to_string()))?,
    None => LiveRange::default(),
  };

  Ok(Json(json!(range)))
}

pub async fn history(
  service: Shared,
  Path(name): Path<String>,
  Query(query): Query<HashMap<String, String>>,
) -> Answer {
  let machine = machine(&service, &name)?;
  let asked = query.get("span").map_or(HISTORY_FIRST_SPAN, String::as_str);
  let (seconds, point) =
    span(asked).ok_or_else(|| Reply::bad_request(format!("{asked} is not a span the monitor draws")))?;
  let range =
    read_span(&service.store, reader(&service).as_ref(), &machine.name, seconds, point, now_seconds())
      .map_err(|why| Reply::failed(why.to_string()))?;

  Ok(Json(json!(range)))
}

pub async fn tracking(service: Shared, bytes: Bytes) -> Answer {
  let asked: Value = body(&bytes)?;
  let (Some(name), Some(on)) = (asked["machine"].as_str(), asked["on"].as_bool()) else {
    return Err(Reply::bad_request("machine (string) and on (boolean) are required"));
  };
  let machine = machine(&service, name)?;

  service.store.set_tracking(&machine.name, on)?;
  follow_tracking();

  Ok(Json(json!({ "machine": machine.name, "on": service.store.tracking(&machine.name)? })))
}

pub async fn cpu_limit(service: Shared, bytes: Bytes) -> Answer {
  let asked: Value = body(&bytes)?;
  let celsius = &asked["celsius"];
  let (Some(name), true) = (asked["machine"].as_str(), celsius.is_null() || celsius.is_number()) else {
    return Err(Reply::bad_request("machine (string) and celsius (number or null) are required"));
  };
  let celsius = celsius.as_f64();

  if celsius.is_some_and(|one| !plausible_celsius(one)) {
    return Err(Reply::bad_request("a limit is above 0 °C and below 127 °C"));
  }

  let machine = machine(&service, name)?;

  service
    .store
    .set_cpu_limit(&machine.name, celsius.map(hardware::js::shown).as_deref())?;

  Ok(Json(
    json!({ "machine": machine.name, "celsius": service.store.cpu_limit(&machine.name)? }),
  ))
}
