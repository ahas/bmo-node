# Working in this repository

This repository is the node service: the machines reachable from here and what is known of them. It
keeps the node list and the jake hubs, checks whether each node answers, reads every machine's hardware
live and keeps it for a month, reads this machine's room for another launch and tells a pressure that
stands, and carries the tunnel door nodes dial in by. It decides nothing about jobs, agents or rooms:
which job a process serves, whether a launch goes ahead and what the chat agent is told are the callers'.

## Rules

- **`docs/` is canonical.** `docs/api.md` is the boundary contract, every route, event and command the
  service answers and its configuration; `docs/tools.md` is the hosted tool; `docs/inventory.md` is what
  came from the old repository. Read the document covering a change first. A change that would make it wrong repairs the document first,
  stating what is true now, and the code is then written to match. A change to the boundary API changes
  `docs/api.md` in the same commit.
- **No other service's code.** Among other repositories only the base libraries `bmo-chat`,
  `bmo-chat-client` and `bmo-ui` may be depended on or imported; `tests/dependencies.rs` fails on any
  other. Another unit reaches the service through its API, and the service tells a consumer something by
  publishing an event, never by calling it.
- **A variation belongs to its member.** A value that differs per connection type, road or machine is
  answered by that member, never by a central table in the layer below.
- **No hand-written parser for a general format.** TOML, JSON, XML and command lines are read by a
  maintained library; where none fits, the reason is written beside the code. The probe's sections are
  this repository's own syntax.
- **Tests live in `tests/`.** The root package holds them in one test target, `tests/main.rs` (bmo-chat's
  `docs/conventions.md`, 「Rust builds」): a group of tests is a module directory under `tests/`, the checks
  that span the repository are files at the top of `tests/`, and what groups share is `tests/support`. No test
  module sits in a source file.
- **English only** in code, comments, strings, tests and documents, and in commit messages; `tests/literals.rs`
  fails on Hangul. Messages the service shows are English sentences; what a machine reads is a status code
  or a kebab-case code, and the web console words them.
- **No names of agents, owners or hosts, and no personal paths** in code, documents or commits. Write the
  role instead: the owner, a node, this machine. A path is written from `$HOME` or from a setting.
  `tests/literals.rs` fails on the account name, the host name and the committer's address of the machine it
  runs on.
- **No personal data in a tracked file.** A mail address outside the reserved example domains, a person's home
  path, a telephone number, a token and a private network address are never written; a value is read from a
  setting and an example is neutral. The testbed's `personal-data` check fails on one, and `PERSONAL-DATA.md` states the
  rule whole.
- **No credential in a row, an answer, an event or a log line,** only where one is kept.
- **No directory named after its parent.** `tests/layout.rs` fails on one.
- **Only Cargo manifests at the root,** beside `testbed.toml`, the testbed's declaration. The root
  `Cargo.toml` is the workspace and the root package, which is never published and exists to hold
  `tests/`. `tests/layout.rs` fails on another build tool's manifest at the root.
- **Documents state what is true now:** no change history, no quoted source and no file-and-line pins.

## Layout

- `crates/`: the Rust crates.
  - `client`: `bmo-node-client`, every body of the API as a type and a call for every route (`docs/api.md`).
  - `plugin`: `bmo-node-plugin`, the harness plugin: the MCP server `bmo-node mcp` starts, its tools, the
    `bmo-node` skill under `skills/`, and the generator of `docs/openapi.json`.
  - `policy`: the policy files, `node-states.toml`, `ssh.toml` and `windows-sensors.toml`, and their readers.
  - `store`: node's own database and the live record's path (`docs/store.md`).
  - `node-check`: whether each node answers, the SSH login's arguments and a hub's token (`docs/nodes.md`).
  - `hardware`: the probe, its parser, the recorder, the history and the machine reading
    (`docs/hardware.md`, `docs/machine.md`).
  - `llm-bridge`: the tunnel door (`docs/llm-bridge.md`).
  - `server`: the `bmo-node` binary: the routes, the loops and the command (`docs/api.md`).
- `packages/node-client`: the TypeScript client, generated from `docs/openapi.json`.
- `docs/`: the canonical documents, and `openapi.json`, the contract the clients are generated from.
- `deploy/`: the service units, `bmo-node.service` and `bmo-llm-bridge.service`.
- `tools/node-exec`: the hosted tool, its manifest, schemas and manual beside `node-exec-tool`, the crate of its
  executable (`docs/tools.md`).
- `testbed.toml`: how the testbed tests, checks, builds and deploys the repository.
- `tests/`: every test, with the probe's fixtures under `tests/hardware/fixtures`.

## Checks

- Static: `cargo check --workspace --all-targets`.
- Review: `cargo fmt --all --check` (108 columns, 2-space indentation, as `rustfmt.toml` sets) and
  `cargo clippy --workspace --all-targets -- -D warnings`.
- The TypeScript client: `bun run check` in `packages/node-client`.
- Tests: `cargo test --workspace`.
- The version is the workspace's: `0.1.0` with a revision, `0.1.0-rev.<n>`, until the first release. It moves
  only at a release the owner declares (`docs/api.md`, 「Versions」).
