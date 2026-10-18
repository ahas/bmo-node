# The node service's boundary

Every route and event the node service answers, every call it makes, its configuration and its command.
This document is the contract other units match: agency, which calls the node service for this
machine's room and the nodes; bmo-job, whose node routes agency answers from the reads below; and the web
console, whose node and monitor screens read and change what is stated here. A change to anything here
changes this document in the same commit and follows the version rule (「Versions」).

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

## Conventions

- **Bodies are JSON, both ways, with field names in camelCase,** as the chat wire protocol writes them
  (the protocol document of `bmo-chat`). A field written with a question mark may be null or left out.
- **Times are Unix seconds,** and a figure is a number in the unit its name says.
- **Every route needs `Authorization: Bearer <token>`,** the configured token; a missing or wrong token is
  answered 401.
- **A refusal is an answer:** 200 with `answered` false and a `refusal` carrying its `code` and an English
  `message`. A route's own answer is its object as stated, with no `answered` field.
- **A request that could not be answered at all** is non-2xx with `code` and an English `error`:
  `bad-request` (400, the body or a query is not the shape the route takes), `unauthorized` (401),
  `no-route` (404), `not-ready` (409, the store is not open yet), `failed` (500, the failure as the
  sentence).
- **A machine** is named as the hardware monitor knows it: `local` for this machine, a node's own name for
  a node.

| Refusal code | When |
|---|---|
| `no-node` | no node of that name is listed |
| `name-taken` | a node of that name is already listed |
| `name-rule` | the name is not lowercase letters, digits and hyphens of at most 32 characters |
| `bad-field` | a field is out of range or of the other connection type; the message names it |
| `hub-taken` | the hub is already listed |
| `no-hub` | no hub of that address is listed |
| `not-ssh` | the node is reached through a hub and has no SSH route |
| `incomplete` | the node lacks a field its connection type needs |

## Agency's calls

Agency asks these four (agency's `docs/api.md`, 「The node service」). Each is one POST of a JSON body,
an empty object where nothing is sent.

| Path | Sends | Answers |
|---|---|---|
| `/machine/room` | nothing | `held`: what this machine is short of for another launch, each an English reason with its figure; empty where it has room (`docs/machine.md`) |
| `/links/state` | `node` | `up` (boolean): the node's last check found it connected and was made within the link window (`docs/nodes.md`, 「A node's link」); false for a node not listed |
| `/nodes/list` | nothing | `nodes`, in the list's order: each `node`, `label`, `kind` (`ssh` or `jake`), `state` and `detail?` |
| `/nodes/login` | `node` | `ssh`: the arguments that open a batch SSH login to the node, ahead of the command, or null where the node has no SSH route |

- **The held reasons** are `memory pressure <full>% >= <limit>%`, `available memory <available>GB <
  <limit>GB`, `load <load> >= <threads> threads x <limit>` and `disk free <free>GB < <limit>GB`, each
  figure to a tenth and a whole one without a point.
- **The login's arguments** are the batch options of the policy's `ssh.toml`, the key options and the key
  where the node names one, the port where it is not 22, and `user@host` last.

## Node reads

The node list's rows as bmo-job's node routes answer them; agency hands them on.

| Method and path | Answers |
|---|---|
| `GET /nodes` | `nodes`, in the list's order: each `name`, `label`, `kind`, `role?`, `state`, `checkedAt?` |
| `GET /nodes/{name}/reach` | an ssh node's `host`, `port`, `user`, `keyPath?` and `shell?` (`posix` or `powershell`); refused `not-ssh` for a node reached through a hub, `no-node` for one not listed |

## Console routes

### The node list

| Method and path | Sends | Answers |
|---|---|---|
| `GET /nodes/view` | | `nodes`, in the list's order, each a node view |
| `POST /nodes/check` | `name?` | the one node named, or every node, checked now (`docs/nodes.md`); then `nodes` as `GET /nodes/view` answers |

A node view holds no credential:

| Field | What it is |
|---|---|
| `name`, `label` | the node's name and label |
| `kind` | `ssh` or `jake` |
| `agent?` | a jake node's agent, by the name its hub knows it by |
| `address` | where it is reached: `user@host`, `:port` after it where the port is not 22 and ` (PowerShell)` where the login answers in PowerShell; `agent @ hub` for a jake node |
| `credential?` | where the credential is kept: an ssh node's key path, or the token file of a jake node's hub |
| `role?` | the services the node is reserved for |
| `state` | the last check's state (`docs/nodes.md`, 「The states」) |
| `tone` | `good`, `bad` or `unknown`, as the policy's node states give it |
| `detail?`, `checkedAt?`, `connectedAt?` | the last check's line, when it was made, and when the node was last found connected |
| `thisMachine` | an ssh node whose host names this machine (`docs/nodes.md`, 「This machine」) |
| `takesJobs` | an ssh node other than this machine, whose login is not PowerShell's and whose role is none or names `jobs` |

