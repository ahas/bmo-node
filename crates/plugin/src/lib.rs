//! The node service's harness plugin, `bmo-node` (docs/api.md, 「The clients and the harness plugin」).
//!
//! `bmo-node mcp` starts this crate's MCP server, one tool for each route a harness reaches nodes and
//! machines through, each a thin call of the node client; the skill beside this crate tells a harness when
//! and how to use it. The crate also generates the service's OpenAPI document, which carries the tools
//! under `x-tools`.

mod openapi;
mod server;
mod tools;

pub use openapi::{openapi, openapi_json};
pub use server::NodeServer;
pub use tools::{Answered, NodeTool, ToolFailure, tools};
