# bmo/node_exec

Runs one command on a hub node, a machine attached to a jake hub such as a person's PC, and answers the
command's exit code and its output.

## Who may call it

- A session reaches the node its own account owns: the node agency's record of a person's node names for the
  account of the agent the session is held by.
- A session holding the administrator's grant, `node.admin`, reaches every node.
- Any other call is refused before the node is reached, and the refusal names the node and its owner.

## Arguments

- `node`: the node's name, as the node service lists it.
- `account`: the account whose node it is, by its id, a number or a string, in place of `node`. The node is
  the one agency's record of a person's node names for the account; an account with no node is refused, and
  the caller is judged on that node as on one named. Name the node by `node` or by `account`, never both.
- `command`: the command's text.
- `shell`: `powershell`, `wsl` or `linux`. Where none is named the node's own is taken: `powershell` on a
  Windows host, `linux` otherwise. A Windows host runs `powershell` and `wsl`, any other host `linux`; a shell the
  host does not run is refused. A `wsl` command reaches bash as written: write `$`, quotes and backticks as you
  would on a Linux host, without escaping them again.
- `cwd`: the folder the command runs in.
- `env`: variables laid over the agent's environment, each a string.
- `distro`: the WSL distribution a `wsl` command runs in.
- `secrets`: saved secrets the command is handed by name (below).

## Secrets by name

Never write a key, a token or a password into `command` or `env`. Name the saved secret instead, and the tool
reads it on the main machine and hands it to the command, so its value passes through nothing you write or
read.

- Which secrets you reach: with the administrator's grant, the platform's, the data root's `env/`; otherwise
  your own account's alone.
- Each item of `secrets` is a name, or an object `{"name": …, "prefix": …, "as": "env" | "file" | "both"}`.
  A name is a plain file name; one that names no secret is refused, naming it.
- A secret of `KEY=VALUE` lines, such as an account login, is handed as variables, one per field, named by its
  keys with `prefix` before each.
- A secret of one value, such as a token or a key file, is written to a file on the node that only the node's
  user reads, and its path is in `BMO_SECRET_<STEM>_FILE`, the stem being the name in capitals with every other
  character an underscore: `my-token` gives `BMO_SECRET_MY_TOKEN_FILE`. Asked `as: "env"`, the value is in
  `BMO_SECRET_<STEM>`. `as: "file"` writes a file of fields whole, too.
- The file is removed when the command ends, however it ends.
- A secret's value the command prints is masked in the answer as `[secret <name>]`. Still, never print one.
- A variable two secrets would set, or a secret and `env` both, is refused: give a `prefix`.

## The answer

`exitCode` and `signal` say how the command ended, and `stdout` and `stderr` what it said, each its last
64 KiB with `stdoutCut` and `stderrCut` saying where more came. A command that ran is answered whatever its
code. A node out of reach, a hub that refuses, or a run the hub ended before the command's end is answered
as a failure saying why, with whatever the command said before.

A command is held to the hub's own limits on time and output. Start long work on the node as a unit of its
own and read it back with later commands.

## Examples

    bmo use bmo/node_exec --arg node=my-pc --arg command=hostname
    bmo use bmo/node_exec --json '{"account": 4, "command": "hostname"}'
    bmo use bmo/node_exec --json '{"node": "my-pc", "shell": "wsl", "cwd": "~", "command": "systemctl --user status"}'
    bmo use bmo/node_exec --json '{"node": "my-pc", "secrets": ["site-login"], "command": "./sign-in.ps1 -User $env:USERNAME"}'
    bmo use bmo/node_exec --json '{"node": "my-pc", "secrets": [{"name": "site-login", "prefix": "SITE_"}], "shell": "linux", "command": "python3 sign_in.py"}'
    bmo use bmo/node_exec --json '{"node": "my-pc", "shell": "linux", "secrets": ["deploy-key"], "command": "ssh -i \"$BMO_SECRET_DEPLOY_KEY_FILE\" host true"}'
