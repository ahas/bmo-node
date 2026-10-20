//! What a node is called and how it is reached (docs/nodes.md, 「What a node is」 and 「What a row holds」):
//! its connection types and the rule its name holds to.

pub use bmo_node_client::nodes::NodeKind;

/// The longest a node's name may be.
pub const NAME_MAX: usize = 32;

/// Whether a name is a node's: lowercase letters, digits and hyphens, at most 32, starting with a letter or
/// a digit. A name is a path segment and a token in callers' own commands alike, so it holds to what both
/// accept.
pub fn is_node_name(name: &str) -> bool {
  let mut chars = name.chars();

  name.len() <= NAME_MAX
    && chars.next().is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
    && chars.all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == '-')
}
