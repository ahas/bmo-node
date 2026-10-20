//! Every route of docs/api.md, grouped by the resource it acts on, each behind the bearer token.

mod hubs;
mod machines;
mod nodes;
mod stream;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use bmo_node_client::ErrorCode;
use store::nodes::Node;
use subtle::ConstantTimeEq;

use crate::Service;
use crate::reply::Reply;

pub type Shared = State<Arc<Service>>;

pub fn router(service: Arc<Service>) -> Router {
  Router::new()
    .route("/health", get(stream::health))
    .route("/events", get(stream::events))
    .route("/nodes", get(nodes::list).post(nodes::create))
    .route("/nodes/{nodeName}", get(nodes::one).patch(nodes::change).delete(nodes::remove))
    .route("/nodes/{nodeName}/target", get(nodes::target))
    .route("/node-checks", post(nodes::check))
    .route("/hubs", get(hubs::list).post(hubs::create))
    .route("/hubs/{hubId}", delete(hubs::remove))
    .route("/machines", get(machines::list))
    .route("/machines/{machineName}", get(machines::one).patch(machines::change))
    .route("/machines/{machineName}/readings", get(machines::readings))
    .fallback(no_route)
    .method_not_allowed_fallback(no_route)
    .layer(middleware::from_fn_with_state(service.clone(), bearer))
    .with_state(service)
}

async fn no_route() -> Reply {
  Reply::new(ErrorCode::NoRoute, "no route at this method and path")
}

/// Every route needs the configured token.
async fn bearer(State(service): Shared, request: Request, next: Next) -> Response {
  let given = request
    .headers()
    .get(header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "));

  if !given.is_some_and(|given| same(given, &service.token)) {
    return Reply::new(ErrorCode::Unauthorized, "the token is missing or wrong").into_response();
  }

  next.run(request).await
}

/// A listed node by its name, or the refusal saying none is.
fn node(service: &Service, name: &str) -> Result<Node, Reply> {
  service.store.node(name)?.ok_or_else(|| Reply::no_node(name))
}

/// The node list's events published once a route has moved it.
fn settle(service: &Service) -> Result<(), Reply> {
  Ok(service.settle_nodes()?)
}

/// The token compared whole in constant time, so how long a refusal takes says nothing of how much of
/// the token a caller had right.
fn same(given: &str, token: &str) -> bool {
  given.as_bytes().ct_eq(token.as_bytes()).into()
}
