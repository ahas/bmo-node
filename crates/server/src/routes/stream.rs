//! The machine's standing, the event stream and the health route (docs/api.md, 「The machine's standing」,
//! 「Events」 and 「Health」).

use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::Query;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;

use super::Shared;
use crate::reply::{Answer, Reply};

pub async fn standing(service: Shared) -> Answer {
  let kept = hardware::machine::kept(&service.store).map_err(Reply::from)?;

  Ok(Json(kept.unwrap_or_else(
    || json!({ "reading": null, "held": [], "browsers": 0, "places": [], "largest": [] }),
  )))
}

pub async fn events(service: Shared, Query(query): Query<HashMap<String, String>>) -> impl IntoResponse {
  let kinds: Vec<String> = query
    .get("kinds")
    .map(|kinds| kinds.split(',').filter(|one| !one.is_empty()).map(str::to_owned).collect())
    .unwrap_or_default();
  let receiver = service.events.subscribe();
  let events = stream::unfold((receiver, kinds), |(mut receiver, kinds)| async move {
    loop {
      match receiver.recv().await {
        Ok((kind, data)) if kinds.is_empty() || kinds.contains(&kind) => {
          let event = Event::default().event(kind).data(data.to_string());

          return Some((Ok::<_, Infallible>(event), (receiver, kinds)));
        }
        Ok(_) | Err(RecvError::Lagged(_)) => continue,
        Err(RecvError::Closed) => return None,
      }
    }
  });

  Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn health(service: Shared) -> Answer {
  Ok(Json(json!({
    "pid": std::process::id(),
    "version": env!("CARGO_PKG_VERSION"),
    "ready": service.ready(),
  })))
}
