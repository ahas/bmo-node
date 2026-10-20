//! The node service's routes asked over HTTP, each answer held to the committed OpenAPI document: the
//! nodes, the node checks and the targets, the hubs, the machines and their readings, the event stream and
//! the health; the harness plugin's MCP server over them; and the pass, the names and the reading taken
//! once that stand behind them.

#[path = "../support/binary.rs"]
mod binary;
mod hubs;
mod machines;
mod mcp;
mod names;
mod nodes;
mod once;
mod pass;
mod stream;
mod support;
