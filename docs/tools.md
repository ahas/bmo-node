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
- **The tool is a session's road; a unit that is not a session makes a run** (`docs/api.md`, 「Runs」) **and
  moves one file's bytes on the node's file-transfer routes** (`docs/api.md`, 「File transfer」). The tool and
  a run aim the hub's exec at the node's agent the same way and answer a command's end in the same shape,
  while each judges its own caller: the tool a session by its record, a run and a file transfer a caller by
  its own token, the node's `callers` and the node's owners.
- **The harness plugin carries no such tool.** Its MCP server calls the service with the service's own token and
  knows no caller, so a command on a node is reached through the tool host alone, where the caller's record is
  known.

## A call

- **The arguments:** the node, named either by `node`, its name, or by `account`, the account whose node it is,
  one of the two and never both; `command`, the command's text; and, as jake's console takes them for its exec,
  `shell`, `powershell`, `wsl` or `linux`, `cwd`, the folder it runs in, `env`, variables laid over the agent's
  environment, and `distro`, the WSL distribution a `wsl` command runs in. A call naming no shell runs the command
  in the shell the node's host takes a command in: `powershell` on a Windows host, `linux` otherwise
  (`docs/nodes.md`, 「What a row holds」). `secrets` names saved secrets the command is handed without their values
  passing through the call (「Secrets handed by name」).
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
- **A shell the node's host does not run is refused, naming the node and the shells it runs:** a Windows host,
  whose target's `shell` is `powershell`, runs `powershell` and `wsl` commands, and any other host `linux` ones.
  The agent would run any other in its host's own shell, so the answer would name a shell the command never ran
  in, and a folder such as `~` would be one its host cannot enter.
- **A `wsl` command reaches bash as written.** The agent enters WSL through `wsl.exe`, which hands its command
  line to the distribution's default shell before bash takes it, so that shell would expand every `$`, quote and
  backtick in the command once, with the shell's own variables still unset. The tool sends a `wsl` command
  base64-encoded inside a line holding no character any shell expands, which decodes it and hands it whole to
  bash on standard input, in one group bash reads to its end before it runs any of it, so a command that reads
  its standard input reads nothing.
- **The command goes to the hub's `POST /exec?agent=<agent>`,** the node's agent named, with the shell, the
  command, the folder, the variables and the distribution as the call gives them, the secrets it names laid in as
  「Secrets handed by name」 says. The hub's control token is read
  by the tool out of the hub's token file for that one request and goes nowhere else: not into the answer, a log
  line or anything the session's shell is handed.
- **The answer** is the command's `exitCode`, null where the hub reports none, `signal`, the signal that ended it,
  or null, and its `stdout` and `stderr` as text, with `node` and the `shell` it ran in, both as a person's
  reading and as structured content. Each stream keeps its last 64 KiB; `stdoutCut` and `stderrCut` say where
  more came. A command that ran and ended with any code is answered, not refused: its code says how it went. A
  secret's value the command printed is masked in it (「Secrets handed by name」).
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
  - the caller is the agent the record is held by, `holderName`;
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

## Secrets handed by name

- **A call names saved secrets, never their values,** in `secrets`: each item a secret's name, or an object of
  `name`, and optionally `prefix` and `as`. The tool reads each named file on this machine and hands what it holds
  to the command as variables, as a file on the node, or both, so a value passes through neither the caller's
  text, the hub's record of the command, nor the answer.
- **Whose secrets a call reads follows how its caller was let through** (「Who may run a command on which
  node」): the administrator reads the platform's space, the data root's `env/`, and a caller let through as the
  node's owner reads its own account's space, `env/users/<account>/`, and never the platform's (bmo-web's
  `docs/accounts.md`, 「The secret store」). The secrets are read after the caller is judged and before the node
  service or the hub is asked, so a caller the node refuses learns nothing of which names stand.
