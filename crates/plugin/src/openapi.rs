//! The OpenAPI document of the service (docs/api.md, 「The clients and the harness plugin」): every route
//! docs/api.md lists, with its path and query parameters, its request body, its answer and the error
//! answers it can give; the events under `x-events`, the harness plugin under `x-harness` and its tools
//! under `x-tools`. The body schemas come from the client's types, each described as its sender writes it.

use std::collections::BTreeMap;

use bmo_node_client::events::{EventKind, EventPage, EventsQuery};
use bmo_node_client::files::{DownloadQuery, Upload, UploadQuery};
use bmo_node_client::hardware::{
  MemorySamplePage, MemorySamplesQuery, Reading, ReadingPage, ReadingsQuery,
};
use bmo_node_client::machines::{Health, Machine, MachineChanges, MachinePage};
use bmo_node_client::nodes::{
  Hub, HubPage, NewHub, NewNode, Node, NodeChanges, NodeCheck, NodeCheckAsk, NodeKey, NodePage,
  NodeRemoveQuery, NodesQuery, Target,
};
use bmo_node_client::runs::{Run, RunAsk};
use bmo_node_client::{ErrorCode, WireError};
use schemars::generate::{SchemaGenerator, SchemaSettings};
use schemars::{JsonSchema, Schema};
use serde_json::{Map, Value, json};

use crate::server::ENTRY;
use crate::tools::tools;

/// Where the schemas the routes refer to stand in the document.
const SCHEMAS: &str = "/components/schemas";

const DESCRIPTION: &str = "The node service: the machines reachable from here and what is known of them. \
  Nodes and hubs, how to reach a node, checking whether each answers, and each machine's hardware readings. \
  A machine carries its readings, tracking and CPU limit; whether a launch may start on it is the caller's \
  own policy, read from these fields. Bodies are JSON with camelCase keys, and times are integers of epoch \
  milliseconds. Every route needs the service's bearer token, but a run and a file transfer, which need a \
  caller's own token. Every failure and refusal is a non-2xx status \
  with a body of `code` and `error`. A page is `items` and `more`. Any other method and path is answered \
  404 `no-route`.";

/// The service's OpenAPI document.
pub fn openapi() -> Value {
  let mut generator = SchemaSettings::draft2020_12()
    .for_serialize()
    .with(|settings| {
      settings.definitions_path = SCHEMAS.into();
      settings.meta_schema = None;
    })
    .into_generator();
  let error = generator.subschema_for::<WireError>();
  let mut paths: BTreeMap<&str, Map<String, Value>> = BTreeMap::new();

  for route in routes(&mut generator) {
    paths
      .entry(route.path)
      .or_default()
      .insert(route.method.to_owned(), route.operation(&error));
  }

  let events: Map<String, Value> = EventKind::ALL
    .into_iter()
    .map(|kind| {
      let schema = match kind {
        EventKind::NodeCreated | EventKind::NodeChanged => generator.subschema_for::<Node>(),
        EventKind::NodeDeleted => generator.subschema_for::<NodeKey>(),
        EventKind::MachineChanged => generator.subschema_for::<Machine>(),
        EventKind::ReadingCreated => generator.subschema_for::<Reading>(),
      };

      (kind.name().to_owned(), json!({ "data": schema.to_value() }))
    })
    .collect();
  let tools: Vec<Value> = tools()
    .iter()
    .map(|tool| {
      json!({
        "name": tool.name,
        "description": tool.description,
        "input": tool.input(&mut generator).to_value(),
        "output": tool.output(&mut generator).to_value(),
        "route": tool.route,
      })
    })
    .collect();
  let responses: Map<String, Value> = ErrorCode::ALL
    .into_iter()
    .map(|code| (response_name(code), answered(&error, &[code])))
    .collect();

  json!({
    "openapi": "3.1.0",
    "info": {
      "title": "bmo-node",
      "version": env!("CARGO_PKG_VERSION"),
      "description": DESCRIPTION,
    },
    "servers": [{ "url": "/", "description": "the node service" }],
    "security": [{ "bearer": [] }],
    "tags": [
      { "name": "health", "description": "Whether the service stands." },
      { "name": "events", "description": "The one event stream: nodes, machines and hardware readings." },
      { "name": "nodes", "description": "The machines reachable from here, and how to reach each." },
      { "name": "node-checks", "description": "Checking whether nodes answer, now." },
      { "name": "hubs", "description": "The hubs hub nodes are reached through." },
      { "name": "machines", "description": "Every machine's hardware, read live and kept for a month." },
    ],
    "x-layer": "upstream",
    "x-calls": [],
    "x-harness": {
      "plugin": "bmo-node",
      "mcp": "bmo-node mcp",
      "entry": ENTRY,
    },
    "paths": paths,
    "x-events": events,
    "x-tools": tools,
    "components": {
      "schemas": generator.take_definitions(true),
      "responses": responses,
      "securitySchemes": {
        "bearer": {
          "type": "http",
          "scheme": "bearer",
          "description": "The service's token, from the caller's own configuration; on a run and on a file \
            transfer, the caller's own token.",
        },
      },
    },
  })
}

