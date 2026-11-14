//! Every route of docs/api.md, grouped by the resource it acts on: a run behind a caller's own token, and
//! every other route behind the service's.

mod hubs;
mod machines;
mod nodes;
mod runs;
mod stream;

use std::sync::Arc;

use axum::Router;
use axum::extract::{MatchedPath, Request, State};
use axum::http::header;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use bmo_node_client::ErrorCode;
use store::nodes::Node;
use subtle::ConstantTimeEq;

use crate::Service;
use crate::reply::Reply;
use crate::routes::runs::RunCaller;

pub type Shared = State<Arc<Service>>;

/// The one route a caller calls as itself, with its own token.
const RUNS: &str = "/nodes/{nodeName}/runs";

pub fn router(service: Arc<Service>) -> Router {
  Router::new()
    .route("/health", get(stream::health))
    .route("/events", get(stream::events))
    .route("/nodes", get(nodes::list).post(nodes::create))
    .route("/nodes/{nodeName}", get(nodes::one).patch(nodes::change).delete(nodes::remove))
    .route("/nodes/{nodeName}/target", get(nodes::target))
    .route(RUNS, post(runs::run))
    .route("/node-checks", post(nodes::check))
    .route("/hubs", get(hubs::list).post(hubs::create))
    .route("/hubs/{hubId}", delete(hubs::remove))
    .route("/machines", get(machines::list))
    .route("/machines/{machineName}", get(machines::one).patch(machines::change))
    .route("/machines/{machineName}/readings", get(machines::readings))
    .route("/machines/{machineName}/memory", get(machines::memory))
    .fallback(no_route)
    .method_not_allowed_fallback(no_route)
    .layer(middleware::from_fn_with_state(service.clone(), bearer))
    .with_state(service)
}

async fn no_route() -> Reply {
  Reply::new(ErrorCode::NoRoute, "no route at this method and path")
}

/// Who a token names: the service's own token names no caller, and a caller's names that caller.
enum Bearer {
  Service,
  Caller(String),
}

/// Every route needs a token: a run a caller's own, and every other route the service's (docs/api.md,
/// 「Wire format」). A path no route answers is let on to the answer saying so.
async fn bearer(State(service): Shared, mut request: Request, next: Next) -> Response {
  let given = request
    .headers()
    .get(header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "));
  let bearer = given.and_then(|given| {
    if same(given, &service.token) {
      return Some(Bearer::Service);
    }

    service
      .callers
      .iter()
      .find(|caller| same(given, &caller.token))
      .map(|caller| Bearer::Caller(caller.name.clone()))
  });
  let Some(bearer) = bearer else {
    return Reply::new(ErrorCode::Unauthorized, "the token is missing or wrong").into_response();
  };
  let route = request.extensions().get::<MatchedPath>().map(|path| path.as_str().to_owned());

  match (route.as_deref(), bearer) {
    (Some(RUNS), Bearer::Caller(name)) => {
      request.extensions_mut().insert(RunCaller(name));
    }
    (Some(RUNS), Bearer::Service) => {
      return Reply::new(
        ErrorCode::Forbidden,
        "the service's own token names no caller; a run is made with a caller's own token",
      )
      .into_response();
    }
    (Some(_), Bearer::Caller(name)) => {
      return Reply::new(
        ErrorCode::Forbidden,
        format!("the caller {name}'s token makes a run and calls no other route"),
      )
      .into_response();
    }
    (_, _) => {}
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
