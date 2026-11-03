//! The harness tools (docs/api.md, 「The clients and the harness plugin」): one for each route a harness
//! reaches nodes and machines through, each a thin call of one method of the node client. The MCP server
//! lists them and the OpenAPI document states them under `x-tools`, both from this one table.

use std::future::Future;
use std::pin::Pin;

use bmo_node_client::hardware::{MemorySamplesQuery, ReadingsQuery};
use bmo_node_client::nodes::{NodeCheckAsk, NodesQuery};
use bmo_node_client::{CallError, NodeClient};
use schemars::generate::SchemaGenerator;
use schemars::{JsonSchema, Schema};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// What a tool's call came to short of the route's answer.
#[derive(Debug)]
pub enum ToolFailure {
  /// The arguments are not the shape the tool takes.
  Arguments(String),
  /// The call reached no answer of the route's shape.
  Call(CallError),
}

impl ToolFailure {
  /// The failure as the tool error's content: the error the service answered with and its status, or the
  /// sentence saying why nothing answered.
  pub fn content(&self) -> Value {
    match self {
      Self::Arguments(why) => json!({ "code": "bad-request", "error": why }),
      Self::Call(CallError::Answered { status, error }) => {
        json!({ "status": status, "code": error.code, "error": error.error })
      }
      Self::Call(failure @ CallError::Unreadable { status, .. }) => {
        json!({ "status": status, "error": failure.to_string() })
      }
      Self::Call(failure) => json!({ "error": failure.to_string() }),
    }
  }
}

/// A tool's answer: the route's answer as JSON, or why there is none.
pub type Answered = Pin<Box<dyn Future<Output = Result<Value, ToolFailure>> + Send>>;

type Call = Box<dyn Fn(NodeClient, Value) -> Answered + Send + Sync>;

/// One tool: its name, what it does, the route it calls, its input and output, and the call itself.
pub struct NodeTool {
  pub name: &'static str,
  pub description: &'static str,
  /// The route the tool calls, as its method and path.
  pub route: &'static str,
  input: fn(&mut SchemaGenerator) -> Schema,
  output: fn(&mut SchemaGenerator) -> Schema,
  call: Call,
}

impl NodeTool {
  /// The tool's input, its nested types referred to where `generator` keeps its definitions.
  pub fn input(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.input)(generator)
  }

  /// The tool's output, referred to where `generator` keeps its definitions.
  pub fn output(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.output)(generator)
  }

  /// Calls the tool's route on the service `client` reaches, with the tool's arguments.
  pub fn call(&self, client: NodeClient, arguments: Value) -> Answered {
    (self.call)(client, arguments)
  }
}

/// A tool whose input is `I` and whose answer is `O`, calling `call` with the arguments read as `I`.
fn tool<I, O, F, Fut>(
  name: &'static str,
  description: &'static str,
  route: &'static str,
  call: F,
) -> NodeTool
where
  I: DeserializeOwned + JsonSchema,
  O: Serialize + JsonSchema,
  F: Fn(NodeClient, I) -> Fut + Send + Sync + 'static,
  Fut: Future<Output = Result<O, CallError>> + Send + 'static,
{
  NodeTool {
    name,
    description,
    route,
    input: I::json_schema,
    output: SchemaGenerator::subschema_for::<O>,
    call: Box::new(move |client, arguments| {
      let input = match serde_json::from_value::<I>(arguments) {
        Ok(input) => input,
        Err(why) => {
          let why = format!("the arguments are not the shape this tool takes: {why}");

          return Box::pin(std::future::ready(Err(ToolFailure::Arguments(why))));
        }
      };
      let answer = call(client, input);

      Box::pin(async move {
        let answer = answer.await.map_err(ToolFailure::Call)?;

        Ok(serde_json::to_value(answer).expect("a route's answer always renders as JSON"))
      })
    }),
  }
}

/// No input.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Nothing {}

/// A body sent as it is.
#[derive(Deserialize, JsonSchema)]
#[schemars(inline)]
struct Body<T> {
  body: T,
}

/// One node, by its name.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct OneNode {
  node_name: String,
}

/// One machine, by its name: `local` for the service's own machine, a node's name for a node.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct OneMachine {
  machine_name: String,
}

/// One machine's readings: the machine, and which of its stretches to read.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MachineReadings {
  machine_name: String,
  #[serde(flatten)]
  query: ReadingsQuery,
}

/// One machine's memory samples: the machine, and which of them to read.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MachineMemory {
  machine_name: String,
  #[serde(flatten)]
  query: MemorySamplesQuery,
}

/// The eight tools, in the order docs/api.md lists them.
pub fn tools() -> Vec<NodeTool> {
  vec![
    tool(
      "node_nodes",
      "Every node, in the list's order: its name, connection type, address, role and the last check's state.",
      "GET /nodes",
      |client, query: NodesQuery| async move { client.nodes(&query).await },
    ),
    tool(
      "node_node",
      "One node, by its name.",
      "GET /nodes/{nodeName}",
      |client, OneNode { node_name }| async move { client.node(&node_name).await },
    ),
    tool(
      "node_target",
      "How to reach a node: host, port, user, key, the SSH policy options, and the same spread as SSH \
       arguments; or the hub, its token file and the agent for a hub node.",
      "GET /nodes/{nodeName}/target",
      |client, OneNode { node_name }| async move { client.target(&node_name).await },
    ),
    tool(
      "node_check",
      "Probe the nodes named, or every node, now; each node's state and checkedAt follow.",
      "POST /node-checks",
      |client, Body { body }: Body<NodeCheckAsk>| async move { client.check_nodes(&body).await },
    ),
    tool(
      "node_machines",
      "Every machine the hardware monitor knows, each with its latest recorded reading.",
      "GET /machines",
      |client, Nothing {}| async move { client.machines().await },
    ),
    tool(
      "node_machine",
      "One machine: its latest reading, tracking and CPU limit, and for this machine its standing.",
      "GET /machines/{machineName}",
      |client, OneMachine { machine_name }| async move { client.machine(&machine_name).await },
    ),
    tool(
      "node_readings",
      "Past hardware readings of one machine, oldest first, each figure's lowest, highest and mean over a \
       stretch of spanMs.",
      "GET /machines/{machineName}/readings",
      |client,
       MachineReadings {
         machine_name,
         query,
       }| async move { client.readings(&machine_name, &query).await },
    ),
    tool(
      "node_memory",
      "Who held one machine's memory, oldest first: its MemAvailable, Shmem and swap used, its largest processes \
       and its largest units; every five seconds over the last fifteen minutes, each minute's worst for seven days.",
      "GET /machines/{machineName}/memory",
      |client,
       MachineMemory {
         machine_name,
         query,
       }| async move { client.memory_samples(&machine_name, &query).await },
    ),
  ]
}
