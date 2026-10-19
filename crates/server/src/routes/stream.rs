//! The event stream and the health route (docs/api.md, 「Events」 and 「Health」).

use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::http::Uri;
use axum::response::sse::{Event as Sent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use bmo_node_client::events::EventsQuery;
use bmo_node_client::machine::Health;
use futures_util::stream;
use tokio::sync::broadcast::error::RecvError;

use super::Shared;
use crate::reply::{Answer, query};

pub async fn events(service: Shared, uri: Uri) -> Response {
  let asked: EventsQuery = match query(&uri) {
    Ok(asked) => asked,
    Err(reply) => return reply.into_response(),
  };
  let kinds: Vec<String> = asked
    .kinds
    .map(|kinds| kinds.split(',').filter(|one| !one.is_empty()).map(str::to_owned).collect())
    .unwrap_or_default();
  let receiver = service.events.subscribe();
  let events = stream::unfold((receiver, kinds), |(mut receiver, kinds)| async move {
    loop {
      match receiver.recv().await {
        Ok(event) if kinds.is_empty() || kinds.iter().any(|kind| kind == event.kind().name()) => {
          let sent = Sent::default().event(event.kind().name()).data(event.data().to_string());

          return Some((Ok::<_, Infallible>(sent), (receiver, kinds)));
        }
        Ok(_) | Err(RecvError::Lagged(_)) => continue,
        Err(RecvError::Closed) => return None,
      }
    }
  });

  Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))).into_response()
}

pub async fn health(service: Shared) -> Answer<Health> {
  Ok(Json(Health {
    pid: std::process::id(),
    version: env!("CARGO_PKG_VERSION").to_owned(),
    ready: service.ready(),
  }))
}