/// The document as the repository keeps it: two-space indented JSON with a closing newline.
pub fn openapi_json() -> String {
  let mut text = serde_json::to_string_pretty(&openapi()).expect("a JSON value always renders");

  text.push('\n');
  text
}

/// An answer in the error shape with one of these codes, all answered with one status.
fn answered(error: &Schema, codes: &[ErrorCode]) -> Value {
  let meaning = |code: ErrorCode| format!("`{code}`: {}", code.meaning());
  let description = match codes {
    [code] => meaning(*code),
    _ => codes.iter().map(|&code| format!("- {}", meaning(code))).collect::<Vec<_>>().join("\n"),
  };

  json!({
    "description": description,
    "content": { "application/json": { "schema": error } },
  })
}

/// The name the answer of one code stands under in the document: the code's own name in Rust.
fn response_name(code: ErrorCode) -> String {
  format!("{code:?}")
}

/// What a route answers on success.
enum Answer {
  /// 200 and a body.
  Read(Schema),
  /// 201 and the member made.
  Made(Schema),
  /// 204 and no body.
  Nothing,
  /// The event stream, or its page to a reader asking for JSON.
  Events(Schema),
  /// 200 and raw bytes, of the length the answer declares.
  Bytes,
}

/// The body or the answer of one whole file, streamed rather than read.
fn binary() -> Value {
  json!({ "type": "string", "format": "binary" })
}

/// One route as the document states it.
struct Route {
  method: &'static str,
  path: &'static str,
  tag: &'static str,
  id: &'static str,
  summary: &'static str,
  /// The path and query parameters, as OpenAPI parameter objects.
  parameters: Vec<Value>,
  body: Option<Schema>,
  /// Whether the body is one whole file's raw bytes rather than JSON.
  raw_body: bool,
  answer: Answer,
  /// Whether the route can answer 500 `failed`.
  can_fail: bool,
  /// The refusals the route can answer, each with its code's status.
  refuses: Vec<ErrorCode>,
}

impl Route {
  fn new(
    method: &'static str,
    path: &'static str,
    tag: &'static str,
    id: &'static str,
    summary: &'static str,
    answer: Answer,
  ) -> Self {
    Self {
      method,
      path,
      tag,
      id,
      summary,
      parameters: Vec::new(),
      body: None,
      raw_body: false,
      answer,
      can_fail: true,
      refuses: Vec::new(),
    }
  }

  /// A path parameter of type `T`, written `{name}` in the path.
  fn at<T: JsonSchema>(mut self, generator: &mut SchemaGenerator, name: &str, description: &str) -> Self {
    self.parameters.push(json!({
      "name": name,
      "in": "path",
      "required": true,
      "description": description,
      "schema": generator.subschema_for::<T>(),
    }));
    self
  }

