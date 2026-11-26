//! Every route of docs/api.md, grouped by the resource it acts on: a run and a file transfer behind a
//! caller's own token, and every other route behind the service's.

mod files;
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
use node_check::rows::Hub;
use policy::nodes::NodeKind;
use store::nodes::Node;
use subtle::ConstantTimeEq;

use crate::Service;
use crate::reply::Reply;

pub type Shared = State<Arc<Service>>;

/// The route a caller makes a run on.
const RUNS: &str = "/nodes/{nodeName}/runs";

/// The route a caller moves a file on.
const FILE: &str = "/nodes/{nodeName}/file";

/// The routes a caller calls as itself, with its own token (docs/api.md, 「Wire format」).
const AS_CALLER: [&str; 2] = [RUNS, FILE];

/// Who a token names: the service's own token names no caller, and a caller's names that caller.
enum Bearer {
  Service,
  Caller(String),
}

/// The caller a route is called as, named by its own token. The middleware mints it, and only the routes
/// `AS_CALLER` holds read it.
#[derive(Clone)]
pub struct Caller(pub String);

/// What a caller does on a node behind its own token, as a refusal names it.
#[derive(Clone, Copy)]
pub enum Doing {
  /// One command run there (docs/api.md, 「Runs」).
  Run,
  /// One file's bytes moved there (docs/api.md, 「File transfer」).
  File,
}

impl Doing {
  /// The words for the caller doing it, after "does not let the caller X".
  fn making(self) -> &'static str {
    match self {
      Self::Run => "make a run",
      Self::File => "move a file",
    }
  }

  /// The words for one act of it.
  fn thing(self) -> &'static str {
    match self {
      Self::Run => "a run",
      Self::File => "a file transfer",
    }
  }

  /// The same with none of it standing, in a sentence that lets nothing through.
  fn none(self) -> &'static str {
    match self {
      Self::Run => "no run",
      Self::File => "no file transfer",
    }
  }
}

/// A node a caller was let to: its row, and the hub and the agent its road goes through.
pub struct Reached {
  pub node: Node,
  pub hub: Hub,
  pub agent: String,
}

pub fn router(service: Arc<Service>) -> Router {
  Router::new()
    .route("/health", get(stream::health))
    .route("/events", get(stream::events))
    .route("/nodes", get(nodes::list).post(nodes::create))
    .route("/nodes/{nodeName}", get(nodes::one).patch(nodes::change).delete(nodes::remove))
    .route("/nodes/{nodeName}/target", get(nodes::target))
    .route(RUNS, post(runs::run))
    .route(FILE, post(files::upload).get(files::download))
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

/// Every route needs a token: a run and a file transfer a caller's own, and every other route the service's
/// (docs/api.md, 「Wire format」). A path no route answers is let on to the answer saying so.
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
  let as_caller = route.as_deref().is_some_and(|route| AS_CALLER.contains(&route));

  match (as_caller, bearer) {
    (true, Bearer::Caller(name)) => {
      request.extensions_mut().insert(Caller(name));
    }
    (true, Bearer::Service) => {
      return Reply::new(
        ErrorCode::Forbidden,
        "the service's own token names no caller; a run and a file transfer are made with a caller's own \
         token",
      )
      .into_response();
    }
    (false, Bearer::Caller(name)) => {
      return Reply::new(
        ErrorCode::Forbidden,
        format!("the caller {name}'s token makes a run and moves a file and calls no other route"),
      )
      .into_response();
    }
    (false, Bearer::Service) => {}
  }

  next.run(request).await
}

/// What a run and a file transfer check of a node and its caller, in the order docs/api.md states, before
/// the hub is asked anything (「Runs」 and 「File transfer」).
pub async fn let_through(
  service: &Service,
  name: &str,
  caller: &str,
  account: &str,
  doing: Doing,
) -> Result<Reached, Reply> {
  let node = node(service, name)?;

  let_in(&node, caller, doing)?;

  if node.kind != NodeKind::Hub {
    return Err(Reply::new(
      ErrorCode::NotAHubNode,
      format!(
        "The node {name} is an ssh node, which {} does not reach: log in with its target's sshArgs.",
        doing.thing()
      ),
    ));
  }

  let reached = Reached {
    hub: Hub {
      url: node.hub_url.clone().ok_or_else(|| incomplete(name))?,
      token_file: node.token_file.clone().ok_or_else(|| incomplete(name))?,
    },
    agent: node.agent.clone().ok_or_else(|| incomplete(name))?,
    node,
  };

  owned(service, name, caller, account, doing).await?;

  Ok(reached)
}

fn forbidden(error: impl Into<String>) -> Reply {
  Reply::new(ErrorCode::Forbidden, error)
}

/// The refusal saying a hub node's row lacks a field its connection type needs.
fn incomplete(name: &str) -> Reply {
  Reply::new(ErrorCode::Incomplete, format!("{name} is missing a field its connection type needs"))
}

fn upstream(error: impl Into<String>) -> Reply {
  Reply::new(ErrorCode::UpstreamFailed, error)
}

/// Whether `node` lets the caller onto it, or the refusal naming both and what it was asked to do.
fn let_in(node: &Node, caller: &str, doing: Doing) -> Result<(), Reply> {
  if node.lets(caller) {
    return Ok(());
  }

  let callers = match node.callers.as_deref() {
    Some(callers) => format!("its callers are {callers}"),
    None => "it lets no caller".to_owned(),
  };

  Err(forbidden(format!(
    "The node {} does not let the caller {caller} {}: {callers}.",
    node.name,
    doing.making()
  )))
}

/// Whether the node is the person's own whose account is `account`, or the refusal saying whose it is.
async fn owned(
  service: &Service,
  node: &str,
  caller: &str,
  account: &str,
  doing: Doing,
) -> Result<(), Reply> {
  let Some(read) = &service.owners else {
    return Err(forbidden(format!(
      "No owners' read is configured, so the node {node} is no one's and {} is let through.",
      doing.none()
    )));
  };
  let owners = read
    .owners(service.machine.http(), node, caller)
    .await
    .map_err(|why| upstream(format!("Whose node {node} is could not be read: {why}.")))?;

  if owners.iter().any(|owner| owner == account) {
    return Ok(());
  }

  Err(forbidden(match owners.as_slice() {
    [] => format!(
      "The node {node} is no person's node, so {} is made on it for account {account}.",
      doing.none()
    ),
    owners => {
      format!("The node {node} is account {}'s, not account {account}'s.", owners.join(", account "))
    }
  }))
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