- **A name is a plain file name:** no slash or backslash, no leading dot and no `.previous` ending; any other is
  refused naming it. A name its space holds no regular file under is refused naming it and the space, a link or a
  folder being no secret. A file over 16 KiB is refused, the most a variable carries on every node's shell once
  encoded, and so is a file that cannot be read. No refusal carries what a file holds.
- **What a file holds:** fields, where every line is a `KEY=VALUE` line whose key is a capital letter or an
  underscore followed by capitals, digits and underscores, no key twice, the shape the console writes a secret
  of several fields in; or else one value, the file's bytes. A field's value is the line after its first `=`,
  exactly as written: the console writes a value as typed, with no quoting, so no quoting or expansion is read
  into it.
- **How a secret is handed is its `as`:**
  - `env`: a file of fields hands each field as a variable named by its key, with `prefix` written before the key;
    a file of one value hands the value as `BMO_SECRET_<STEM>`, the stem being the name in capitals with every
    character other than a letter or a digit made an underscore, so `acme-portal` is `ACME_PORTAL`. A value
    that is not UTF-8 text is refused as a variable.
  - `file`: the file's bytes unchanged, written into a file on the node whose path the command finds in
    `BMO_SECRET_<STEM>_FILE`.
  - `both`: the two together.
  - Where `as` is left out, a file of fields is handed as `env` and a file of one value as `file`.
  - `prefix` is letters, digits and underscores, not opening on a digit.
- **A variable named twice,** by two secrets or by a secret and the call's `env`, is refused naming the variable.
- **A file on the node** is written by the command's own shell. The file's bytes travel base64-encoded in a
  variable `BMO_SECRET_BYTES_<n>`, and the command is wrapped in the shell's handing script, which names the
  variables and holds no value:
  - it makes a folder `bmo-secret-<id>`, its id random to the call, in the user's temporary folder (`$TMPDIR`, or
    `/tmp` where none is set, for `linux` and `wsl`; PowerShell's temporary path, the user's `%TEMP%` on Windows),
    readable by the user alone: made under a umask of 077 in bash, and on Windows given an access list of its own
    that holds the user alone and inherits nothing, since a user's temporary folder may grant others too, as a
    sandbox's install does. It writes each file there, once that access list stands, removes the variable that carried it, sets the path
    variable and runs the command.
  - In bash, for `linux` and `wsl`, the command runs in a subshell, so a trap it sets replaces none of the
    script's own, and on `wsl` the whole script is sent encoded as every `wsl` command is (「A call」). The folder is removed when the script exits, also on a hangup, an interrupt or a termination,
    the way the agent stops a command, and the script exits with the command's status by an `exit` of its own
    after the subshell, since bash runs a subshell that ends a script in the script's own process, dropping its
    traps.
  - In PowerShell the command runs in a `try` whose `finally` removes the folder, and the process ends with 1
    where the command's last statement failed, as PowerShell does for a command it is handed.
  - **After the command's end, however it ended, the tool sends a second command removing the folder,** since a
    kill skips the script's own removal: the agent's kill after its grace on Linux, and its forced tree kill on
    Windows on a cancel or at the hub's ceiling. Where that command does not end with code 0, the answer says the
    folder's removal could not be confirmed.
  - Every handing script first removes the user's `bmo-secret-*` folders more than a day old, so a folder left by
    a node lost in the middle of a command goes with the next call that hands a file there.
- **A value in the answer is masked:** every value handed of at least four characters, each field's value, each
  line of a file of one value, the file whole and its encoded form, is replaced in the output and in a failure's
  reason by `[secret <name>]`. A value shorter than that is not masked, a value the command prints in another
  encoding is not recognised, and a value cut at the head of a stream's last 64 KiB may leave its tail.
- **Where a value goes:** the tool's own process, the hub's exec request, whose record of a command keeps its
  text and not its variables, and the environment of the command's process on the node, and the file where one
  is asked for. No log line, refusal or answer of the tool carries one.