  /// The query parameters, one for each field of the query's type.
  fn query<Query: JsonSchema>(mut self, generator: &mut SchemaGenerator) -> Self {
    let schema = generator.root_schema_for::<Query>();
    let required: Vec<&str> = schema
      .get("required")
      .and_then(Value::as_array)
      .map(|names| names.iter().filter_map(Value::as_str).collect())
      .unwrap_or_default();
    let properties = schema.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();

    for (name, mut property) in properties {
      let description = property.as_object_mut().and_then(|fields| fields.remove("description"));

      // A query parameter is sent or left out, never sent as null.
      if let Some(Value::Array(types)) = property.get_mut("type") {
        types.retain(|one| one != "null");

        if let [only] = types.as_slice() {
          property["type"] = only.clone();
        }
      }
      let mut parameter = json!({
        "name": name,
        "in": "query",
        "required": required.contains(&name.as_str()),
        "schema": property,
      });

      if let Some(description) = description {
        parameter["description"] = description;
      }

      self.parameters.push(parameter);
    }

    self
  }

  fn body<Body: JsonSchema>(mut self, generator: &mut SchemaGenerator) -> Self {
    self.body = Some(generator.subschema_for::<Body>());
    self
  }

  /// A body of raw bytes rather than JSON: one whole file, streamed as it arrives.
  fn raw_body(mut self) -> Self {
    self.raw_body = true;
    self
  }

  fn never_fails(mut self) -> Self {
    self.can_fail = false;
    self
  }

  fn refuses(mut self, codes: &[ErrorCode]) -> Self {
    self.refuses.extend_from_slice(codes);
    self
  }

  /// The route's operation. A status that one code answers refers to that code's answer; a status that
  /// several share describes them together.
  fn operation(self, error: &Schema) -> Value {
    let mut codes = Vec::new();

    if self.body.is_some()
      || self.raw_body
      || self.parameters.iter().any(|parameter| parameter["in"] == "query")
    {
      codes.push(ErrorCode::BadRequest);
    }

    // Every route refuses a token that names a caller where the route takes the other kind (docs/api.md,
    // 「Wire format」).
    codes.push(ErrorCode::Unauthorized);
    codes.push(ErrorCode::Forbidden);

    if self.can_fail {
      codes.push(ErrorCode::Failed);
    }

    let mut statuses: BTreeMap<u16, Vec<ErrorCode>> = BTreeMap::new();

    for &code in codes.iter().chain(&self.refuses) {
      let shared = statuses.entry(code.status()).or_default();

      if !shared.contains(&code) {
        shared.push(code);
      }
    }

    let (status, success) = match &self.answer {
      Answer::Read(schema) => (
        "200",
        json!({
          "description": self.summary,
          "content": { "application/json": { "schema": schema } },
        }),
      ),
      Answer::Made(schema) => (
        "201",
        json!({
          "description": self.summary,
          "content": { "application/json": { "schema": schema } },
        }),
      ),
      Answer::Nothing => ("204", json!({ "description": "Done." })),
      Answer::Events(page) => (
        "200",
        json!({
          "description": "Server-sent events, each named by its kind with its number as its id and its data an \
            Event; or, to a reader asking `Accept: application/json`, a page of the same events.",
          "content": {
            "text/event-stream": { "schema": { "$ref": format!("#{SCHEMAS}/Event") } },
            "application/json": { "schema": page },
          },
        }),
      ),
      Answer::Bytes => (
        "200",
        json!({
          "description": self.summary,
          "content": { "application/octet-stream": { "schema": binary() } },
        }),
      ),
    };
    let mut responses = Map::new();

    responses.insert(status.into(), success);

    for (status, codes) in statuses {
      let answer = match codes.as_slice() {
        [code] => json!({ "$ref": format!("#/components/responses/{}", response_name(*code)) }),
        _ => answered(error, &codes),
      };

      responses.insert(status.to_string(), answer);
    }

    let mut operation = json!({
      "tags": [self.tag],
      "operationId": self.id,
      "summary": self.summary,
      "responses": responses,
    });

    if !self.parameters.is_empty() {
      operation["parameters"] = self.parameters.into();
    }

    let content = match (self.raw_body, self.body) {
      (true, _) => Some(json!({ "application/octet-stream": { "schema": binary() } })),
      (false, Some(body)) => Some(json!({ "application/json": { "schema": body } })),
      (false, None) => None,
    };

    if let Some(content) = content {
      operation["requestBody"] = json!({ "required": true, "content": content });
    }

    operation
  }
}