The console words each state and tone itself; the service hands the codes.

### Hardware

| Method and path | Sends | Answers |
|---|---|---|
| `GET /hardware/{machine}` | | the machine's latest reading (below) |
| `GET /hardware/{machine}/live` | query `since?`, `metrics?` | the live record's readings of the machine from `since`, no further back than the record reaches, and only the figures measuring the comma-separated `metrics` where it names any: `at`, each reading's second, and `series`, each figure's values at those seconds, null where a reading did not carry it |
| `GET /hardware/{machine}/history` | query `span?` | one span of the machine's history ending now (`docs/hardware.md`, 「History」): `at`, each point's start, `pointSeconds`, and `series`, each figure's `min`, `max` and `mean` per point, null where nothing was recorded; `span` is `5m`, `15m`, `1h`, `6h`, `24h`, `7d` or `30d`, `1h` where none is named, and any other is `bad-request` |
| `POST /hardware/tracking` | `machine`, `on` | `machine` and `on` as kept; the recorder starts or stops the machine's live run at once |
| `POST /hardware/cpu-limit` | `machine`, `celsius?` | `machine` and `celsius` as kept; null clears the limit |

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

A figure that could not be read is null, and a CPU power the account may not read is the code
`denied`; the console words both.

### The machine's standing

| Method and path | Answers |
|---|---|
| `GET /machine/standing` | this machine as the minute watch last read it (`docs/machine.md`): `reading?`, `held`, `browsers`, `places` and `largest` |

- `reading?`: `at` (milliseconds), `load1`, `threads`, `memTotalGb`, `memAvailableGb`, `swapTotalGb`,
  `swapFreeGb`, `diskFreeGb?`, and `memory?` and `cpu?`, each pressure's `some` and `full?` lines with
  their `avg10`, `avg60` and `avg300` in percent; null where the machine could not be read.
- `held`: as `/machine/room` answers.
- `browsers`: how many browser main processes run.
- `places`: what each agent place holds, largest first: `place`, `megabytes` and `browsers`.
- `largest`: the five largest processes: `pid`, `name`, `megabytes` and `place?`.

## Registry routes

| Method and path | Sends | Answers |
|---|---|---|
| `POST /nodes` | `name`, `host`, `user`, `port?`, `keyPath?`, `shell?`, `label?`, `role?` | the new ssh node's view; it goes last |
| `PATCH /nodes/{name}` | any of `label`, `role`, `host`, `user`, `port`, `keyPath`, `shell`; null clears `keyPath` and `role` | the node's view; a changed address, port, account, key or shell clears the last check |
| `DELETE /nodes/{name}` | | `removed`, the name |
| `POST /hubs` | `url`, `tokenFile` | `url` and `tokenFile` |
| `DELETE /hubs` | query `url` | `url` and `removed`, the names of the jake nodes reached through it, removed with it |
| `GET /nodes/{name}/target` | | the node's connection for a tool that reaches it by name: `kind`, then an ssh node's `host`, `port`, `user`, `keyPath?`, `shell?`, or a jake node's `hubUrl`, `tokenFile` and `agent`; and `role?`; refused `incomplete` where a field its kind needs is missing |

A jake node is never registered by a route: it is added when its agent attaches to a listed hub
(`docs/nodes.md`). A jake node takes `label` and `role` alone. Every change that moves the list publishes
`nodes-changed`.

## Events

- **`GET /events?kinds=<name>,<name>`** answers a stream of server-sent events of the kinds named, every
  kind where none is, from the moment it is opened: each event named by its kind and its data the event's
  JSON. An event published while no stream reads its kind is written out for nobody.
- **`nodes-changed`** carries nothing: a pass moved a node's state, or the list changed (a node added,
  renamed, relabelled, moved or removed). A consumer showing the nodes reads them again.
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
- `hardware-setup [<node>…]`: the Windows sensor setup (`docs/hardware.md`, 「The Windows side」).

## Versions

The repository's version is the workspace's, in semver. The major moves on a change to this document's
routes, fields or events, or to a table's contract; the minor on an addition to either; the patch on
anything else.
