//! The node service's routes asked over HTTP as their callers ask them: agency's four calls in the shapes
//! agency reads, job's node reads, the console's node list, check and hardware reads, the registry's
//! changes, the machine's standing, the event stream and the health; and the pass, the names and the
//! reading taken once that stand behind them.

mod agency;
mod hardware;
mod names;
mod nodes;
mod once;
mod pass;
mod registry;
mod stream;
mod support;
