//! The contract: the committed `docs/openapi.json` is what the code generates, and it states every route,
//! event and tool docs/api.md lists, the harness plugin among them.

use std::path::Path;

use serde_json::Value;

fn committed() -> String {
  std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/openapi.json")).unwrap()
}

#[test]
fn the_committed_document_is_what_the_code_generates() {
  assert!(
    committed() == bmo_node_plugin::openapi_json(),
    "docs/openapi.json differs from the code; run cargo run -p bmo-node-plugin --example openapi > docs/openapi.json"
  );
}

#[test]
fn the_document_states_the_routes_events_and_tools_of_the_api_document() {
  let document: Value = serde_json::from_str(&committed()).unwrap();
  let mut routes: Vec<String> = document["paths"]
    .as_object()
    .unwrap()
    .iter()
    .flat_map(|(path, methods)| {
      methods
        .as_object()
        .unwrap()
        .keys()
        .map(move |method| format!("{} {path}", method.to_uppercase()))
    })
    .collect();

  routes.sort();
  assert_eq!(
    routes,
    [
      "DELETE /hubs/{hubId}",
      "DELETE /nodes/{nodeName}",
      "GET /events",
      "GET /health",
      "GET /hubs",
      "GET /machines",
      "GET /machines/{machineName}",
      "GET /machines/{machineName}/memory",
      "GET /machines/{machineName}/readings",
      "GET /nodes",
      "GET /nodes/{nodeName}",
      "GET /nodes/{nodeName}/target",
      "PATCH /machines/{machineName}",
      "PATCH /nodes/{nodeName}",
      "POST /hubs",
      "POST /node-checks",
      "POST /nodes",
      "POST /nodes/{nodeName}/runs",
    ]
  );
  assert_eq!(
    document["x-events"].as_object().unwrap().keys().collect::<Vec<_>>(),
    ["machine.changed", "node.changed", "node.created", "node.deleted", "reading.created"]
  );
  assert_eq!(
    (document["x-harness"]["plugin"].as_str(), document["x-harness"]["mcp"].as_str()),
    (Some("bmo-node"), Some("bmo-node mcp"))
  );
  assert_eq!(document["info"]["version"], env!("CARGO_PKG_VERSION"));

  // Every tool calls a route the document states.
  for tool in document["x-tools"].as_array().unwrap() {
    let route = tool["route"].as_str().unwrap();

    assert!(routes.iter().any(|one| one == route), "{route}");
  }
}
