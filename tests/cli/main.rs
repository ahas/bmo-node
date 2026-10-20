//! The `bmo-node` command: the node verbs read from their arguments and run against a store, the list as
//! it prints, the Windows sensor setup read and handed to a Windows machine over its own road, and the
//! node list's own rules as the store keeps them.

#[path = "../support/binary.rs"]
mod binary;

mod list;
mod nouns;
mod setup;
mod store_rules;
mod verbs;
