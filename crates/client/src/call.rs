//! A call for every route of the service (docs/api.md), each sending the route's body and reading its
//! answer as the route's type, and the event stream read as events.

use std::fmt;

use futures_util::{Stream, StreamExt};
use reqwest::header::ACCEPT;
use reqwest::{Method, RequestBuilder, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sse_stream::SseStream;

use crate::error::WireError;
use crate::events::{Event, EventPage, EventsQuery};
use crate::hardware::{ReadingPage, ReadingsQuery};
use crate::machines::{Health, Machine, MachineChanges, MachinePage};
use crate::nodes::{
  Hub, HubPage, NewHub, NewNode, Node, NodeChanges, NodeCheck, NodeCheckAsk, NodePage, NodesQuery, Target,
};

/// Why a call did not come back with its route's answer.
#[derive(Debug)]
pub enum CallError {
  /// The service answered in the error shape.
  Answered { status: u16, error: WireError },
  /// The service answered something that is neither the route's answer nor the error shape.
  Unreadable { status: u16, body: String },
  /// The request did not reach the service, or its answer did not arrive whole.
  Transport(reqwest::Error),
}

impl CallError {
  /// A status other than success, with the body it came with: the error shape, or what was said instead.
  fn not_answered(status: u16, body: String) -> Self {
    match serde_json::from_str::<WireError>(&body) {
      Ok(error) => Self::Answered { status, error },
      Err(_) => Self::Unreadable { status, body },
    }
  }

  /// The error the service answered, where it answered one.
  pub fn wire(&self) -> Option<&WireError> {
    match self {
      Self::Answered { error, .. } => Some(error),
      _ => None,
    }
  }
}

impl fmt::Display for CallError {
  fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Answered { status, error } => write!(out, "the node service answered {status}: {error}"),
      Self::Unreadable { status, body } => {
        write!(out, "the node service answered {status} unreadably: {body}")
      }
      Self::Transport(why) => write!(out, "the node service could not be asked: {why}"),
    }
  }
}

impl std::error::Error for CallError {}

impl From<reqwest::Error> for CallError {
  fn from(why: reqwest::Error) -> Self {
    Self::Transport(why)
  }
}

pub type CallResult<T> = Result<T, CallError>;

/// The node service at one base address, asked with one token.
#[derive(Clone)]
pub struct NodeClient {
  base: Url,
  token: String,
  http: reqwest::Client,
}

impl NodeClient {
  /// A client of the service at `base`, an absolute HTTP address, with a client of its own.
  pub fn new(base: &str, token: impl Into<String>) -> Result<Self, String> {
    Self::with_http(base, token, reqwest::Client::new())
  }

  /// A client of the service at `base` over the HTTP client given, which sets its timeouts.
  pub fn with_http(base: &str, token: impl Into<String>, http: reqwest::Client) -> Result<Self, String> {
    let base = Url::parse(base).map_err(|why| format!("{base} is not an address: {why}"))?;

    if base.cannot_be_a_base() {
      return Err(format!("{base} cannot carry a path"));
    }

    Ok(Self {
      base,
      token: token.into(),
      http,
    })
  }

  /// The address of a path, its segments given apart so each is escaped as one.
  fn url(&self, segments: &[&str]) -> Url {
    let mut url = self.base.clone();

    if let Ok(mut path) = url.path_segments_mut() {
      path.pop_if_empty().extend(segments);
    }

    url
  }

  fn request(&self, method: Method, segments: &[&str]) -> RequestBuilder {
    self.http.request(method, self.url(segments)).bearer_auth(&self.token)
  }

  async fn answer<T: DeserializeOwned>(request: RequestBuilder) -> CallResult<T> {
    let answer = request.send().await?;
    let status = answer.status();
    let body = answer.text().await?;

    if status.is_success() {
      return serde_json::from_str(&body).map_err(|_| CallError::Unreadable {
        status: status.as_u16(),
        body,
      });
    }

    Err(CallError::not_answered(status.as_u16(), body))
  }

  async fn get<T: DeserializeOwned>(&self, segments: &[&str]) -> CallResult<T> {
    Self::answer(self.request(Method::GET, segments)).await
  }

  async fn get_query<T: DeserializeOwned>(
    &self,
    segments: &[&str],
    query: &impl Serialize,
  ) -> CallResult<T> {
    Self::answer(self.request(Method::GET, segments).query(query)).await
  }

  async fn send<T: DeserializeOwned>(
    &self,
    method: Method,
    segments: &[&str],
    body: &impl Serialize,
  ) -> CallResult<T> {
    Self::answer(self.request(method, segments).json(body)).await
  }

