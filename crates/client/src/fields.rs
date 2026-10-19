//! How a change's field is read where leaving it out and sending null mean different things.

use serde::{Deserialize, Deserializer};

/// A field that was sent, null among what may be sent: left out reads as none, through the field's
/// default, and anything sent as some.
pub fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
  D: Deserializer<'de>,
  T: Deserialize<'de>,
{
  T::deserialize(deserializer).map(Some)
}
