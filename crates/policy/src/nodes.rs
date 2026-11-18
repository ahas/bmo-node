//! What a node is called, how it is reached and whose it is (docs/nodes.md, 「What a node is」, 「What a row
//! holds」 and 「A node's ownership」): its connection types, the rule its name holds to, and the name a
//! person's machine takes.

pub use bmo_node_client::nodes::{NodeKind, Ownership};

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

/// The longest an account's id may be written: the digits of a signed 64-bit number.
pub const ACCOUNT_MAX: usize = 19;

/// Whether a value is an account's id written out: a decimal number with no leading zero, of at most 19
/// digits.
pub fn is_account(value: &str) -> bool {
  value.len() <= ACCOUNT_MAX
    && value.chars().next().is_some_and(|first| first.is_ascii_digit() && first != '0')
    && value.chars().all(|one| one.is_ascii_digit())
}

/// The account a hub agent's name says its machine belongs to (docs/nodes.md, 「A node's ownership」): the
/// invitation and the pairing that bind a person's machine name it `u<user>` or `u<user>-<tail>`, the
/// account's id as a decimal number. None for any other name, which is a machine of the platform's.
pub fn personal_account(agent: &str) -> Option<String> {
  let rest = agent.strip_prefix('u')?;
  let (account, tail) = rest.split_once('-').map_or((rest, None), |(account, tail)| (account, Some(tail)));

  (is_account(account) && tail.is_none_or(|tail| !tail.is_empty())).then(|| account.to_owned())
}
