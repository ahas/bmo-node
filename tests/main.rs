//! The crate's one test harness (bmo-chat's docs/conventions.md, 「Rust builds」): each file of tests a
//! module of it, sharing the process.

mod support;

mod cli;
mod dependencies;
mod hardware;
mod layout;
mod literals;
mod llm_bridge;
mod machine;
mod node_check;
mod node_exec;
mod openapi;
mod policy;
mod server;
mod store;
