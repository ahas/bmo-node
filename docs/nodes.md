# Nodes

The machines reachable from here, listed in one place. Every other unit reads the list through this
service's API (`docs/api.md`); nothing else keeps a list of its own.

## What a node is

- **A node is one machine a person can reach from here, and every node is the same kind of thing.**
  What differs is its connection type, `kind`, a property of the node and nothing more:
  - **`ssh`**: a machine logged into over SSH, with an address, a port, an account and the place of the
    key the login offers;
  - **`hub`**: a machine running an agent of a jake hub, an outside program, reached through the hub the
    agent is attached to. One hub node is one agent.
- **This machine is a node too where its own card carries a service:** an ssh node on its own loopback,
  so a service's installer reaches its card the way it reaches any other machine's.
- **A hub is not a node.** It is the backend a hub node is reached through, a control address and the
  file its token is kept in, and it is never listed, counted or monitored as a machine.
- **The list and the hubs are two tables of node's store,** `nodes` and `hubs` (`docs/store.md`).
- **An agent joins the list by attaching to a hub.** Every pass asks each hub which agents are attached,
  and an agent no hub node stands for yet is added as one, last in the list, named after the agent and
  labelled with the agent's own name. A hub node whose agent its hub says has departed leaves the list on
  the next pass, since the machine is gone rather than down. One whose agent merely dropped stays as
  `disconnected` until it attaches again or a person removes it; one removed while its agent is still
  attached comes back on the next pass.

## What a row holds

- `name`: lowercase letters, digits and hyphens, at most 32 characters. It never changes; the label does.
  A hub node's name is its agent's name with every other character made a hyphen, with `-2`, `-3` added
  where the name is already taken.
- `label`: what a person reads.
- `kind`: `ssh` or `hub`.
- For an ssh node: `host`, `port` (22 when not given), `user`, `key_path`, the private key the login
  offers, and `shell`, the language the login answers a command in: `posix` when not given, or
  `powershell` for a Windows machine whose OpenSSH logs into Windows PowerShell (`docs/hardware.md`, 「A
  Windows machine」). A node with no key path offers whatever the account's own SSH configuration
  offers.
- For a hub node: `agent`, the agent's name exactly as the hub knows it, and `hub_url`, the hub it is
  reached through. The token file is the hub's. Its `shell` is the language its agent's host runs a command
  in, read off the shells the hub says the agent offers (its capabilities `exec:<shell>`) on every pass:
  `powershell` for an agent that offers PowerShell and no Linux shell — a Windows host —, `posix` otherwise.
- A hub: `id`, its key, `url`, its control address, `token_file`, the environment file whose
  `JAKE_CONTROL_TOKEN` line holds the control token, and `label`, what a person reads, or none.
- **A credential is never in either table.** A row names where a key or a token is kept, never the key or
  the token; the token is read out of its file for the one request that needs it and goes nowhere else:
  not into a row, a log line, an event or an answer.
