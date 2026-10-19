//! The service's own machine (docs/api.md, 「This machine」): its room for another launch, read now, and
//! its standing as the minute watch last read it.

use axum::Json;
use bmo_node_client::machine::{Room, Standing};

use super::{Shared, now_ms};
use crate::reply::{Answer, Reply};

pub async fn room(service: Shared) -> Answer<Room> {
  let standing = hardware::machine::here(&service.store, &service.proc_root, &service.data_root, now_ms())?;

  Ok(Json(Room { held: standing.held }))
}

pub async fn standing(service: Shared) -> Answer<Standing> {
  let kept = hardware::machine::kept(&service.store).map_err(Reply::from)?;

  Ok(Json(kept.unwrap_or_else(Standing::unread)))
}
