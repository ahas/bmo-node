//! The event stream and the health route (docs/api.md, 「Events」 and 「Health」).

use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::http::{HeaderMap, Uri, header};
use axum::response::sse::{Event as Sent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use bmo_node_client::ErrorCode;
use bmo_node_client::events::{EventKind, EventPage, EventsQuery};
use bmo_node_client::machines::Health;
use futures_util::stream::{self, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use super::Shared;
use crate::events::{Gone, Published};
use crate::reply::{Answer, Reply, query};

/// How often a comment line keeps a quiet stream open.
const KEEP_OPEN: Duration = Duration::from_secs(15);

fn gone(_: Gone) -> Reply {
  Reply::new(
    ErrorCode::Gone,
    "the stream resumes from before the window the service keeps; read the collections again",
  )
}

/// What a reader asked of the stream: where it resumes, which kinds and which machine.
struct Asked {
  after: Option<i64>,
  kinds: Vec<EventKind>,
  machine_name: Option<String>,
  limit: Option<usize>,
}

fn asked(uri: &Uri, headers: &HeaderMap) -> Result<Asked, Reply> {
  let asked: EventsQuery = query(uri)?;
  let resumed = headers
    .get("last-event-id")
    .map(|value| {
      value
        .to_str()
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .ok_or_else(|| Reply::bad_request("Last-Event-ID is not an event's number"))
    })
    .transpose()?;
  let mut kinds = Vec::new();

  for name in asked.kinds.as_deref().unwrap_or_default().split(',').filter(|one| !one.is_empty()) {
    kinds.push(
      EventKind::of_name(name)
        .ok_or_else(|| Reply::bad_request(format!("{name} is not a kind of event")))?,
    );
  }

  Ok(Asked {
    after: asked.after.or(resumed),
    kinds,
    machine_name: asked.machine_name,
    limit: asked.limit.map(|limit| limit as usize),
  })
}

fn sent(published: &Published) -> Sent {
  let event = &published.event;

  Sent::default()
    .id(event.id.to_string())
    .event(event.kind.name())
    .data(serde_json::to_string(event).unwrap_or_default())
}

pub async fn events(service: Shared, uri: Uri, headers: HeaderMap) -> Response {
  let asked = match asked(&uri, &headers) {
    Ok(asked) => asked,
    Err(reply) => return reply.into_response(),
  };
  let wants_page = headers
    .get(header::ACCEPT)
    .and_then(|value| value.to_str().ok())
    .is_some_and(|accept| accept.contains("application/json"));

  if wants_page {
    return page(&service, asked).into_response();
  }

  let (backlog, receiver) = match service.events.subscribe(asked.after) {
    Ok(taken) => taken,
    Err(why) => return gone(why).into_response(),
  };
  let Asked {
    kinds,
    machine_name,
    ..
  } = asked;
  let carried = move |published: &Published| published.carried(&kinds, machine_name.as_deref());
  let carried_too = carried.clone();
  let kept = stream::iter(
    backlog
      .into_iter()
      .filter(carried)
      .map(|published| Ok::<_, Infallible>(sent(&published))),
  );
  let live = stream::unfold(receiver, move |mut receiver| {
    let carried = carried_too.clone();

    async move {
      loop {
        match receiver.recv().await {
          Ok(published) if carried(&published) => return Some((Ok(sent(&published)), receiver)),
          Ok(_) => continue,
          // A reader fallen behind ends here and resumes from the last event it read.
          Err(RecvError::Lagged(_) | RecvError::Closed) => return None,
        }
      }
    }
  });

  // Every stream ends once the service is told to stop, so none holds the drain open.
  let until_stopped = service.stopping.clone().cancelled_owned();

  Sse::new(kept.chain(live).take_until(until_stopped))
    .keep_alive(KeepAlive::new().interval(KEEP_OPEN))
    .into_response()
}

/// The kept events as a page, for a reader that polls: after the number it names, or every one kept.
fn page(service: &crate::Service, asked: Asked) -> Answer<EventPage> {
  let after = asked.after.unwrap_or_else(|| service.events.floor());
  let carried: Vec<Published> = service
    .events
    .after(after)
    .map_err(gone)?
    .into_iter()
    .filter(|published| published.carried(&asked.kinds, asked.machine_name.as_deref()))
    .collect();
  let limit = asked.limit.unwrap_or(usize::MAX);
  let more = carried.len() > limit;

  Ok(Json(EventPage {
    items: carried.into_iter().take(limit).map(|published| published.event).collect(),
    more,
  }))
}

pub async fn health(service: Shared) -> Answer<Health> {
  let ready = service.ready();

  Ok(Json(Health {
    ready,
    reason: (!ready).then(|| "the loops have not started".to_owned()),
    version: bmo_node_plugin::VERSION.to_owned(),
  }))
}
