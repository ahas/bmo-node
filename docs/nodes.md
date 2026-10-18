# Nodes

The machines reachable from here, listed in one place. Every other unit reads the list through this
service's API (`docs/api.md`); nothing else keeps a list of its own.

## What a node is

- **A node is one machine a person can reach from here, and every node is the same kind of thing.**
  What differs is its connection type, `kind`, a property of the node and nothing more:
  - **`ssh`**: a machine logged into over SSH, with an address, a port, an account and the place of the
    key the login offers;
  - **`jake`**: a machine running a jake agent, reached through the jake hub the agent is attached to.
    One jake node is one agent.
- **This machine is a node too where its own card carries a service:** an ssh node on its own loopback,
  so a service's installer reaches its card the way it reaches any other machine's.
- **A jake hub is not a node.** It is the backend a jake node is reached through, a control address and
  the file its token is kept in, and it is never listed, counted or monitored as a machine.
- **The list and the hubs are two tables of node's store,** `nodes` and `hubs` (`docs/store.md`).
- **A jake agent joins the list by attaching to a hub.** Every pass asks each hub which agents are
  attached, and an agent no jake node stands for yet is added as one, last in the list, named after the
  agent and labelled with the agent's own name. A jake node whose agent its hub says has departed leaves
  the list on the next pass, since the machine is gone rather than down. One whose agent merely dropped
  stays as `disconnected` until it attaches again or a person removes it; one removed while its agent is
  still attached comes back on the next pass.

## What a row holds

- `name`: lowercase letters, digits and hyphens, at most 32 characters. It never changes; the label does.
  A jake node's name is its agent's name with every other character made a hyphen, with `-2`, `-3`
  added where the name is already taken.
- `label`: what a person reads.
- `kind`: `ssh` or `jake`.
- For an ssh node: `host`, `port` (22 when not given), `user`, `key_path`, the private key the login
  offers, and `shell`, the language the login answers a command in: `posix` when not given, or
  `powershell` for a Windows machine whose OpenSSH logs into Windows PowerShell (`docs/hardware.md`, 「A
  Windows machine」). A node with no key path offers whatever the account's own SSH configuration
  offers.
- For a jake node: `agent`, the agent's name exactly as the hub knows it, and `hub_url`, the hub it is
  reached through. The token file is the hub's.
- A hub: `url`, its control address, and `token_file`, the environment file whose `JAKE_CONTROL_TOKEN`
  line holds the control token.
- **A credential is never in either table.** A row names where a key or a token is kept, never the key or
  the token; the token is read out of its file for the one request that needs it and goes nowhere else:
  not into a row, a log line, an event or an answer.
