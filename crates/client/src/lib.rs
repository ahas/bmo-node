//! The node service's client (docs/api.md): every body its routes take and answer and every event it
//! publishes as a type, and a call for every route. The service itself answers in these types, and the
//! OpenAPI document the harness plugin generates is drawn from them, so the document is what it serves.

mod call;
mod error;
pub mod events;
mod fields;
pub mod hardware;
pub mod machines;
pub mod nodes;
mod page;

pub use call::{CallError, CallResult, NodeClient};
pub use error::{ErrorCode, WireError};
