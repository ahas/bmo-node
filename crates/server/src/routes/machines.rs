//! The machines (docs/api.md, 「Machines」; docs/hardware.md): every machine and one, its tracking and CPU
//! limit changed, and its past readings.

use axum::Json;
use axum::body::Bytes;
use axum::extract::Path;
use axum::http::Uri;
use bmo_node_client::hardware::{ReadingPage, ReadingsQuery};
use bmo_node_client::machines::{Machine, MachineChanges, MachinePage};
use hardware::follow_tracking;
use hardware::probe::plausible_celsius;
use hardware::record::Reader;
use hardware::series::{Asked, DEFAULT_SPAN_MS, ReadingsError, read_readings};

use super::Shared;
use crate::machines::{Read, every, known, machine, publish_changed};
use crate::reply::{Answer, Reply, body, query};

pub async fn list(service: Shared) -> Answer<MachinePage> {
  let mut items = Vec::new();

  for one in every(&service)? {
    items.push(machine(&service, one, Read::Recorded).await?);
  }

  Ok(Json(MachinePage::whole(items)))
}

pub async fn one(service: Shared, Path(name): Path<String>) -> Answer<Machine> {
  Ok(Json(machine(&service, known(&service, &name)?, Read::Now).await?))
}

pub async fn change(service: Shared, Path(name): Path<String>, bytes: Bytes) -> Answer<Machine> {
  let asked: MachineChanges = body(&bytes)?;

  if let Some(Some(limit)) = asked.cpu_limit
    && !plausible_celsius(limit)
  {
    return Err(Reply::bad_field("cpuLimit is above 0 °C and below 127 °C"));
  }

  let named = known(&service, &name)?;

  if let Some(on) = asked.tracking {
    service.store.set_tracking(&named.name, on)?;
    follow_tracking();
  }

  if let Some(limit) = asked.cpu_limit {
    service.store.set_cpu_limit(&named.name, limit.map(hardware::js::shown).as_deref())?;
  }

  if asked.tracking.is_some() || asked.cpu_limit.is_some() {
    publish_changed(&service, &named.name).await?;
  }

  Ok(Json(machine(&service, named, Read::Now).await?))
}

pub async fn readings(service: Shared, Path(name): Path<String>, uri: Uri) -> Answer<ReadingPage> {
  let asked: ReadingsQuery = query(&uri)?;
  let named = known(&service, &name)?;
  let asked = Asked {
    span_ms: asked.span_ms.unwrap_or(DEFAULT_SPAN_MS),
    after_ms: asked.after,
    before_ms: asked.before,
    limit: asked.limit.map(|limit| limit as usize),
  };
  let live = Reader::open(&service.record);
  let page = read_readings(&service.store, live.as_ref(), &named.name, asked, store::now_seconds())
    .map_err(|why| match why {
      ReadingsError::Span(_) => Reply::bad_request(why.to_string()),
      other => Reply::failed(other.to_string()),
    })?;

  Ok(Json(page))
}