- `ownership`: `shared` or `personal`, and `account`, the account a personal node belongs to, or none
  (「A node's ownership」).
- `role`: the services the node is reserved for, joined by `+`, or none (「A node's role」).
- `callers`: the callers the node lets make a run on it, joined by `+`, or none (「A node's callers」).
- `position`: the order the list is shown in. A new node goes last.
- The last check: `state`, `detail` (one line saying what the check met), `checked_at`, and
  `connected_at`, the last moment the node was found connected, kept when a later check fails.

## A node's ownership

- **Every node is either shared or personal.** A `shared` node is a machine of the platform, run for everyone;
  a `personal` node is one person's own PC, and names the `account` it belongs to where that is known.
- **A personal node stays in the list and is reached as any node is:** it is checked, its target is handed
  out, and runs and the hosted tool reach it as before. What it is spared is monitoring: the hardware monitor
  never reads it (`docs/hardware.md`, 「Which machines」), and a personal node that is merely off is not told as
  a failure (「The states」).
- **A hub node takes its ownership from its agent's name when it joins the list.** A person's machine is named
  by the invitation or the pairing that binds it to their account as `u<user>`, the account's id as a decimal
  number, or `u<user>-<tail>`; an agent whose name has that shape is `personal`, its account the number. Every
  other hub node, and every ssh node, is `shared`. The stores made before ownership existed are given the same
  reading of their hub nodes once.
- **The account is what a screen shows and filters by,** never who may reach the node: a run and the hosted
  tool read the node's owners afresh each time (「A person's node」).
- **A PC several accounts were declared to share** (a binding the account service keeps as shared) is still a
  person's PC, and so `personal` here: `shared` means a machine of the platform, not one shared by people.
- **A person changes it with the command or a route:** `bmo-node node --set <name> --ownership
  shared|personal [--account <id>|none]`, or `PATCH /nodes/{nodeName}` (`docs/api.md`). A node made `shared`
  loses its account, and a shared node takes none.

## A node's role

- **A node reserved for services carries those services alone.** Its `role` names the service, or several
  joined by `+`; a node with none is shared by whatever the services' own placements put there.
- **Each service names itself,** a word of lowercase letters, digits and hyphens, and nothing here keeps a
  list of them or reads any one of them: what a role means for a placement is the caller's.
- **The role is written by whatever places services on nodes,** through the registry's change, and read by
  every installer and placement that must refuse a node reserved for another service.

## A node's callers

- **A node lets a caller make a run on it only where its own `callers` name that caller** (`docs/api.md`,
  「Runs」). A node naming none, as every node does until the owner writes one, takes no caller's run.
- **A caller is a name the configuration gives a token of its own** (`docs/api.md`, 「Configuration」),
  lowercase letters, digits and hyphens: a unit of the deployment that is not a session, such as a service
  driving a program on a person's PC. Nothing here keeps a list of them or reads any one of them.
- **The owner writes it, with the command alone:** `bmo-node node --set <name> --callers <caller>[+<caller>…]`
  sets the whole list, and `--callers none` clears it. No route writes it, so a caller holding the service's
  token cannot let itself onto a node; every route and event shows it.
- **A node's callers do not pass a run on their own:** the node must also be the person's own node the run is
  made for (「A person's node」).

## A person's node

- **A run is made for one person,** named by the account it is for, and only on that person's own node: a node
  whose owners hold the account. A node another person owns, or no one owns, is refused.
- **Whose node it is, is read and not kept here.** The service asks the owners' read its configuration names
  (`docs/api.md`, 「Calls the node service makes」) for each run, and holds nothing of what it answers.
- **Deployed, the owners are agency's:** its access read answers the accounts whose record of a person's node
  names the node, and for an isolation node the account it is named for (agency's `docs/person-nodes.md`,
  「Which node is a person's」). A person's PC is their node once agency's record binds their account to it,
  set by the service that binds an account to its machine, or by the owner with `bmo node person <account>
  <node>`.
- **Where no owners' read is configured,** no node is anyone's and no run is let through.

## How a node's state is known

- **A check is made in one place, the `node-check` crate:** the service's pass, its check route and the
  command's check call it as a library, and each writes what it answers to the node's row.
  - Its binary takes its input as JSON on its standard input and answers JSON: `node-check hubs` asks
    every hub it is handed which agents are attached; `node-check nodes` checks every row it is handed,
    reading a hub node off the hub answers handed with it.
  - How OpenSSH is started for a node and how its end is read are the policy's `ssh.toml`, so the check,
    the hardware monitor and the target the service hands out log in the same way.
- **When a check runs:**
  - the service starts a pass when it starts and once a minute after, each checking every node at once,
    and writes each check to the node's row as it lands, so a slow check holds back no other node's;
  - a pass does not wait on the one before: a node or a hub whose check from an earlier pass is still out
    is left to that check, so one node is never checked twice at once and a later check never lands
    under an earlier one;
  - `POST /node-checks` and `bmo-node node --check` make the same check at once.
  - Whatever moves the list is published as the node's own event, `node.created`, `node.changed` or
    `node.deleted` (`docs/api.md`, 「Events」); a check that moves nothing but its times publishes nothing.
- **An ssh node is checked by one login that runs nothing.** OpenSSH starts in batch mode, asking nobody
  for anything, with the node's key alone where it names one, a host key accepted the first time and held
  to after, and a keep-alive every fifteen seconds that ends a login whose machine stopped answering it
  three times running. The login is waited on until it ends; a connection that cannot be made ends when
  the network says so. What it met decides the state:
  - it logged in: `connected`;
  - the machine answered and refused the login: `auth`;
  - the host key is not the one held for that address: `hostkey`;
  - nothing answered, the connection was refused or broken, or the name did not resolve: `unreachable`;
  - anything else: `error`, with OpenSSH's own last line as the detail.
  - A key the row names that is not there is `error` without a login.
- **A hub node is checked by asking its hub,** once per hub for every node on it: `GET /status` on the
  control address with the token, whose `agents` list names every agent attached and whose `departed`
  list every agent uninstalled. The request waits for the hub's answer, and the connection's keep-alive
  tells a hub gone.
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
`bad` for every other. A personal node's PC is switched off as a matter of course, so its `disconnected` and
`unreachable` are `unknown` instead, as the policy's `personal_tone` gives them; its `auth`, `hostkey` and
`error` stay `bad`. The service hands the state and its tone; a caller words them.

## A node's link

- **A node's `up` says whether it is reachable now,** from the node's own checks: true where its last
  check found it `connected` and was made within the link window, three minutes, which covers two passes.
  The window only says how fresh the reading is; it ends no check. A node never checked, or last checked
  longer ago, is not up.
- A tunnel another unit holds open to a node is that unit's own and is read on its side.

## This machine

- **Whether a node is this machine is decided here:** an ssh node whose host is a loopback address,
  `localhost`, this machine's host name or one of its own interface addresses. A hub node never is, since
  its hub's address says nothing of where its agent runs.
- A loopback address is taken at its word, so a node reached through a tunnel ending on this machine's
  loopback is registered by an address of its own instead.

## Registering and changing a node

The command is `bmo-node --config <file> node`, served through `bmo` as `bmo node …`, and the registry
routes do the same (`docs/api.md`).

- `node` alone lists the nodes with their state and when each was last connected, a personal node marked
  so with its account, and the hubs after them.
- `--add <name> --ssh <user@host> [--port <n>] [--key <path>] [--shell posix|powershell] [--label
  <label>] [--role <service>]` registers an ssh node, `shared` until it is set otherwise.
- A hub node is never registered by hand: it is added when its agent attaches to a listed hub.
- `--add-hub <control url> --token-file <path>` lists a hub; `--remove-hub <control url>` removes it with
  every hub node reached through it.
- `--set <name>` with any of `--label`, `--role`, `--callers`, `--ownership`, `--account`, `--ssh`, `--port`,
  `--key`, `--shell` changes those fields; `--key none` clears the key path, `--role none` the role,
  `--callers none` the callers and `--account none` the account. A hub node takes `--label`, `--role`,
  `--callers`, `--ownership` and `--account` alone. An account is the account's id as a decimal number, and
  is refused on a node left or made `shared` (「A node's ownership」). A changed address, port, login account, key or shell
  clears the last check, since it was a check of something else. `--callers` is this command's alone
  (「A node's callers」).
- `--remove <name>` removes a node. A hub node whose agent is still attached comes back on the next pass,
  so `DELETE /nodes/{nodeName}` with `leave=true` first sends the agent a leave through its hub — the hub's
  `POST /leave`, which has the agent uninstall itself and depart — and removes the node once the hub took
  it; a hub that refuses, as it does for an agent not attached, or cannot be reached, leaves the node listed
  and says why. Without a leave a hub node is removed alone, which holds for good only once its agent is
  gone. An ssh node takes no leave.
- `--check [<name>]` checks every node, or one, now.
- `--target <name>` prints the one node's target as JSON, as `GET /nodes/{nodeName}/target` answers it, for
  a tool outside the service that reaches a node by its registered name: its connection type with the
  address, port, account, key path, shell, OpenSSH options and login arguments of an ssh node, or the
  agent, hub and token file of a hub node, and its role. A credential is never in it; a node missing a
  field its type needs is refused.
- A name already taken, a name the rule refuses, a port outside 1 to 65535, a field of the other
  connection type or a hub already listed is refused, and nothing is written.
- A change made through a route publishes its event at once. The command writes the store itself, so it
  works whether or not the service runs; the service's next pass, which holds the list to the one it last
  published, publishes it.
- The list prints each node's state by its code and each moment in UTC.

## Reaching a node

- **The service reaches a node in three ways and no others:** the check's own login or hub status
  (「How a node's state is known」); the hardware monitor's road, which runs a script on the node and
  hands its output on as it comes (`docs/hardware.md`, 「Which machines」), over which the Windows sensor
  setup runs too; and a run, the hub's exec aimed at the node's agent for a caller (below).
- **A service kept on a node is built, fed and asked over the node's own login by the unit that keeps it,**
  with the node's target from `GET /nodes/{nodeName}/target` (`docs/api.md`), whose `sshArgs` open its
  batch login. The node service's routes carry no file transfer.
- **A unit that is not a session runs a command on a hub node through a run alone** (`docs/api.md`, 「Runs」):
  as itself, with a token of its own, on a node whose `callers` name it (「A node's callers」), for a person
  whose own node it is (「A person's node」). The hub's control token is read by the service for that one
  request and handed to no caller.
- **An agent runs a command on a hub node through the hosted tool `bmo/node_exec` alone** (`docs/tools.md`): the
  hub's exec aimed at the node's agent, with the hub's control token read by the tool and handed to no session,
  and only for a caller whose account owns the node or that holds the administrator's grant. A hub's own exec checks
  no owner, since any holder of its control token runs a command on any agent attached to it, so the tool is the
  one road an agent is given to a hub node.
- **A node that cannot be reached is told apart from a script that failed:** a login that never happens, a
  hub that refuses or an agent that is gone is the node out of reach; a script that ran is its exit code.
