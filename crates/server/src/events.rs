//! The events the service publishes (docs/api.md, 「Events」): each handed to every stream open on its
//! kind at the moment it is published, and to nobody where none is.

use bmo_node_client::events::{Event, NodesChanged};
use tokio::sync::broadcast;

// How many events a slow stream may fall behind before it skips the oldest.
const BEHIND_MOST: usize = 256;

#[derive(Clone)]
pub struct Events(broadcast::Sender<Event>);

impl Default for Events {
  fn default() -> Self {
    Self(broadcast::channel(BEHIND_MOST).0)
  }
}

impl Events {
  /// An event published to every stream open now.
  pub fn publish(&self, event: Event) {
    let _ = self.0.send(event);
  }

  /// The node list moved.
  pub fn nodes_changed(&self) {
    self.publish(Event::NodesChanged(NodesChanged {}));
  }

  /// A stream of every event published from now on.
  pub fn subscribe(&self) -> broadcast::Receiver<Event> {
    self.0.subscribe()
  }
}

impl hardware::machine::Publish for Events {
  fn publish(&self, event: Event) {
    Events::publish(self, event);
  }
}
