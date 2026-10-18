//! Every route of docs/api.md, each behind the bearer token.

mod agency;
mod hardware;
mod nodes;
mod stream;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use store::nodes::Node;
use subtle::ConstantTimeEq;

use crate::Service;
use crate::reply::Reply;

pub type Shared = State<Arc<Service>>;

pub fn router(service: Arc<Service>) -> Router {
  Router::new()
    .route("/machine/room", post(agency::machine_room))
    .route("/links/state", post(agency::link_state))
    .route("/nodes/list", post(agency::node_rows))
    .route("/nodes/login", post(agency::node_login))
    .route("/nodes", get(nodes::list).post(nodes::add))
    .route("/nodes/view", get(nodes::views))
    .route("/nodes/check", post(nodes::check))
    .route("/nodes/{name}", axum::routing::patch(nodes::set).delete(nodes::remove))
    .route("/nodes/{name}/reach", get(nodes::reach))
    .route("/nodes/{name}/target", get(nodes::target))
    .route("/hubs", post(nodes::add_hub).delete(nodes::remove_hub))
    .route("/hardware/tracking", post(hardware::tracking))
    .route("/hardware/cpu-limit", post(hardware::cpu_limit))
    .route("/hardware/{machine}", get(hardware::latest))
    .route("/hardware/{machine}/live", get(hardware::live))
    .route("/hardware/{machine}/history", get(hardware::history))
    .route("/machine/standing", get(stream::standing))
    .route("/events", get(stream::events))
    .route("/health", get(stream::health))
    .fallback(no_route)
    .layer(middleware::from_fn_with_state(service.clone(), bearer))
    .with_state(service)
}

async fn no_route() -> Reply {
  Reply::Failed(StatusCode::NOT_FOUND, "no-route", "no route at this method and path".into())
}

/// Every route needs the configured token.
async fn bearer(State(service): Shared, request: Request, next: Next) -> Response {
  let given = request
    .headers()
    .get(header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "));

  if !given.is_some_and(|given| same(given, &service.token)) {
    return Reply::Failed(StatusCode::UNAUTHORIZED, "unauthorized", "the token is missing or wrong".into())
      .into_response();
  }

  next.run(request).await
}

/// A listed node by its name, or the refusal saying none is.
fn node(service: &Service, name: &str) -> Result<Node, Reply> {
  service.store.node(name)?.ok_or_else(|| Reply::no_node(name))
}

/// Now, in seconds and in milliseconds since the epoch.
fn now_seconds() -> i64 {
  store::now_seconds()
}

fn now_ms() -> i64 {
  ::hardware::now_ms()
}

/// The token compared whole in constant time, so how long a refusal takes says nothing of how much of
/// the token a caller had right.
fn same(given: &str, token: &str) -> bool {
  given.as_bytes().ct_eq(token.as_bytes()).into()
}
