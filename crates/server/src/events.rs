//! The events the service publishes (docs/api.md, 「Events」): each a kind and its JSON, handed to every
//! stream open on its kind at the moment it is published, and to nobody where none is.

use serde_json::Value;
use tokio::sync::broadcast;

/// What changes on the node list is published as.
pub const NODES_CHANGED: &str = "nodes-changed";

// How many events a slow stream may fall behind before it skips the oldest.
const BEHIND_MOST: usize = 256;

#[derive(Clone)]
pub struct Events(broadcast::Sender<(String, Value)>);

impl Default for Events {
  fn default() -> Self {
    Self(broadcast::channel(BEHIND_MOST).0)
  }
}

impl Events {
  /// An event published to every stream open now.
  pub fn publish(&self, kind: &str, data: Value) {
    let _ = self.0.send((kind.to_owned(), data));
  }

  /// A stream of every event published from now on.
  pub fn subscribe(&self) -> broadcast::Receiver<(String, Value)> {
    self.0.subscribe()
  }
}

impl hardware::machine::Publish for Events {
  fn publish(&self, kind: &str, data: Value) {
    Events::publish(self, kind, data);
  }
}
