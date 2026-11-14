# The hosted tool

## What it is

- **The node service provides one tool to the tool host, `node-exec`,** a folder under `tools/` with its
  `tool.toml`, the schemas its entry names, its manual and the Rust crate of its executable, a member of the
  workspace. Its one entry, **`bmo/node_exec`**, runs one command on a hub node through the node's jake hub and
  answers the command's exit code and output.
- **It is declared in the tool host's manifest format,** protocol 1, a process per call: the host hands the
  executable the call and the session record on its standard input and reads one tool result on its standard
  output (bmo-pocket's `docs/api.md`, 「The call protocol」).
- **It is placed and registered by this repository's deployment:** the release's executable is copied with the
  manifest, the schemas and the manual into the tool's `.placed/` folder before the flip, and registered as this
  service's own with `bmo pocket put … --placed --service bmo-node --allow room.tools` once the service answers
  ready. Both steps stand in `testbed.toml`.
- **A session sees the entry where its record holds `room.tools`,** the chat line's capability, so a supervisor
  reaches it, by `bmo use bmo/node_exec` where it is not shown by name. The entry declares no grant of the host's:
  whom it serves is decided per node by the tool itself (「Who may run a command on which node」).
- **The tool is a session's road; a unit that is not a session makes a run** (`docs/api.md`, 「Runs」). Both
  aim the hub's exec at the node's agent the same way and answer a command's end in the same shape, while
  each judges its own caller: the tool a session by its record, a run a caller by its own token, the node's
  `callers` and the node's owners.
- **The harness plugin carries no such tool.** Its MCP server calls the service with the service's own token and
  knows no caller, so a command on a node is reached through the tool host alone, where the caller's record is
  known.

## A call

- **The arguments:** the node, named either by `node`, its name, or by `account`, the account whose node it is,
  one of the two and never both; `command`, the command's text; and, as jake's console takes them for its exec,
  `shell`, `powershell`, `wsl` or `linux`, `cwd`, the folder it runs in, `env`, variables laid over the agent's
  environment, and `distro`, the WSL distribution a `wsl` command runs in. A call naming no shell runs the command
  in the shell the node's host takes a command in: `powershell` on a Windows host, `linux` otherwise
  (`docs/nodes.md`, 「What a row holds」).
- **A call that names the node by its account** has the node read first from agency's records of a person's node,
  the same `GET /person-nodes` the caller's check reads (「Who may run a command on which node」): the node the
  record for that account names, an account's id compared as it is written out, so `4` and `"4"` name the same
  account. An account no record names is refused as one with no node, naming the account; where agency cannot
  be asked, or answers something that does not read, the call is refused saying so. A call naming both `node`
  and `account`, or neither, is refused saying the tool takes one of them. The node so read then stands for the
  rest of the call exactly as a node named by `node` does, and the answer's `node` names it.
- **The caller is checked next,** for the node named or read, before the node service or the hub is asked anything
  (「Who may run a command on which node」): naming a node by its account reaches no node the caller could not
  reach by its name.
- **The node is read through the node service,** as `GET /nodes/{nodeName}/target` answers it, at the address and
  with the token of the service's configuration under the data root, `env/node.toml`. A node no row holds is
  refused naming it, and an ssh node is refused as one this tool does not reach: it is reached over its own login
  with its target's `sshArgs`.
- **The command goes to the hub's `POST /exec?agent=<agent>`,** the node's agent named, with the shell, the
  command, the folder, the variables and the distribution as the call gives them. The hub's control token is read
  by the tool out of the hub's token file for that one request and goes nowhere else: not into the answer, a log
  line or anything the session's shell is handed.
- **The answer** is the command's `exitCode`, null where the hub reports none, `signal`, the signal that ended it,
  or null, and its `stdout` and `stderr` as text, with `node` and the `shell` it ran in, both as a person's
  reading and as structured content. Each stream keeps its last 64 KiB; `stdoutCut` and `stderrCut` say where
  more came. A command that ran and ended with any code is answered, not refused: its code says how it went.
- **A command is held to the hub's own ceilings** on its time and its output, and the tool adds none: a call
  waits for the hub's stream to end. Work longer than the hub's ceiling is started on the node as a unit of its
  own and read back by later commands.
- **What is told apart as the node out of reach,** answered as a failed call with why: the hub refusing the token,
  the agent not attached, the hub not answering, the token file unreadable, the hub ending the run before the
  command's end (the agent dropped, the hub stopped), and the hub's stream breaking off. Output that came before
  a break is said with it, and so is that the command's own end is not known.

## Who may run a command on which node

- **A caller reaches the nodes its own account owns, and an administrator reaches every node.** Anything else is
  refused before the node is reached, with a reason naming the node and its owner.
- **The administrator** is a session whose record's `grants` hold the resource `node.admin`, the administrator's
  grant. The record's issuer writes it; the tool reads it and asks nobody else.
- **The caller's account** is read from agency, the issuer of the session records, since a record carries no
  account of its own:
  - the caller is the agent the record is held by, `holderName`, or the supervisor a double stands for where its
    labels name one as `doubleOf`;
  - a supervisor's account is its `ownerAccount` on agency's `GET /supervisors/{supervisorName}`, and a role
    agent's its `ownerAccount` among agency's `GET /role-agents`;
  - an agent agency knows as neither, or one with no `ownerAccount`, belongs to no account and owns no node.
- **The node's owner** is the account agency's record of a person's node names for it, read on agency's
  `GET /person-nodes` (agency's `docs/person-nodes.md`, 「Which node is a person's」). A node no record names is no
  person's node and is reached by the administrator alone.
- **A caller is let through where its account is the node's owner,** compared as the account's id written out.
- **Agency is reached on its control port** with its control token, both read from the data root's
  `env/agency.env` (`AGENCY_CONTROL_PORT`, and `AGENCY_CONTROL_TOKEN` or the file `AGENCY_CONTROL_TOKEN_FILE`
  names), a variable the tool's own environment holds standing first. Where agency cannot be asked, or answers
  something that does not read, the call is refused saying so: an owner that cannot be read lets nobody through
  but the administrator.
- **Where the data root stands** is the record's `BMO_DATA_DIR` variable, handed to the call's environment; a
  call whose environment names none is refused.
