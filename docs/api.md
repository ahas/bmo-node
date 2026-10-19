# The node service's boundary

This document states the fixed API of the node service, version **1.0.0**: every route and event it
answers, the client it publishes, its configuration and its command. A caller pins 1.0.0 or a later 1.x
for every route and field below. A change to anything here changes this document in the same commit and
follows the version rule (「Versions」).

Routes are grouped by the resource they act on. The service names no caller and keeps no list of callers:
any unit holding the token calls any route.

## Wire format

The service answers in the common wire format of bmo-next (bmo-chat's `docs/conventions.md`, 「The common
wire format」): camelCase keys, times as one integer of epoch milliseconds, every failure and refusal a
non-2xx status with a body of `code` and `error`, and lists read by one cursor and `limit`. What the
service adds to it:

- **Every route needs `Authorization: Bearer <token>`,** the configured token.
- **A field written with a question mark** may be null or left out of an answer, and may be left out of a
  request.
- **A figure is a number in the unit its name says,** so a duration is named by its unit (`pointSeconds`,
  `uptimeSeconds`, `minutes`), and only a time is in milliseconds.
- **A node is named by its name,** which is its key: lowercase letters, digits and hyphens, at most 32.
- **A machine** is named as the hardware monitor knows it: `local` for this machine, a node's own name for
  a node.
- **A list** takes `after`, the key of the last item already read, and `limit`, the most items to answer;
  with neither it answers whole.

| Status | Code | When |
|---|---|---|
| 400 | `bad-request` | the body or a query is not the shape the route takes |
| 401 | `unauthorized` | the token is missing or wrong |
| 404 | `no-route` | no route answers this method and path |
| 404 | `no-node` | no node of that name is listed; for a machine, no machine of that name |
| 404 | `no-hub` | no hub of that address is listed |
| 409 | `name-taken` | a node of that name is already listed |
| 409 | `hub-taken` | the hub is already listed |
| 409 | `not-ssh` | the node is reached through a hub and has no SSH route |
| 409 | `incomplete` | the node lacks a field its connection type needs |
| 422 | `name-rule` | the name is not lowercase letters, digits and hyphens of at most 32 characters |
| 422 | `bad-field` | a field is out of range or of the other connection type; the error names it |
| 500 | `failed` | the service failed; the error is the failure as a sentence |

## The client

The service publishes its own client, and a caller uses it rather than writing one by hand.

- **`docs/openapi.json`** states every route below in OpenAPI 3.1: each route's method and path, its
  query and body, its answer and the error answers it can give, with every body as a JSON Schema, and
  the events' bodies among the schemas. A body is described as its sender writes it, so a field the
  sender may leave out is not required, and a nullable field is required and may be null.
- **`bmo-node-client`** (`crates/client`), the Rust crate, holds every body of this document as a type,
  generates the OpenAPI document from those types, and calls every route and reads the event stream. Its
  tests fail where the committed document differs from what it generates; `cargo run -p bmo-node-client
  --example openapi > docs/openapi.json` writes it again.
- **`bmo-node-client`** (`packages/node-client`), the TypeScript package, is generated from the same
  document: its types by `openapi-typescript`, its calls by `openapi-fetch`. `bun run generate` writes the
  types again, and its check fails where they differ from the document.
- This document is the canonical statement: where the document, the code and the clients disagree, the
  code is repaired and both clients generated again.

## Nodes

| Method and path | Sends | Answers |
|---|---|---|
| `GET /nodes` | query `after?`, `limit?` | `nodes`, in the list's order, each a node |
| `GET /nodes/{name}` | | the node |
| `POST /nodes/check` | `name?` | the one node named, or every node, checked now (`docs/nodes.md`); then `nodes` as `GET /nodes` answers them whole |
| `GET /nodes/{name}/target` | | the node's connection (below) |
| `GET /nodes/{name}/login` | | `args`: the arguments that open a batch SSH login to the node, ahead of the command; refused `not-ssh` for a node reached through a hub |
| `POST /nodes` | `name`, `host`, `user`, `port?`, `keyPath?`, `shell?`, `label?`, `role?` | 201 and the new ssh node; it goes last |
| `PATCH /nodes/{name}` | any of `label`, `role`, `host`, `user`, `port`, `keyPath`, `shell`; null clears `keyPath` and `role` | the node; a changed address, port, account, key or shell clears the last check |
| `DELETE /nodes/{name}` | | `removed`, the name |

