//! The OpenAPI document of the service: every route docs/api.md lists, with its path and query parameters,
//! its request body, its answer and the error answers it can give, for a caller written in any language.
//! The body schemas come from this crate's types, each described as its sender writes it.

use std::collections::BTreeMap;

use schemars::generate::{SchemaGenerator, SchemaSettings};
use schemars::{JsonSchema, Schema};
use serde_json::{Map, Value, json};

use crate::error::{ErrorCode, WireError};
use crate::events::{EventKind, EventsQuery, MachinePressure, NodesChanged};
use crate::hardware::{
  CpuLimit, CpuLimitSet, HistoryQuery, HistoryRange, LatestReading, LiveQuery, LiveRange, Tracking,
  TrackingSet,
};
use crate::machine::{Health, Room, Standing};
use crate::nodes::{
  CheckNodes, Hub, HubQuery, HubRemoved, Login, NewNode, Node, NodeChanges, NodeList, NodeListQuery,
  Removed, Target,
};

/// Where the schemas the routes refer to stand in the document.
const SCHEMAS: &str = "/components/schemas";

const DESCRIPTION: &str = "The node service: the machines reachable from here and what is known of them. \
  Bodies are JSON with camelCase keys, and times are integers of epoch milliseconds; a duration is named by \
  its unit. Every route needs the service's bearer token. Every failure and refusal is a non-2xx status with \
  a body of `code` and `error`, so a 2xx answer is always the route's own. A list takes one cursor, `after`, \
  and `limit`. Any other method and path is answered 404 `no-route`.";

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
    paths.entry(route.path).or_default().insert(route.method.to_owned(), route.operation(&error));
  }

  let events: Map<String, Value> = EventKind::ALL
    .into_iter()
    .map(|kind| {
      let schema = match kind {
        EventKind::NodesChanged => generator.subschema_for::<NodesChanged>(),
        EventKind::MachinePressure => generator.subschema_for::<MachinePressure>(),
      };

      (kind.name().to_owned(), schema.to_value())
    })
    .collect();
  let responses: Map<String, Value> =
    ErrorCode::ALL.into_iter().map(|code| (response_name(code), answered(&error, &[code]))).collect();

  json!({
    "openapi": "3.1.0",
    "info": {
      "title": "The node service",
      "version": env!("CARGO_PKG_VERSION"),
      "description": DESCRIPTION,
    },
    "tags": [
      { "name": "nodes", "description": "The machines reachable from here, each checked for whether it answers." },
      { "name": "hubs", "description": "The jake hubs jake nodes are reached through." },
      { "name": "hardware", "description": "Every machine's hardware, read live and kept for a month." },
      { "name": "machine", "description": "The service's own machine: its room for another launch and its standing." },
      { "name": "events", "description": "The events the service publishes, as server-sent events." },
      { "name": "health", "description": "Whether the service stands." },
    ],
    "security": [{ "bearer": [] }],
    "paths": paths,
    "x-events": events,
    "components": {
      "schemas": generator.take_definitions(true),
      "responses": responses,
      "securitySchemes": {
        "bearer": {
          "type": "http",
          "scheme": "bearer",
          "description": "The service's token, from the caller's own configuration.",
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
  Json(Schema),
  Events,
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
  answer: Answer,
  /// Whether success is 201, a thing made, rather than 200.
  creates: bool,
  /// Whether the route can answer 500 `failed`.
  can_fail: bool,
  /// The refusals the route can answer, each with its code's status.
  refuses: Vec<ErrorCode>,
}

impl Route {
  fn new(method: &'static str, path: &'static str, tag: &'static str, id: &'static str, answer: Answer) -> Self {
    Self {
      method,
      path,
      tag,
      id,
      summary: "",
      parameters: Vec::new(),
      body: None,
      answer,
      creates: false,
      can_fail: true,
      refuses: Vec::new(),
    }
  }

  fn summary(mut self, summary: &'static str) -> Self {
    self.summary = summary;
    self
  }

  /// A path parameter, written `{name}` in the path.
  fn at(mut self, generator: &mut SchemaGenerator, name: &str, description: &str) -> Self {
    self.parameters.push(json!({
      "name": name,
      "in": "path",
      "required": true,
      "description": description,
      "schema": generator.subschema_for::<String>(),
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

  fn creates(mut self) -> Self {
    self.creates = true;
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

    if self.body.is_some() || self.parameters.iter().any(|parameter| parameter["in"] == "query") {
      codes.push(ErrorCode::BadRequest);
    }

    codes.push(ErrorCode::Unauthorized);

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

    let success = match &self.answer {
      Answer::Json(schema) => json!({
        "description": if self.creates { "Made." } else { "Answered." },
        "content": { "application/json": { "schema": schema } },
      }),
      Answer::Events => json!({
        "description": "A stream of server-sent events, each named by its kind, its data the event's JSON as \
          the document's `x-events` gives the schema of each kind.",
        "content": { "text/event-stream": { "schema": { "type": "string" } } },
      }),
    };
    let mut responses = Map::new();

    responses.insert(if self.creates { "201" } else { "200" }.into(), success);

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

    if let Some(body) = self.body {
      operation["requestBody"] = json!({
        "required": true,
        "content": { "application/json": { "schema": body } },
      });
    }

    operation
  }
}

fn json_answer<T: JsonSchema>(generator: &mut SchemaGenerator) -> Answer {
  Answer::Json(generator.subschema_for::<T>())
}

/// Every route of docs/api.md.
fn routes(generator: &mut SchemaGenerator) -> Vec<Route> {
  use ErrorCode::{BadField, HubTaken, Incomplete, NameRule, NameTaken, NoHub, NoNode, NotSsh};

  const NODE: &str = "The node's name.";
  const MACHINE: &str = "`local` for the service's own machine, a node's name for a node.";

  vec![
    Route::new("get", "/nodes", "nodes", "nodes", json_answer::<NodeList>(generator))
      .summary("The nodes in the list's order, after the one `after` names and at most `limit` of them.")
      .query::<NodeListQuery>(generator),
    Route::new("get", "/nodes/{name}", "nodes", "node", json_answer::<Node>(generator))
      .summary("One node.")
      .at(generator, "name", NODE)
      .refuses(&[NoNode]),
    Route::new("patch", "/nodes/{name}", "nodes", "changeNode", json_answer::<Node>(generator))
      .summary(
        "Changes the fields sent; null clears `keyPath` and `role`. A changed address, port, account, key or \
         shell clears the last check. A jake node takes `label` and `role` alone.",
      )
      .at(generator, "name", NODE)
      .body::<NodeChanges>(generator)
      .refuses(&[NoNode, BadField]),
    Route::new("delete", "/nodes/{name}", "nodes", "removeNode", json_answer::<Removed>(generator))
      .summary("Removes a node.")
      .at(generator, "name", NODE)
      .refuses(&[NoNode]),
    Route::new("post", "/nodes", "nodes", "addNode", json_answer::<Node>(generator))
      .summary("Registers an ssh node; it goes last.")
      .body::<NewNode>(generator)
      .creates()
      .refuses(&[NameTaken, NameRule, BadField]),
    Route::new("post", "/nodes/check", "nodes", "checkNodes", json_answer::<NodeList>(generator))
      .summary("Checks the one node named, or every node, now; then answers the whole list.")
      .body::<CheckNodes>(generator)
      .refuses(&[NoNode]),
    Route::new("get", "/nodes/{name}/target", "nodes", "target", json_answer::<Target>(generator))
      .summary("The node's connection, the one form it is reached by.")
      .at(generator, "name", NODE)
      .refuses(&[NoNode, Incomplete]),
    Route::new("get", "/nodes/{name}/login", "nodes", "login", json_answer::<Login>(generator))
      .summary("The arguments that open a batch SSH login to the node, ahead of the command.")
      .at(generator, "name", NODE)
      .refuses(&[NoNode, NotSsh, Incomplete]),
    Route::new("post", "/hubs", "hubs", "addHub", json_answer::<Hub>(generator))
      .summary("Lists a hub.")
      .body::<Hub>(generator)
      .creates()
      .refuses(&[HubTaken, BadField]),
    Route::new("delete", "/hubs", "hubs", "removeHub", json_answer::<HubRemoved>(generator))
      .summary("Removes a hub and every jake node reached through it.")
      .query::<HubQuery>(generator)
      .refuses(&[NoHub, BadField]),
    Route::new("get", "/hardware/{machine}", "hardware", "hardware", json_answer::<LatestReading>(generator))
      .summary("The machine's latest reading.")
      .at(generator, "machine", MACHINE)
      .refuses(&[NoNode]),
    Route::new("get", "/hardware/{machine}/live", "hardware", "live", json_answer::<LiveRange>(generator))
      .summary(
        "The live record's readings of the machine taken after `after`, no further back than the record \
         reaches: the first `limit` after `after`, or the last `limit` where `after` is not given.",
      )
      .at(generator, "machine", MACHINE)
      .query::<LiveQuery>(generator)
      .refuses(&[NoNode]),
    Route::new("get", "/hardware/{machine}/history", "hardware", "history", json_answer::<HistoryRange>(generator))
      .summary("One span of the machine's history ending now.")
      .at(generator, "machine", MACHINE)
      .query::<HistoryQuery>(generator)
      .refuses(&[NoNode]),
    Route::new("put", "/hardware/{machine}/tracking", "hardware", "setTracking", json_answer::<Tracking>(generator))
      .summary("Turns the machine's live tracking on or off; the recorder follows at once.")
      .at(generator, "machine", MACHINE)
      .body::<TrackingSet>(generator)
      .refuses(&[NoNode]),
    Route::new("put", "/hardware/{machine}/cpu-limit", "hardware", "setCpuLimit", json_answer::<CpuLimit>(generator))
      .summary("Sets the machine's CPU limit, above 0 °C and below 127 °C; null clears it.")
      .at(generator, "machine", MACHINE)
      .body::<CpuLimitSet>(generator)
      .refuses(&[NoNode]),
    Route::new("get", "/machine/room", "machine", "room", json_answer::<Room>(generator))
      .summary("What the service's machine is short of for another launch, read now; empty where it has room."),
    Route::new("get", "/machine/standing", "machine", "standing", json_answer::<Standing>(generator))
      .summary("The service's machine as the minute watch last read it."),
    Route::new("get", "/events", "events", "events", Answer::Events)
      .summary(
        "The events of the comma-separated `kinds`, every kind where none is, from the moment the stream \
         is opened.",
      )
      .query::<EventsQuery>(generator)
      .never_fails(),
    Route::new("get", "/health", "health", "health", json_answer::<Health>(generator))
      .summary("Whether the service's store is open and migrated and its loops have started.")
      .never_fails(),
  ]
}

