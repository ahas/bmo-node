//! The hubs (docs/api.md, 「Hubs」): the list, a hub listed, and a hub removed with every node reached
//! through it, each node's removal published as its own event.

use axum::Json;
use axum::body::Bytes;
use axum::extract::Path;
use axum::http::StatusCode;
use bmo_node_client::nodes::{Hub, HubPage, NewHub};
use store::nodes::Hub as Kept;

use super::{Shared, settle};
use crate::reply::{Answer, Done, Made, Reply, body};
use crate::views::MS_PER_SECOND;

/// A hub as every route answers it.
pub fn view(hub: Kept) -> Hub {
  Hub {
    id: hub.id,
    address: hub.url,
    token_file: hub.token_file,
    label: hub.label,
    created_at: hub.created_at * MS_PER_SECOND,
  }
}

pub async fn list(service: Shared) -> Answer<HubPage> {
  Ok(Json(HubPage::whole(service.store.hubs()?.into_iter().map(view).collect())))
}

pub async fn create(service: Shared, bytes: Bytes) -> Made<Hub> {
  let asked: NewHub = body(&bytes)?;
  let hub = service.store.add_hub(&asked.address, &asked.token_file, asked.label.as_deref())?;

  Ok((StatusCode::CREATED, Json(view(hub))))
}

pub async fn remove(service: Shared, Path(id): Path<String>) -> Done {
  let no_hub = || Reply::not_found(format!("no hub has the id {id}"));
  let key = id.parse::<i64>().map_err(|_| no_hub())?;

  if service.store.remove_hub(key)?.is_none() {
    return Err(no_hub());
  }

  settle(&service)?;
  Ok(StatusCode::NO_CONTENT)
}