A node holds no credential, only where one is kept:

| Field | What it is |
|---|---|
| `name`, `label` | the node's name and label |
| `kind` | `ssh` or `jake` |
| `agent?` | a jake node's agent, by the name its hub knows it by |
| `address` | where it is reached: `user@host`, `:port` after it where the port is not 22 and ` (PowerShell)` where the login answers in PowerShell; `agent @ hub` for a jake node |
| `credential?` | where the credential is kept: an ssh node's key path, or the token file of a jake node's hub |
| `role?` | the services the node is reserved for, joined by `+` |
| `state` | the last check's state: `connected`, `disconnected`, `auth`, `hostkey`, `unreachable`, `error` or `unknown` (`docs/nodes.md`, 「The states」) |
| `tone` | `good`, `bad` or `unknown`, as the policy's node states give it |
| `detail?`, `checkedAt?`, `connectedAt?` | the last check's line, when it was made, and when the node was last found connected |
| `up` | the last check found the node connected and was made within the link window (`docs/nodes.md`, 「A node's link」) |
| `thisMachine` | an ssh node whose host names this machine (`docs/nodes.md`, 「This machine」) |
| `takesJobs` | an ssh node other than this machine, whose login is not PowerShell's and whose role is none or names `jobs` |

A caller words each state and tone itself; the service hands the codes.

The connection is the one form a node is reached by, whatever reaches it: `kind`, and `role?`, then

- for an ssh node: `host`, `port`, `user`, `keyPath?`, `shell` (`posix` or `powershell`), and `options`:
  the OpenSSH options of a batch login, each given after `-o`, the options a key brings among them where
  the node names a key. The same options serve `ssh`, `sftp` and `scp` alike; `keyPath` is as the row
  holds it, a leading `~/` meaning the home of the account that reads it;
- for a jake node: `hubUrl`, `tokenFile` and `agent`.

A connection missing a field its kind needs is refused `incomplete`.

The login's arguments are the connection laid out for `ssh`: each option after `-o`, `-i` and the key
with its home expanded where the node names one, `-p` and the port, and `user@host` last.

A jake node is never registered by a route: it is added when its agent attaches to a listed hub
(`docs/nodes.md`). A jake node takes `label` and `role` alone. Every change that moves the list publishes
`nodes-changed`.

## Hubs

| Method and path | Sends | Answers |
|---|---|---|
| `POST /hubs` | `url`, `tokenFile` | 201 and `url` and `tokenFile` as kept |
| `DELETE /hubs` | query `url` | `url` and `removed`, the names of the jake nodes reached through it, removed with it |

## Hardware

| Method and path | Sends | Answers |
|---|---|---|
| `GET /hardware/{machine}` | | the machine's latest reading (below) |
| `GET /hardware/{machine}/live` | query `after?`, `limit?`, `metrics?` | the live record's readings of the machine taken after `after`, no further back than the record reaches: the first `limit` after `after`, or the last `limit` where `after` is not given; only the figures measuring the comma-separated `metrics` where it names any. `at`, each reading's time, and `series`, each figure's values at those times, null where a reading did not carry it |
| `GET /hardware/{machine}/history` | query `span?` | one span of the machine's history ending now (`docs/hardware.md`, 「History」): `at`, each point's start, `pointSeconds`, and `series`, each figure's `min`, `max` and `mean` per point, null where nothing was recorded; `span` is `5m`, `15m`, `1h`, `6h`, `24h`, `7d` or `30d`, `1h` where none is named, and any other is `bad-request` |
| `PUT /hardware/{machine}/tracking` | `on` | `machine` and `on` as kept; the recorder starts or stops the machine's live run at once |
| `PUT /hardware/{machine}/cpu-limit` | `celsius` | `machine` and `celsius` as kept; null clears the limit, and a limit is above 0 °C and below 127 °C |

A machine other than `local` that is not a listed node is refused `no-node`.

The latest reading:

| Field | What it is |
|---|---|
| `name`, `label` | the machine's name and how it is shown |
| `kind` | `local`, `ssh` or `jake` |
| `hardware?` | the reading as the monitor draws it: `host?`, `kernel?`, `os?`, `hostOs?`, `wsl`, `sensorLibrary?`, `uptimeSeconds?`, `load?`, and one group per piece of hardware, `cpu`, `memory`, `gpus`, `storage`, `board` and `services` (`docs/hardware.md`); null where the machine was not read |
| `failure?` | what was met where the machine could not be read |
| `readAt` | when the reading was taken |
| `tracking` | whether the machine's live tracking is on; a machine not tracked has neither a reading nor a failure |
| `cpuLimit?` | the CPU limit set for the machine |

A figure that could not be read is null, and a CPU power the account may not read is the code `denied`; a
caller words both.

## This machine

| Method and path | Answers |
|---|---|
| `GET /machine/room` | `held`: what this machine is short of for another launch, read now, each an English reason with its figure; empty where it has room (`docs/machine.md`) |
| `GET /machine/standing` | this machine as the minute watch last read it (`docs/machine.md`): `reading?`, `held`, `browsers`, `places` and `largest` |

- **The held reasons** are `memory pressure <full>% >= <limit>%`, `available memory <available>GB <
  <limit>GB`, `load <load> >= <threads> threads x <limit>` and `disk free <free>GB < <limit>GB`, each
  figure to a tenth and a whole one without a point.
- `reading?`: `at`, `load1`, `threads`, `memTotalGb`, `memAvailableGb`, `swapTotalGb`, `swapFreeGb`,
  `diskFreeGb?`, and `memory?` and `cpu?`, each pressure's `some` and `full?` lines with their `avg10`,
  `avg60` and `avg300` in percent; null where the machine could not be read.
- `held`: as `GET /machine/room` answers, from that reading.
- `browsers`: how many browser main processes run.
- `places`: what each agent place holds, largest first: `place`, `megabytes` and `browsers`.
- `largest`: the five largest processes: `pid`, `name`, `megabytes` and `place?`.

## Events

- **`GET /events?kinds=<name>,<name>`** answers a stream of server-sent events of the kinds named, every
  kind where none is, from the moment it is opened: each event named by its kind and its data the event's
  JSON. An event published while no stream reads its kind is written out for nobody.
- **`nodes-changed`** carries an empty object: a pass moved a node's state, or the list changed (a node
  added, renamed, relabelled, moved or removed). A consumer showing the nodes reads them again.
