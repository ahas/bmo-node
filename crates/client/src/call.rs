//! A call for every route of the service (docs/api.md), each sending the route's body and reading its
//! answer as the route's type, and the event stream read as events.

use std::fmt;

use futures_util::{Stream, StreamExt};
use reqwest::{Method, RequestBuilder, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sse_stream::SseStream;

use crate::error::WireError;
use crate::events::{Event, EventKind};
use crate::hardware::{
  CpuLimit, CpuLimitSet, HistoryQuery, HistoryRange, LatestReading, LiveQuery, LiveRange, Tracking,
  TrackingSet,
};
use crate::machine::{Health, Room, Standing};
use crate::nodes::{
  CheckNodes, Hub, HubQuery, HubRemoved, Login, NewNode, Node, NodeChanges, NodeList, NodeListQuery,
  Removed, Target,
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
      Self::Unreadable { status, body } => write!(out, "the node service answered {status} unreadably: {body}"),
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

  async fn send<T: DeserializeOwned>(
    &self,
    method: Method,
    segments: &[&str],
    body: &impl Serialize,
  ) -> CallResult<T> {
    Self::answer(self.request(method, segments).json(body)).await
  }

  /// `GET /nodes`.
  pub async fn nodes(&self, query: &NodeListQuery) -> CallResult<NodeList> {
    Self::answer(self.request(Method::GET, &["nodes"]).query(query)).await
  }

  /// `GET /nodes/{name}`.
  pub async fn node(&self, name: &str) -> CallResult<Node> {
    self.get(&["nodes", name]).await
  }

  /// `POST /nodes/check`.
  pub async fn check_nodes(&self, which: &CheckNodes) -> CallResult<NodeList> {
    self.send(Method::POST, &["nodes", "check"], which).await
  }

  /// `GET /nodes/{name}/target`.
  pub async fn target(&self, name: &str) -> CallResult<Target> {
    self.get(&["nodes", name, "target"]).await
  }

  /// `GET /nodes/{name}/login`.
  pub async fn login(&self, name: &str) -> CallResult<Login> {
    self.get(&["nodes", name, "login"]).await
  }

  /// `POST /nodes`.
  pub async fn add_node(&self, node: &NewNode) -> CallResult<Node> {
    self.send(Method::POST, &["nodes"], node).await
  }

  /// `PATCH /nodes/{name}`.
  pub async fn change_node(&self, name: &str, changes: &NodeChanges) -> CallResult<Node> {
    self.send(Method::PATCH, &["nodes", name], changes).await
  }

  /// `DELETE /nodes/{name}`.
  pub async fn remove_node(&self, name: &str) -> CallResult<Removed> {
    Self::answer(self.request(Method::DELETE, &["nodes", name])).await
  }

  /// `POST /hubs`.
  pub async fn add_hub(&self, hub: &Hub) -> CallResult<Hub> {
    self.send(Method::POST, &["hubs"], hub).await
  }

  /// `DELETE /hubs`.
  pub async fn remove_hub(&self, url: &str) -> CallResult<HubRemoved> {
    let query = HubQuery { url: url.to_owned() };

    Self::answer(self.request(Method::DELETE, &["hubs"]).query(&query)).await
  }

  /// `GET /hardware/{machine}`.
  pub async fn hardware(&self, machine: &str) -> CallResult<LatestReading> {
    self.get(&["hardware", machine]).await
  }

  /// `GET /hardware/{machine}/live`.
  pub async fn live(&self, machine: &str, query: &LiveQuery) -> CallResult<LiveRange> {
    Self::answer(self.request(Method::GET, &["hardware", machine, "live"]).query(query)).await
  }

  /// `GET /hardware/{machine}/history`.
  pub async fn history(&self, machine: &str, query: &HistoryQuery) -> CallResult<HistoryRange> {
    Self::answer(self.request(Method::GET, &["hardware", machine, "history"]).query(query)).await
  }

  /// `PUT /hardware/{machine}/tracking`.
  pub async fn set_tracking(&self, machine: &str, on: bool) -> CallResult<Tracking> {
    self.send(Method::PUT, &["hardware", machine, "tracking"], &TrackingSet { on }).await
  }

  /// `PUT /hardware/{machine}/cpu-limit`.
  pub async fn set_cpu_limit(&self, machine: &str, celsius: Option<f64>) -> CallResult<CpuLimit> {
    self.send(Method::PUT, &["hardware", machine, "cpu-limit"], &CpuLimitSet { celsius }).await
  }

  /// `GET /machine/room`.
  pub async fn room(&self) -> CallResult<Room> {
    self.get(&["machine", "room"]).await
  }

  /// `GET /machine/standing`.
  pub async fn standing(&self) -> CallResult<Standing> {
    self.get(&["machine", "standing"]).await
  }

  /// `GET /health`.
  pub async fn health(&self) -> CallResult<Health> {
    self.get(&["health"]).await
  }

  /// `GET /events`: the events of the kinds named, every kind where none is, from now on. An event of a
  /// kind this client does not know is passed over.
  pub async fn events(
    &self,
    kinds: &[EventKind],
  ) -> CallResult<impl Stream<Item = CallResult<Event>> + use<>> {
    let mut request = self.request(Method::GET, &["events"]);

    if !kinds.is_empty() {
      let kinds: Vec<&str> = kinds.iter().map(|kind| kind.name()).collect();

      request = request.query(&[("kinds", kinds.join(","))]);
    }

    let answer = request.send().await?;
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
      let data = event.data.unwrap_or_default();

      Event::read(event.event.as_deref()?, &data)
        .map(|read| read.map_err(|_| CallError::Unreadable { status: 200, body: data }))
    });

    Ok(events)
  }
}
