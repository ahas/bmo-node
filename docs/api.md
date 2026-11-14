# The node service's boundary

This document states the API of the node service, version **0.1.0-rev.2**: every route and event it
answers, the clients and the harness plugin it publishes, its configuration and its command. A change to
anything here changes this document in the same commit.

The service is an upstream unit (bmo-chat's `docs/conventions.md`, 「Upstream and downstream」): a harness
can take it as its first entry point through its MCP server, with no other unit running. Routes are
grouped by the resource they act on. The service's own token names no caller: any caller holding it calls
any route but a run, and whether a launch may start on a machine is the caller's own policy, read from the
fields below. A run is the one route a caller calls as itself, with a token of its own the deployment's
configuration names (「Runs」, 「Configuration」); no name of a caller is written in the service.

## Wire format

The service answers in the common wire format of bmo (bmo-chat's `docs/conventions.md`, 「The common
wire format」 and 「API design」): camelCase keys, times as one integer of epoch milliseconds, every failure
and refusal a non-2xx status with a body of `code` and `error`, and lists as a page of `items` and `more`.
What the service adds to it:

- **Every route needs `Authorization: Bearer <token>`,** `GET /health` among them: the configured token on
  every route but a run, and a caller's own token on a run and on no other route.
- **A field written with a question mark** may be null or left out of an answer, and may be left out of a
  request.
- **A figure is a number in the unit its name says** (`celsius`, `watts`, `mhz`, `usedGb`, `megabytes`,
  `usePct`), and a span is in milliseconds (`spanMs`).
- **A node is keyed by its name:** lowercase letters, digits and hyphens, at most 32. A hub is keyed by its
  id, a signed 64-bit integer.
- **A machine** is named as the hardware monitor knows it: `local` for this machine, a node's own name for
  a node.
- **A page** is `{"items": [...], "more": boolean}`; `more` says whether anything stands past the last item,
  and the next page is asked with that item's key or time as `after`.

| Status | Code | When |
|---|---|---|
| 400 | `bad-request` | the body or the query is not the shape the route takes; the error names what is wrong |
| 401 | `unauthorized` | the token is missing or wrong |
| 403 | `forbidden` | the token is right but the caller may not do this: a caller's token on a route but a run, the service's own token on a run, or a run the node does not let the caller make; the error names why |
| 404 | `no-route` | no route answers this method and path |
| 404 | `not-found` | nothing stands at the key the path names: no node, hub or machine of that name or id |
| 409 | `conflict` | the key is taken: a node of that name, or a hub at that address, is already listed |
| 410 | `gone` | an event stream resumed from before the window the service keeps |
| 422 | `name-rule` | a node's name is not lowercase letters, digits and hyphens of at most 32 characters |
| 422 | `bad-field` | a field is out of range, of the other connection type, or names nothing listed; the error names it |
| 422 | `incomplete` | the node's row lacks a field its connection type needs |
| 422 | `not-a-hub-node` | a run names an ssh node, which is reached over its own login |
| 500 | `failed` | the service failed; the error is the failure as a sentence |
| 502 | `upstream-failed` | what a run needed did not answer: the node's hub, its agent, or the owners' read; the error names which and why |

## The clients and the harness plugin

The service publishes its own clients and its harness plugin, and a caller uses them rather than writing
its own.

- **`docs/openapi.json`** is the contract in OpenAPI 3.1: every route below with its parameters, its body,
  its answer and the codes it can answer, every body a JSON Schema, the events under `x-events`, the
  harness plugin under `x-harness` and its tools under `x-tools`. A body is described as its sender writes
  it, so a field the sender may leave out is not required, and a nullable field is required and may be
  null. It is generated from the service's own types, and a test fails where the committed document
  differs from what the code generates; `cargo run -p bmo-node-plugin --example openapi >
  docs/openapi.json` writes it again.
- **`bmo-node-client`** (`crates/client`), the Rust crate, holds every body of this document as a type and
  calls every route and reads the event stream.
- **`bmo-node-client`** (`packages/node-client`), the TypeScript package, is generated from the same
  document: its types by `openapi-typescript`, its calls by `openapi-fetch`. `bun run generate` writes the
  types again, and `bun run check` fails where they differ from the document.
- **The harness plugin, `bmo-node`** (`crates/plugin`): the MCP server `bmo-node mcp` starts on standard
  input and output (「The command」), and the `bmo-node` skill beside it that tells a harness when and how
  to use it. Each tool is one call of one route through the Rust client:

  | Tool | Route |
  |---|---|
  | `node_nodes` | `GET /nodes` |
  | `node_node` | `GET /nodes/{nodeName}` |
  | `node_target` | `GET /nodes/{nodeName}/target` |
  | `node_check` | `POST /node-checks` |
  | `node_machines` | `GET /machines` |
  | `node_machine` | `GET /machines/{machineName}` |
  | `node_readings` | `GET /machines/{machineName}/readings` |
  | `node_memory` | `GET /machines/{machineName}/memory` |

  A harness asks `node_target` how to reach a node before it logs in, and `node_machine` how loaded a
  machine is before it starts heavy work there. A tool's input is the route's path and query parameters by
  name, and `body` for a route that takes one; a refusal comes back as a tool error whose text is the
  status, `code` and `error`.
- **The hosted tool, `node-exec`** (`tools/node-exec`, `docs/tools.md`): the tool host's entry `bmo/node_exec`,
  which runs one command on a hub node through its hub for a caller whose account owns the node, or that holds
  the administrator's grant, and answers its exit code and output. It is not among the plugin's tools, which know
  no caller, and neither is a run (「Runs」), which a caller makes as itself with a token of its own.
- This document is the canonical statement: where the document, the code and the clients disagree, the
  code is repaired and both clients generated again.

## Nodes

| Method and path | Sends | Answers |
|---|---|---|
| `GET /nodes` | query `after?`, the name of the last node read, and `limit?` | a page of nodes in the list's order; an `after` naming no listed node is `bad-request` |
| `POST /nodes` | `name`, `host`, `user`, `port?`, `keyPath?`, `shell?`, `label?`, `role?` | 201 and the new ssh node; it goes last |
| `GET /nodes/{nodeName}` | | the node |
| `PATCH /nodes/{nodeName}` | any of `label`, `role`, `host`, `user`, `port`, `keyPath`, `shell`; null clears `keyPath` and `role` | the node; a changed address, port, account, key or shell clears the last check |
| `DELETE /nodes/{nodeName}` | query `leave?`, `true` to send a hub node's agent a leave first | 204; a leave the hub refuses or cannot be sent is `conflict` with the hub's reason, and nothing is removed |
| `GET /nodes/{nodeName}/target` | | how to reach the node (below) |
| `POST /nodes/{nodeName}/runs` | `account`, `command`, `shell?`, `cwd?`, `env?`, `distro?` | one command run on the node for the caller, answered with its end (「Runs」) |

A node holds no credential, only where one is kept:

| Field | What it is |
|---|---|
| `name`, `label` | the node's name and label |
| `kind` | `ssh` or `hub` (`docs/nodes.md`, 「What a node is」) |
| `hubId?`, `agentName?` | a hub node's hub, and the name the hub knows the node's agent by |
| `address` | where it is reached: `user@host`, `:port` after it where the port is not 22 and ` (PowerShell)` where the login answers in PowerShell; `agent @ hub address` for a hub node |
| `credential?` | where the credential is kept: an ssh node's key path, or the token file of a hub node's hub |
| `role?` | the services the node is reserved for, joined by `+` (`docs/nodes.md`, 「A node's role」) |
| `callers` | the callers the node lets make a run on it, by name, none where it lets none (`docs/nodes.md`, 「A node's callers」) |
| `state` | the last check's state: `connected`, `disconnected`, `auth`, `hostkey`, `unreachable`, `error` or `unknown` (`docs/nodes.md`, 「The states」) |
| `tone` | `good`, `bad` or `unknown`, as the policy's node states give it |
| `detail?`, `checkedAt?`, `connectedAt?` | the last check's line, when it was made, and when the node was last found connected |
| `up` | the last check found the node connected and was made within the link window (`docs/nodes.md`, 「A node's link」) |
| `thisMachine` | an ssh node whose host names this machine (`docs/nodes.md`, 「This machine」) |

A caller words each state and tone itself; the service hands the codes.

**The target** is the one form a node is reached by, whatever reaches it: `nodeName`, `kind` and `role?`,
then

- for an ssh node: `host`, `port`, `user`, `keyPath?` and `shell` (`posix` or `powershell`), with
  `sshOptions`, the OpenSSH options of a batch login, each given after `-o`, the options a key brings among
  them where the node names a key, the same for `ssh`, `sftp` and `scp` alike; and `sshArgs`, the same
  login laid out for `ssh` ahead of the command: each option after `-o`, `-i` and the key with its home
  expanded where the node names one, `-p` and the port, and `user@host` last. `keyPath` is as the row holds
  it, a leading `~/` meaning the home of the account that reads it;
- for a hub node: `hubId`, `hubAddress`, `hubTokenFile`, the file the hub's control token is kept in, and
  `agentName`; `sshOptions` and `sshArgs` are empty.

The target names where the credential is kept and never the credential. A node whose row lacks a field its
connection type needs is refused `incomplete`.

A hub node is never registered by a route: it is added when its agent attaches to a listed hub
(`docs/nodes.md`). A hub node takes `label` and `role` alone. A node's `callers` are written by the command
alone, never by a route, so no caller lets itself onto a node (`docs/nodes.md`, 「A node's callers」).

### Runs

A run is one command a caller runs on a hub node through the node's hub, for a person whose own node it is,
answered with the command's end. It is a computation that keeps nothing: the service records no run.

- **The caller is named by its own token,** one of the callers the configuration lists (「Configuration」). The
  service's own token names no caller and is refused here `forbidden`; a token neither is `unauthorized`.
- **The body:** `account`, the person the caller acts for, as the account's id written out; `command`, the
  command's text; and, as the hosted tool takes them, `shell?`, `powershell`, `wsl` or `linux`, the shell the
  node's host takes a command in where none is named; `cwd?`, the folder it runs in; `env?`, variables laid over
  the agent's environment; and `distro?`, the WSL distribution a `wsl` command runs in.
- **What is checked, in this order,** before the hub is asked anything:
  1. a node no row holds is `not-found`;
  2. a node whose `callers` do not name the caller is `forbidden`, the error naming the node and the caller;
  3. an ssh node is `not-a-hub-node`: it is reached over its own login with its target's `sshArgs`;
  4. the node's owners are read (`docs/nodes.md`, 「A person's node」), and a node whose owners do not hold
     `account` is `forbidden`, the error naming the node, its owners or that it has none, and the account. An
     owners' read that is not configured lets no run through and is `forbidden` saying so; one that does not
     answer, or answers something that does not read, is `upstream-failed`.
- **The answer** is 200 and the run as the hosted tool answers it (`docs/tools.md`, 「A call」): `node`, the
  `shell` it ran in, `exitCode`, null where the hub reports none, `signal`, the signal that ended it, or null,
  and `stdout` and `stderr` as text, each its last 64 KiB, with `stdoutCut` and `stderrCut` saying where more
  came. A command that ran and ended with any code is answered, not refused.
- **A node out of reach is `upstream-failed`,** told apart as the tool tells it: the hub refusing the token, the
  agent not attached, the hub not answering, the token file unreadable, the hub ending the run before the
  command's end and its stream breaking off. Output that came before a break is said in the error with it.
- **A run is held to the hub's own ceilings** on its time and its output, and the service adds none: the call
  waits for the hub's stream to end. A caller whose work runs longer starts it on the node as a unit of its own
  and reads it back with later runs.

## Node checks

| Method and path | Sends | Answers |
|---|---|---|
| `POST /node-checks` | `nodeNames?`, the nodes to check; every node where it is empty or left out | 201 and `items`, each node checked now as `GET /nodes/{nodeName}` answers it after its check; asked for every node, one whose check from a pass is still out is left to that check and answered as its row stands (`docs/nodes.md`, 「How a node's state is known」); a name no node holds is `bad-field` |

A check keeps nothing of its own: each node's `state`, `detail` and `checkedAt` carry what it came to.

## Hubs

| Method and path | Sends | Answers |
|---|---|---|
| `GET /hubs` | | a page of every hub, in the order they were listed |
| `POST /hubs` | `address`, `tokenFile`, `label?` | 201 and the hub |
| `DELETE /hubs/{hubId}` | | 204; every hub node reached through it is removed with it |

A hub is `id`, `address`, its control address, `tokenFile`, the file its control token is kept in,
`label?` and `createdAt`.

## Machines

| Method and path | Sends | Answers |
|---|---|---|
| `GET /machines` | | a page of every machine the hardware monitor knows: this machine, then every node in the list's order, each with the recorder's latest reading |
| `GET /machines/{machineName}` | | the machine, with its latest reading |
| `PATCH /machines/{machineName}` | any of `tracking` and `cpuLimit`; null clears `cpuLimit` | the machine; the recorder follows a tracking switch at once, and a limit is above 0 °C and below 127 °C |
| `GET /machines/{machineName}/readings` | query `after?`, `before?`, `limit?`, `spanMs?` | a page of the machine's past readings (below) |
| `GET /machines/{machineName}/memory` | query `after?`, `before?`, `limit?` | a page of the machine's memory samples, who held its memory and when (below) |

A machine:

| Field | What it is |
|---|---|
| `name`, `label` | the machine's name and how it is shown |
| `kind` | `local`, `ssh` or `hub` |
| `tracking` | whether the machine's live tracking is on; a machine not tracked has neither a reading nor a failure |
| `cpuLimit?` | the CPU limit set for the machine, in °C |
| `readAt?` | when the reading or the failure came; null where the machine is not tracked |
| `hardware?` | the reading as the monitor draws it: `host?`, `kernel?`, `os?`, `hostOs?`, `wsl`, `sensorLibrary?`, `uptimeSeconds?`, `load?`, and one group per piece of hardware, `cpu`, `memory`, `gpus`, `storage`, `board` and `services` (`docs/hardware.md`); null where the machine was not read |
| `failure?` | what was met where the machine could not be read |
| `standing?` | this machine as the minute look last read it (below); null for any other machine |

`GET /machines/{machineName}` reads a tracked machine itself where the recorder has nothing fresh of it
(`docs/hardware.md`, 「How often」); `GET /machines` answers what the recorder holds and reads no machine.

A figure that could not be read is null, and a CPU power the account may not read is the code `denied`; a
caller words both.

**The standing** (`docs/machine.md`):

- `reading?`: `at`, `load1`, `threads`, `memTotalGb`, `memAvailableGb`, `swapTotalGb`, `swapFreeGb`,
  `diskFreeGb?`, and `memory?` and `cpu?`, each pressure's `some` and `full?` lines with their `avg10`,
  `avg60` and `avg300` in percent; null where the machine could not be read.
- `alerts`: each pressure that stands now, by `cause`: `memory` with `fullPct`, the memory `full` over the
  last minute, `alertPct`, the alert line, and `minutes` it has stood; `disk` with `freeGb`, the data
  root's disk's free space, and `limitGb`, the line.
- `browsers`: how many browser main processes run.
- `scopes`: what each scope holds, largest first: `scope`, `megabytes` and `browsers`.
- `largest`: the five largest processes: `pid`, `name`, `megabytes` and `scope?`.

**A reading** is one stretch of a machine's recorded figures: `machineName`, `at`, the stretch's start,
`spanMs`, its width, and one group per piece of hardware, each figure a band of its `min`, `max` and `mean`
over the stretch, left out where nothing was recorded:

- `cpu`: `usePct?`, `celsius?`, `watts?` and `mhz?`;
- `memory`: `usedGb?`, `availableGb?`, `shmemGb?` and `swapUsedGb?`;
- `gpus`: per GPU, `index` with `usePct?`, `celsius?`, `watts?`, `mhz?` and `vramUsedGb?`;
- `storage`: per device, `key` with `usedGb?` and `celsius?`;
- `fans`: per fan, `chip` and `label` with `rpm?`.

The readings route answers the stretches of `spanMs` that start after `after` and before `before`, oldest
first, at most `limit` of them; a stretch nothing was recorded in is left out. `spanMs` is 1000 where none
is given. A span under a minute is a whole number of seconds and is read out of the live record, which
reaches fifteen minutes back; a span of a minute or more is a whole number of minutes, read out of the
minutes' rows, which reach two days back, and one of a whole number of half hours out of the half hours'
rows, which reach thirty-one days back (`docs/hardware.md`, 「History」). `after` defaults to the reach and
is held to it; `before` defaults to now. Any other span is `bad-request`.

**A memory sample** is one look at who held a machine's memory (`docs/hardware.md`, 「Who holds the memory」):

| Field | What it is |
|---|---|
| `machineName`, `at` | the machine, and the second the look was taken at |
| `availableGb?`, `shmemGb?`, `swapUsedGb?` | the machine's `MemAvailable`, `Shmem` and swap used at that reading; null where not read |
| `processes` | the largest processes by resident memory and swap together: `pid`, `name`, `unit?`, the unit its control group names, `rssMb`, `shmemMb`, its `RssShmem`, and `swapMb` |
| `units` | the largest service and scope control groups by memory and swap together: `unit`, `currentMb`, `anonMb`, `shmemMb`, `fileMb` and `swapMb` |

The memory route answers the samples taken after `after` and before `before`, oldest first, at most `limit` of them,
500 where none is given: every five seconds over the last fifteen minutes, and before that each minute's sample of the
lowest `MemAvailable`, reaching seven days back. `after` defaults to the reach and is held to it; `before` defaults to
now. A machine read by no such look answers an empty page. The next page is asked with the last item's `at`.

## Events

- **`GET /events`** is the one stream, as server-sent events. Each event's SSE `event` is its kind, its
  `id` the service's event number, a signed 64-bit integer that only grows, and its data `{"id", "kind",
  "at", "data"}`, the inner `data` being the member as a GET answers it, or its key alone for `deleted`.
- **A reader resumes** with `Last-Event-ID` or `after`, the number of the last event it read, and narrows
  with `kinds`, a comma-separated list, and `machineName`, which keeps the `machine.changed` and
  `reading.created` events of that machine alone. A stream opened with neither starts at the moment it is
  opened.
- **The window** is the last 4096 events, held in the service's memory. A resume from before it, or from a
  number an earlier run of the service gave, is answered 410 `gone`, and the reader reads the collections
  again.
- **A reader asking `Accept: application/json`** is answered the same events as a page, by `after` and
  `limit`, for a caller that polls.
- **A comment line every fifteen seconds** keeps the stream open.

| Kind | Data | Published when |
|---|---|---|
| `node.created` | the node | a node is registered, or a hub's agent joins the list |
| `node.changed` | the node | a node's fields or its check's state, tone, detail or link move; a check that moves nothing but its times publishes nothing |
| `node.deleted` | `name` | a node is removed, by a route, with its hub, or as an agent its hub names departed |
| `machine.changed` | the machine | its tracking or CPU limit is changed, or this machine's alerts move (`docs/machine.md`) |
| `reading.created` | a reading, `spanMs` 1000 | the recorder takes a reading of a tracked machine, once a second |

A change made by the command, which writes the store itself, is published by the service's next pass.

## Health

- **`GET /health`** answers `ready`, `reason`, null when ready and otherwise why not, and `version`: ready
  once the store is open and migrated and the loops have started.
- **`bmo-node --config <file> health`** asks the service the configuration names, waits for its answer,
  and ends 0 where it answers ready; a service not listening refuses the connection and ends it at once.
  The deployment waits on it.

## Stopping

The service stops on SIGTERM, which a deployment or systemd sends when its unit is stopped, and on SIGINT
from a terminal. It drains before it ends:

- **It stops accepting at once.** The listening socket closes, so a caller connecting after the signal is
  refused and asks the next run.
- **Every event stream ends,** so no reader holds the stop open. The reader connects to the next run, where
  a resume from this run's numbers is answered 410 `gone` and it reads the collections again (「Events」).
- **A call under way is given the drain to finish,** thirty seconds unless the configuration sets another,
  and answers as it would have, with whatever it wrote kept. A connection with no call under way closes. A
  call still running when the drain is out is cut as the process ends, having written nothing more: its
  caller sees the connection drop and asks the next run.
- **The loops end with the process.** A node check pass, a recorder run or a machine look cut midway writes
  nothing more, and the next run starts each loop's first pass at once.
- **The signal reaches the service alone.** The unit sends SIGTERM to the service's own process and not to
  the logins and probes it started, so a call under way is not cut by its own login ending under it;
  whatever the service started that still runs when it ends is killed with it.

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
| scope variable | `scope_variable` | the environment variable a process's scope is read from (`docs/machine.md`, 「What is read」); where none is set, no process has a scope |
| drain | `drain_ms` | how long a call under way is given to finish once the service is told to stop, in milliseconds (「Stopping」); where none is set, thirty seconds |
| callers | `[callers.<name>]` | one table for each caller with a token of its own, keyed by its name, lowercase letters, digits and hyphens of at most 32: `token_file`, the file holding the caller's bearer token, its surrounding whitespace dropped; where none is set, no caller makes a run |
| owners' read | `[owners]` | where a node's owners are read for a run (`docs/nodes.md`, 「A person's node」): `url`, an address in which `{nodeName}` stands for the node's name and `{caller}` for the caller's, each written escaped; and `token_file`, the file holding the bearer token presented there; where none is set, no run is let through |

- **The callers' tokens are read once, at start,** and the service does not start where a file cannot be read,
  holds no token, or holds the service's own token or another caller's. A changed token is taken up by a
  restart.
- **The owners' read's token is read for each run,** so a token changed where it is kept is taken up at once,
  and it goes nowhere else: not into an answer, an event or a log line.
- A path in either may start `~/`, the home of the account the service runs as.

## Calls the node service makes

The node service names no other unit. It reaches the machines themselves, and one address its configuration
gives:

- **an ssh node** through OpenSSH, with the policy's options (`docs/nodes.md`);
- **a hub,** an outside program an agent attaches to, over its HTTP control address with the token read
  from the hub's token file for each request: `GET /status` for the agents attached and departed, and
  `POST /exec?agent=<agent>` to run a command on an agent's machine, a run's among them;
- **the owners' read,** for a run alone: a `GET` of the address `[owners]` names, with its token, answered by a
  JSON object whose `owners` lists the accounts that own the node, each a number or a string, none for a node
  no one owns (`docs/nodes.md`, 「A person's node」). Deployed, it is agency's
  `GET /nodes/{nodeName}/access?agent={caller}` on its control port, with its control token.

The hosted tool is no part of the service and makes calls of its own (`docs/tools.md`): the service's
`GET /nodes/{nodeName}/target`, a hub's `POST /exec?agent=<agent>`, and agency's `GET /supervisors/{supervisorName}`,
`GET /role-agents` and `GET /person-nodes` to know whose node it is and whose the caller is.

## The command

`bmo-node` followed by:

- `--config <file>`: the configuration every step but `nouns` and `mcp` needs. It may stand before or
  after the step's words, since `bmo` hands a noun's words on after the noun.
- `nouns`: prints the nouns the command carries for `bmo` to dispatch, one a line, each marked `home`
  after a tab since both act on the node list the service keeps. It is asked with no configuration and
  no standard input.
- `serve`: runs the migrations, then the routes, the check pass, the recorder and the machine look;
- `health`: asks the service the configuration names whether it is ready;
- `node` with the verbs of `docs/nodes.md`, 「Registering and changing a node」, against the same store;
  `--target <name>` prints the node's target as `GET /nodes/{nodeName}/target` answers it;
- `hardware-setup [<node>…]`: the Windows sensor setup (`docs/hardware.md`, 「The Windows side」);
- `mcp`: the harness plugin's MCP server on standard input and output, calling the service at `--url`
  with the token `--token` or the `BMO_NODE_TOKEN` environment variable gives; with `--config <file>` and
  neither, the address and the token that configuration names.

### The nouns `bmo` serves

Each noun runs where the service stands, and against the store the configuration names rather than through
the service's routes. Where a route makes the same change, the table names it.

| Noun | Takes | Route making the same change |
|---|---|---|
| `node` | the verbs and arguments of `docs/nodes.md`, 「Registering and changing a node」 | `GET /nodes`, `POST /nodes`, `PATCH /nodes/{nodeName}`, `DELETE /nodes/{nodeName}`, `GET /nodes/{nodeName}/target`, `POST /hubs`, `DELETE /hubs/{hubId}` and `POST /node-checks` |
| `hardware-setup` | the nodes to set up, or every node | none: the Windows sensor setup runs over each node's own road |

Through `bmo`, the same commands are typed as `bmo node …` and `bmo hardware-setup …`.

## Versions

The repository's version is the workspace's, and both clients and the plugin carry it. It follows bmo-chat's
`docs/conventions.md`, 「Versions」: it moves only at a release the owner declares, and until the first release it is
`0.1.0` with a revision, `0.1.0-rev.<n>`. A commit, a deployment or a change to this document moves no version.
