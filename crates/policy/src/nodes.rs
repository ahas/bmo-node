//! What a node is called and how it is reached (docs/nodes.md, 「What a node is」 and 「What a row holds」):
//! its connection types and the rule its name holds to.

use serde::{Deserialize, Serialize};

/// A node's connection type: logged into over SSH, or a jake agent reached through its hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
  Ssh,
  Jake,
}

impl NodeKind {
  /// Every connection type.
  pub const ALL: [Self; 2] = [Self::Ssh, Self::Jake];

  /// The connection type a code names, or none where it names none.
  pub fn of_code(code: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|kind| kind.name() == code)
  }

  /// The code the column and every answer hold.
  pub fn name(self) -> &'static str {
    match self {
      Self::Ssh => "ssh",
      Self::Jake => "jake",
    }
  }
}

/// The longest a node's name may be.
pub const NAME_MAX: usize = 32;

/// Whether a name is a node's: lowercase letters, digits and hyphens, at most 32, starting with a letter or
/// a digit. A name is a herdr token value and a path segment of the console alike, so it holds to what both
/// accept.
pub fn is_node_name(name: &str) -> bool {
  let mut chars = name.chars();

  name.len() <= NAME_MAX
    && chars.next().is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
    && chars.all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == '-')
}