- `role`: the services the node is reserved for, joined by `+`, or none (「A node's role」).
- `position`: the order the list is shown in. A new node goes last.
- The last check: `state`, `detail` (one line saying what the check met), `checked_at`, and
  `connected_at`, the last moment the node was found connected, kept when a later check fails.

## A node's role

- **A node reserved for services carries those services alone.** Its `role` names the service, or several
  joined by `+`; a node with none is shared by whatever the services' own placements put there.
- **Each service names itself,** a word of lowercase letters, digits and hyphens, and nothing here keeps a
  list of them. The one word this service reads is `jobs`: a node whose role is none or names `jobs` takes
  a worker's checkout and heavy runs (`takesJobs`, `docs/api.md`).
- **The role is written by whatever places services on nodes,** through the registry's set, and read by
  every installer and placement that must refuse a node reserved for another service.

## How a node's state is known

- **A check is made in one place, the `node-check` crate:** the service's pass, its check route and the
  command's check call it as a library, and each writes what it answers to the node's row.
  - Its binary takes its input as JSON on its standard input and answers JSON: `node-check hubs` asks
    every hub it is handed which agents are attached; `node-check nodes` checks every row it is handed,
    reading a jake node off the hub answers handed with it.
  - How OpenSSH is started for a node and how its end is read are the policy's `ssh.toml`, so the check,
    the hardware monitor and the login arguments handed to agency log in the same way.
- **When a check runs:**
  - the service checks every node when it starts and once a minute after, all nodes at once, and writes
    each check to the node's row;
  - `POST /nodes/check` and `bmo-node node --check` make the same check at once.
  - A pass publishes `nodes-changed` when a check moved a node's state or the list itself changed.
- **An ssh node is checked by one login that runs nothing.** OpenSSH starts in batch mode, asking nobody
  for anything, with a five-second connection limit, the node's key alone where it names one, and a host
  key accepted the first time and held to after. A login that has not ended after twelve seconds is
  stopped. What it met decides the state:
  - it logged in: `connected`;
  - the machine answered and refused the login: `auth`;
  - the host key is not the one held for that address: `hostkey`;
  - nothing answered, the connection was refused, the name did not resolve, or the login did not end in
    time: `unreachable`;
  - anything else: `error`, with OpenSSH's own last line as the detail.
  - A key the row names that is not there is `error` without a login.
- **A jake node is checked by asking its hub,** once per hub for every node on it: `GET /status` on the
  control address with the token, whose `agents` list names every agent attached and whose `departed`
  list every agent uninstalled.
  - the node's agent is in the list: `connected`;
  - the hub names the agent departed: `disconnected`, said as uninstalled, until the next whole pass
    removes the node;
  - the agent is in neither list: `disconnected`;
  - the hub refuses the token: `auth`, for every node on it;
  - the hub does not answer, or the token file cannot be read: `unreachable` or `error`, for every node
    on it.
- **A whole pass adds the agents it finds and removes the departed** before the rows are checked. A check
  of one node asks only what that node needs.
- A node never checked is `unknown`. A node removed while its check was out is not written back.

## The states

`connected`, `disconnected`, `auth`, `hostkey`, `unreachable`, `error` and `unknown`. The policy's
`node-states.toml` gives each its tone at a glance: `good` for `connected`, `unknown` for `unknown`, and
`bad` for every other. The service hands the state and its tone; the console words them.

## A node's link

- **`/links/state` answers whether a node is reachable now,** from the node's own checks: `up` where its
  last check found it `connected` and was made within the link window, three minutes, which covers two
  passes and the check's own wait. A node not listed, never checked, or last checked longer ago is not
  up.
- The tunnel agency holds open to a work node is agency's own and is read on agency's side.

## This machine

- **Whether a node is this machine is decided here:** an ssh node whose host is a loopback address,
  `localhost`, this machine's host name or one of its own interface addresses. A jake node never is,
  since its hub's address says nothing of where its agent runs.
- A loopback address is taken at its word, so a node reached through a tunnel ending on this machine's
  loopback is registered by an address of its own instead.

## Registering and changing a node

The command is `bmo-node --config <file> node`, and the registry routes do the same (`docs/api.md`).

- `node` alone lists the nodes with their state and when each was last connected, and the hubs after
  them.
- `--add <name> --ssh <user@host> [--port <n>] [--key <path>] [--shell posix|powershell] [--label
  <label>] [--role <service>]` registers an ssh node.
- A jake node is never registered by hand: it is added when its agent attaches to a listed hub.
- `--add-hub <control url> --token-file <path>` lists a hub; `--remove-hub <control url>` removes it with
  every jake node reached through it.
- `--set <name>` with any of `--label`, `--role`, `--ssh`, `--port`, `--key`, `--shell` changes those
  fields; `--key none` clears the key path and `--role none` the role. A jake node takes `--label` and
  `--role` alone. A changed address, port, account, key or shell clears the last check, since it was a
  check of something else.
- `--remove <name>` removes a node.
- `--check [<name>]` checks every node, or one, now.
- `--target <name>` prints the one node's connection as JSON, for a tool outside the service that reaches
  a node by its registered name: its connection type with the address, port, account, key path and shell
  of an ssh node, or the agent, hub and token file of a jake node, and its role. A credential is never in
  it; a node missing a field its type needs is refused.
- A name already taken, a name the rule refuses, a port outside 1 to 65535, a field of the other
  connection type or a hub already listed is refused, and nothing is written.
- Every change publishes `nodes-changed`.

## Reaching a node

- **The service reaches a node in two ways and no others:** the check's own login or hub status
  (「How a node's state is known」), and the hardware monitor's road, which runs a script on the node and
  hands its output on as it comes (`docs/hardware.md`, 「Which machines」). The Windows sensor setup runs
  over that same road.
- **A service kept on a node is built, fed and asked over the node's own login by the unit that keeps it,**
  with the login's arguments from `/nodes/login` and the node's reach from `GET /nodes/{name}/reach`
  (`docs/api.md`). The node service carries no general shell for other units and no file transfer.
- **A node that cannot be reached is told apart from a script that failed:** a login that never happens, a
  hub that refuses or an agent that is gone is the node out of reach; a script that ran is its exit code.
