//! The node service's client (docs/api.md): every body its routes take and answer and every event it
//! publishes as a type, the OpenAPI document generated from those types, and a call for every route. The
//! service itself answers in these types, so the document is what it serves.

mod call;
mod error;
pub mod events;
mod fields;
pub mod hardware;
pub mod machine;
pub mod nodes;
mod openapi;

pub use call::{CallError, CallResult, NodeClient};
pub use error::{ErrorCode, WireError};
pub use openapi::{openapi, openapi_json};
