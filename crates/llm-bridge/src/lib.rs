//! The tunnel door (docs/llm-bridge.md). A node's network lets only HTTPS out, so a tunnel's SSH reaches
//! this machine inside a WebSocket that the proxy hands here; the door takes one only when its bearer is the
//! key of a service named in the door's list, and pipes it to this machine's own SSH. The list and the keys
//! are read from their files for each connection and never kept, logged or answered with.

pub mod door;
pub mod keys;
