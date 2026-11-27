**English** | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# bmo-node

The node service of bmo: the machines reachable from here and what is known of them.

- **The node list:** ssh machines and the agents of hubs, one list every other unit reads, each node
  checked once a minute for whether it answers (`docs/nodes.md`).
- **The hardware monitor:** every machine read live, a reading a second, over its own road, and kept for a
  month as minute and half-hour rows (`docs/hardware.md`).
- **The machine:** this machine's standing read every minute, with memory or disk pressure that stands told
  as an event (`docs/machine.md`). Whether a launch may start is the caller's own policy.
- **The tunnel door:** `llm-bridge`, which a node's tunnel dials in by over HTTPS (`docs/llm-bridge.md`).
- **The hosted tool:** `bmo/node_exec`, a command run on a hub node for a caller whose account owns the node or
  that holds the administrator's grant (`docs/tools.md`).
- **A run:** a command a unit that is not a session runs on a hub node as itself, where the node's own setting
  lets it and the node is the person's it acts for (`docs/api.md`, 「Runs」).

The version is the set's, read off the git tags when a build is made (`docs/api.md`, 「Versions」).

## Layout

| Folder | Holds |
|---|---|
| `crates/` | The Rust crates: the client, the harness plugin, the policy files, the store, the node check, the hardware monitor, the tunnel door and the server binary |
| `deploy/` | The service units, `bmo-node.service` and `bmo-llm-bridge.service` |
| `docs/` | The canonical documents, and `openapi.json`, the contract the clients are generated from |
| `packages/` | The TypeScript client `bmo-node-client`, generated from `docs/openapi.json` |
| `tools/` | The hosted tool `node-exec`: its manifest, schemas, manual and the crate of its executable |
| `tests/` | Every test, with the checks that span the repository at the top and the probe's fixtures under `tests/hardware/fixtures` |

## Running it

- `bmo-node --config <file> serve` runs the service; the configuration file's settings are in
  `docs/api.md`, 「Configuration」.
- `bmo-node --config <file> node` lists and changes the nodes; `bmo-node --config <file> hardware-setup`
  puts the Windows sensor library in place on the nodes that need it.
- `bmo-node mcp --url <url>` is the harness plugin's MCP server, its token in `BMO_NODE_TOKEN`; the
  `bmo-node` skill under `crates/plugin/skills` tells a harness when and how to use it.
- The units in `deploy/` run the service and the door under systemd's user manager.

## The boundary

`docs/api.md` is the whole contract: the routes the service answers, the events it publishes, the clients
and the harness plugin it publishes, and its configuration. `docs/openapi.json` states it in OpenAPI 3.1,
generated from the code; the Rust client `bmo-node-client` (`crates/client`) and the TypeScript client
(`packages/node-client`) stand on it. The service names no other unit: a run asks the owners' read its
configuration names whose node it is (`docs/nodes.md`, 「A person's node」), and the hosted tool asks agency
whose node it is and whose the caller is (`docs/tools.md`). `docs/store.md` says what it keeps and where.

`PERSONAL-DATA.md` states the personal data no file may hold: whose it is, what counts, where a value belongs
instead, and the checks that hold it.

## Building and testing

- `cargo check --workspace --all-targets`
- `cargo test --workspace`, which runs the repository-wide checks.
- The testbed's quick checks, the `personal-data` check among them: it fails
  on a tracked file holding a mail address outside the reserved example domains, a person's home path, a
  telephone number, a token or a private network address (`PERSONAL-DATA.md`).
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`
- `bun run check` in `packages/node-client`, which installs its locked packages first