fn read<T: JsonSchema>(generator: &mut SchemaGenerator) -> Answer {
  Answer::Read(generator.subschema_for::<T>())
}

fn made<T: JsonSchema>(generator: &mut SchemaGenerator) -> Answer {
  Answer::Made(generator.subschema_for::<T>())
}

/// Every route of docs/api.md.
fn routes(generator: &mut SchemaGenerator) -> Vec<Route> {
  use ErrorCode::{BadField, Conflict, Gone, Incomplete, NameRule, NotAHubNode, NotFound, UpstreamFailed};

  const NODE: &str = "The node's name.";
  const HUB: &str = "The hub's id.";
  const MACHINE: &str = "`local` for the service's own machine, a node's name for a node.";

  vec![
    Route::new("get", "/health", "health", "getHealth", "Readiness and version.", read::<Health>(generator))
      .never_fails(),
    Route::new(
      "get",
      "/events",
      "events",
      "streamEvents",
      "The one event stream: nodes, machines and hardware readings, resumed with `Last-Event-ID` or `after`.",
      Answer::Events(generator.subschema_for::<EventPage>()),
    )
    .query::<EventsQuery>(generator)
    .never_fails()
    .refuses(&[Gone]),
    Route::new("get", "/nodes", "nodes", "listNodes", "Every node, in the list's order.", read::<NodePage>(generator))
      .query::<NodesQuery>(generator),
    Route::new("post", "/nodes", "nodes", "createNodes", "Add an ssh node; it goes last.", made::<Node>(generator))
      .body::<NewNode>(generator)
      .refuses(&[Conflict, NameRule, BadField]),
    Route::new("get", "/nodes/{nodeName}", "nodes", "getNode", "One node.", read::<Node>(generator))
      .at::<String>(generator, "nodeName", NODE)
      .refuses(&[NotFound]),
    Route::new(
      "patch",
      "/nodes/{nodeName}",
      "nodes",
      "changeNode",
      "Change a node: the fields sent, null clearing `keyPath` and `role`. A changed address, port, account, \
       key or shell clears the last check; a hub node takes `label` and `role` alone.",
      read::<Node>(generator),
    )
    .at::<String>(generator, "nodeName", NODE)
    .body::<NodeChanges>(generator)
    .refuses(&[NotFound, BadField]),
    Route::new(
      "delete",
      "/nodes/{nodeName}",
      "nodes",
      "deleteNode",
      "Remove a node; with `leave`, a hub node's agent is sent a leave through its hub first, and the node stays \
       where the hub does not take it.",
      Answer::Nothing,
    )
    .at::<String>(generator, "nodeName", NODE)
    .query::<NodeRemoveQuery>(generator)
    .refuses(&[NotFound, Conflict]),
    Route::new(
      "get",
      "/nodes/{nodeName}/target",
      "nodes",
      "getNodesTarget",
      "How to reach a node: host, port, user, key, the SSH policy options, and the same spread as SSH \
       arguments; or the hub, its token file and the agent for a hub node.",
      read::<Target>(generator),
    )
    .at::<String>(generator, "nodeName", NODE)
    .refuses(&[NotFound, Incomplete]),
    Route::new(
      "post",
      "/nodes/{nodeName}/runs",
      "nodes",
      "createNodesRuns",
      "Run one command on a hub node through its hub, as the caller its own token names, for the person whose \
       account is named: the node's callers must name the caller and the node must be that person's own. \
       Answered with the command's exit code and the last 64 KiB of each stream; keeps nothing.",
      read::<Run>(generator),
    )
    .at::<String>(generator, "nodeName", NODE)
    .body::<RunAsk>(generator)
    .refuses(&[NotFound, NotAHubNode, Incomplete, UpstreamFailed]),
    Route::new(
      "post",
      "/nodes/{nodeName}/file",
      "nodes",
      "createNodesFile",
      "Write one file on a hub node through its hub, as the caller its own token names, for the person whose \
       account is named: the node's callers must name the caller and the node must be that person's own. The \
       body is the file's raw bytes, streamed on to the hub unread, declared whole by `total` and `checksum`, \
       which the node's agent verifies before it moves the file into place. Keeps nothing.",
      read::<Upload>(generator),
    )
    .at::<String>(generator, "nodeName", NODE)
    .query::<UploadQuery>(generator)
    .raw_body()
    .refuses(&[NotFound, NotAHubNode, Incomplete, UpstreamFailed]),
    Route::new(
      "get",
      "/nodes/{nodeName}/file",
      "nodes",
      "getNodesFile",
      "Read one file back from a hub node through its hub, as the caller its own token names, for the person \
       whose account is named: the same judgement as a run. The answer is the file's raw bytes with \
       `Content-Length` its byte count; a file the node does not hold is refused `not-found` naming its path.",
      Answer::Bytes,
    )
    .at::<String>(generator, "nodeName", NODE)
    .query::<DownloadQuery>(generator)
    .refuses(&[NotFound, NotAHubNode, Incomplete, UpstreamFailed]),
    Route::new(
      "post",
      "/node-checks",
      "node-checks",
      "createNodeChecks",
      "Probe the nodes named, or every node, now; each node's state and checkedAt follow.",
      made::<NodeCheck>(generator),
    )
    .body::<NodeCheckAsk>(generator)
    .refuses(&[BadField]),
    Route::new("get", "/hubs", "hubs", "listHubs", "The hubs listed.", read::<HubPage>(generator)),
    Route::new("post", "/hubs", "hubs", "createHubs", "Add a hub.", made::<Hub>(generator))
      .body::<NewHub>(generator)
      .refuses(&[Conflict, BadField]),
    Route::new(
      "delete",
      "/hubs/{hubId}",
      "hubs",
      "deleteHub",
      "Remove a hub and every hub node reached through it.",
      Answer::Nothing,
    )
    .at::<i64>(generator, "hubId", HUB)
    .refuses(&[NotFound]),
    Route::new(
      "get",
      "/machines",
      "machines",
      "listMachines",
      "Every machine the hardware monitor knows, each with its latest recorded reading.",
      read::<MachinePage>(generator),
    ),
    Route::new(
      "get",
      "/machines/{machineName}",
      "machines",
      "getMachine",
      "One machine: its latest reading, tracking and CPU limit, and for this machine its standing.",
      read::<Machine>(generator),
    )
    .at::<String>(generator, "machineName", MACHINE)
    .refuses(&[NotFound]),
    Route::new(
      "patch",
      "/machines/{machineName}",
      "machines",
      "changeMachine",
      "Change a machine's tracking or CPU limit; null clears the limit.",
      read::<Machine>(generator),
    )
    .at::<String>(generator, "machineName", MACHINE)
    .body::<MachineChanges>(generator)
    .refuses(&[NotFound, BadField]),
    Route::new(
      "get",
      "/machines/{machineName}/readings",
      "machines",
      "listMachinesReadings",
      "Past hardware readings, oldest first: the stretches of `spanMs` after `after` and before `before`.",
      read::<ReadingPage>(generator),
    )
    .at::<String>(generator, "machineName", MACHINE)
    .query::<ReadingsQuery>(generator)
    .refuses(&[NotFound]),
    Route::new(
      "get",
      "/machines/{machineName}/memory",
      "machines",
      "listMachinesMemorySamples",
      "Who held a machine's memory, oldest first: every five seconds over the last fifteen minutes, and each \
       minute's sample of the lowest `MemAvailable` for seven days.",
      read::<MemorySamplePage>(generator),
    )
    .at::<String>(generator, "machineName", MACHINE)
    .query::<MemorySamplesQuery>(generator)
    .refuses(&[NotFound]),
  ]
}