  /// A request answered with no body, as a removal is.
  async fn remove(&self, segments: &[&str]) -> CallResult<()> {
    let answer = self.request(Method::DELETE, segments).send().await?;
    let status = answer.status();

    if status.is_success() {
      return Ok(());
    }

    Err(CallError::not_answered(status.as_u16(), answer.text().await?))
  }

  /// `GET /health`.
  pub async fn health(&self) -> CallResult<Health> {
    self.get(&["health"]).await
  }

  /// `GET /nodes`.
  pub async fn nodes(&self, query: &NodesQuery) -> CallResult<NodePage> {
    self.get_query(&["nodes"], query).await
  }

  /// `POST /nodes`.
  pub async fn create_node(&self, node: &NewNode) -> CallResult<Node> {
    self.send(Method::POST, &["nodes"], node).await
  }

  /// `GET /nodes/{nodeName}`.
  pub async fn node(&self, node_name: &str) -> CallResult<Node> {
    self.get(&["nodes", node_name]).await
  }

  /// `PATCH /nodes/{nodeName}`.
  pub async fn change_node(&self, node_name: &str, changes: &NodeChanges) -> CallResult<Node> {
    self.send(Method::PATCH, &["nodes", node_name], changes).await
  }

  /// `DELETE /nodes/{nodeName}`.
  pub async fn delete_node(&self, node_name: &str) -> CallResult<()> {
    self.remove(&["nodes", node_name]).await
  }

  /// `GET /nodes/{nodeName}/target`.
  pub async fn target(&self, node_name: &str) -> CallResult<Target> {
    self.get(&["nodes", node_name, "target"]).await
  }

  /// `POST /node-checks`.
  pub async fn check_nodes(&self, ask: &NodeCheckAsk) -> CallResult<NodeCheck> {
    self.send(Method::POST, &["node-checks"], ask).await
  }

  /// `GET /hubs`.
  pub async fn hubs(&self) -> CallResult<HubPage> {
    self.get(&["hubs"]).await
  }

  /// `POST /hubs`.
  pub async fn create_hub(&self, hub: &NewHub) -> CallResult<Hub> {
    self.send(Method::POST, &["hubs"], hub).await
  }

  /// `DELETE /hubs/{hubId}`.
  pub async fn delete_hub(&self, hub_id: i64) -> CallResult<()> {
    self.remove(&["hubs", &hub_id.to_string()]).await
  }

  /// `GET /machines`.
  pub async fn machines(&self) -> CallResult<MachinePage> {
    self.get(&["machines"]).await
  }

  /// `GET /machines/{machineName}`.
  pub async fn machine(&self, machine_name: &str) -> CallResult<Machine> {
    self.get(&["machines", machine_name]).await
  }

  /// `PATCH /machines/{machineName}`.
  pub async fn change_machine(&self, machine_name: &str, changes: &MachineChanges) -> CallResult<Machine> {
    self.send(Method::PATCH, &["machines", machine_name], changes).await
  }

  /// `GET /machines/{machineName}/readings`.
  pub async fn readings(&self, machine_name: &str, query: &ReadingsQuery) -> CallResult<ReadingPage> {
    self.get_query(&["machines", machine_name, "readings"], query).await
  }

  /// `GET /events` asked as a page, for a caller that polls.
  pub async fn event_page(&self, query: &EventsQuery) -> CallResult<EventPage> {
    let request = self.request(Method::GET, &["events"]).query(query).header(ACCEPT, "application/json");

    Self::answer(request).await
  }

  /// `GET /events` as the stream of server-sent events, from the event `query.after` names or from now.
  pub async fn events(
    &self,
    query: &EventsQuery,
  ) -> CallResult<impl Stream<Item = CallResult<Event>> + use<>> {
    let answer = self.request(Method::GET, &["events"]).query(query).send().await?;
    let status = answer.status();

    if !status.is_success() {
      return Err(CallError::not_answered(status.as_u16(), answer.text().await?));
    }

    let events = SseStream::from_bytes_stream(answer.bytes_stream()).filter_map(|event| async move {
      let event = match event {
        Ok(event) => event,
        Err(why) => {
          return Some(Err(CallError::Unreadable {
            status: 200,
            body: why.to_string(),
          }));
        }
      };
      // A comment line carries no data.
      let data = event.data?;

      Some(serde_json::from_str::<Event>(&data).map_err(|_| CallError::Unreadable {
        status: 200,
        body: data,
      }))
    });

    Ok(events)
  }
}
