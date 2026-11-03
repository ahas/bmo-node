//! The MCP server: the harness tools over one node service, each call made through the node client.

use std::sync::Arc;

use bmo_node_client::NodeClient;
use rmcp::model::{
  CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, InitializeResult,
  JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use schemars::Schema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value};

use crate::tools::{NodeTool, ToolFailure, tools};

/// What a harness does first with the server, as the OpenAPI document's `x-harness` states it.
pub const ENTRY: &str = "A harness asks node_target how to reach a node before it logs in, and node_machine \
  how loaded a machine is before it starts heavy work there.";

/// What the server tells a harness of itself when it starts.
const INSTRUCTIONS: &str = "Reads the machines reachable from here through one node service. List the nodes \
  with node_nodes and ask node_target how to reach one before logging in: its sshArgs open a batch SSH \
  login ahead of the command. Ask node_machine how loaded a machine is before starting heavy work there, \
  node_readings for its past figures and node_memory for who held its memory. node_check probes nodes now. \
  A refusal comes back as a tool error carrying the service's status, code and sentence.";

/// The server over the service the client reaches.
pub struct NodeServer {
  client: NodeClient,
  tools: Arc<Vec<NodeTool>>,
}

impl NodeServer {
  pub fn new(client: NodeClient) -> Self {
    Self {
      client,
      tools: Arc::new(tools()),
    }
  }

  /// The tools as MCP lists them, each input and output a schema of its own with its definitions inside.
  pub fn listed(&self) -> Vec<Tool> {
    self.tools.iter().map(listed).collect()
  }
}

fn listed(tool: &NodeTool) -> Tool {
  let mut generator = SchemaSettings::draft2020_12()
    .for_serialize()
    .with(|settings| settings.meta_schema = None)
    .into_generator();
  let input = tool.input(&mut generator);
  let output = tool.output(&mut generator);
  let definitions = generator.take_definitions(true);
  let mut listed = Tool::new(tool.name, tool.description, standing_alone(input, &definitions));

  listed.output_schema = Some(Arc::new(standing_alone(output, &definitions)));
  listed
}

/// A schema made to stand on its own: an object at its root, with every definition it refers to beside it.
fn standing_alone(schema: Schema, definitions: &Map<String, Value>) -> JsonObject {
  let mut object = match schema.to_value() {
    Value::Object(object) if object.contains_key("$ref") => {
      let mut wrapped = Map::new();

      wrapped.insert("allOf".into(), Value::Array(vec![Value::Object(object)]));
      wrapped.insert("type".into(), "object".into());
      wrapped
    }
    Value::Object(object) => object,
    _ => unreachable!("every tool's input and output is an object"),
  };

  if !definitions.is_empty() {
    object.insert("$defs".into(), Value::Object(definitions.clone()));
  }

  object
}

impl ServerHandler for NodeServer {
  fn get_info(&self) -> InitializeResult {
    let mut info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build());
    let mut server = Implementation::default();

    server.name = "bmo-node".to_owned();
    server.version = env!("CARGO_PKG_VERSION").to_owned();
    info.server_info = server;
    info.instructions = Some(INSTRUCTIONS.to_owned());
    info
  }

  async fn list_tools(
    &self,
    _: Option<PaginatedRequestParams>,
    _: RequestContext<RoleServer>,
  ) -> Result<ListToolsResult, ErrorData> {
    Ok(ListToolsResult::with_all_items(self.listed()))
  }

  async fn call_tool(
    &self,
    request: CallToolRequestParams,
    _: RequestContext<RoleServer>,
  ) -> Result<CallToolResponse, ErrorData> {
    let Some(tool) = self.tools.iter().find(|tool| tool.name == request.name) else {
      return Err(ErrorData::invalid_params(format!("there is no tool {}", request.name), None));
    };
    let arguments = Value::Object(request.arguments.unwrap_or_default());
    let result = match tool.call(self.client.clone(), arguments).await {
      Ok(answer) => CallToolResult::structured(answer),
      Err(failure) => failed(&failure),
    };

    Ok(CallToolResponse::Complete(result))
  }
}

/// A failed call as a tool error whose text is the failure's content. It carries no structured content,
/// which would be read against the tool's output schema.
fn failed(failure: &ToolFailure) -> CallToolResult {
  CallToolResult::error(vec![ContentBlock::text(failure.content().to_string())])
}