- **`machine-pressure`** is a pressure on this machine told once (`docs/machine.md`):
  - cause `memory`: `fullPct`, the memory `full` over the last minute, `alertPct`, the alert line, and
    `minutes` it has stood; with `reading`, `places` and `largest` as the standing holds them;
  - cause `disk`: `freeGb`, the data root's disk's free space, and `limitGb`, the launch line; with
    `reading`.

## Health

- **`GET /health`** answers `pid`, `version` and `ready`: the store is open and migrated and the loops
  have started.
- **`bmo-node --config <file> health`** asks the service the configuration names, waiting at most five
  seconds, and ends 0 where it answers ready. The deployment waits on it.

## Configuration

The service's binary, `bmo-node`, reads its configuration from the TOML file its `--config` names, at
start, and holds it for its life. Deployed, the file stands under the data root as `env/node.toml`, and
each release links to it as `node.toml`; the unit does not start where the data root holds none.

| Setting | Key | What it is |
|---|---|---|
| listen address | `listen` | where the service answers, as `host:port` |
| token | `token` | the bearer token every caller presents |
| data root | `data_root` | the shared data root; node's own files stand in its `node/` folder (`docs/store.md`) |
| this machine's name | `machine_label` | how this machine is shown, where none is set the system's host name |

## Calls the node service makes

The node service calls no other unit. It reaches the machines themselves:

- **an ssh node** through OpenSSH, with the policy's options (`docs/nodes.md`);
- **a jake hub,** an outside program a jake agent attaches to, over its HTTP control address with the
  token read from the hub's token file for each request: `GET /status` for the agents attached and
  departed, and `POST /exec?agent=<agent>` to run a command on an agent's machine.

## The command

`bmo-node --config <file>` followed by:

- `serve`: runs the migrations, then the routes, the check pass, the recorder and the machine watch;
- `health`: as above;
- `node` with the verbs of `docs/nodes.md`, 「Registering and changing a node」, against the same store;
  `--target <name>` prints the node's connection as `GET /nodes/{name}/target` answers it;
- `hardware-setup [<node>…]`: the Windows sensor setup (`docs/hardware.md`, 「The Windows side」).

## Versions

The repository's version is the workspace's, in semver, and both clients carry it. The major moves on a
change to this document's routes, fields or events, or to a table's contract; the minor on an addition to
either; the patch on anything else.
