# bmo-node

The node service of bmo: the machines reachable from here and what is known of them.

- **The node list:** ssh machines and jake agents, one list every other unit reads, each node checked once
  a minute for whether it answers (`docs/nodes.md`).
- **The hardware monitor:** every machine read live, a reading a second, over its own road, and kept for a
  month as minute and half-hour rows (`docs/hardware.md`).
- **The machine:** whether this machine has room for another launch, and memory or disk pressure told as
  an event (`docs/machine.md`).
- **The tunnel door:** `llm-bridge`, which a node's tunnel dials in by over HTTPS (`docs/llm-bridge.md`).

Version 0.1.0.

## Running it

- `bmo-node --config <file> serve` runs the service; the configuration file's settings are in
  `docs/api.md`, 「Configuration」.
- `bmo-node --config <file> node` lists and changes the nodes; `bmo-node --config <file> hardware-setup`
  puts the Windows sensor library in place on the nodes that need it.
- The units in `deploy/` run the service and the door under systemd's user manager.

## The boundary

`docs/api.md` is the whole contract: the routes the service answers, the events it publishes and its
configuration. The service calls no other unit. `docs/store.md` says what it keeps and where.

## Building and testing

- `cargo check --workspace --all-targets`
- `cargo test --workspace`
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`
