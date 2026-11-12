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
- `command`: the command's text.
- `shell`: `powershell`, `wsl` or `linux`. Where none is named the node's own is taken: `powershell` on a
  Windows host, `linux` otherwise.
- `cwd`: the folder the command runs in.
- `env`: variables laid over the agent's environment, each a string.
- `distro`: the WSL distribution a `wsl` command runs in.

## The answer

`exitCode` and `signal` say how the command ended, and `stdout` and `stderr` what it said, each its last
64 KiB with `stdoutCut` and `stderrCut` saying where more came. A command that ran is answered whatever its
code. A node out of reach, a hub that refuses, or a run the hub ended before the command's end is answered
as a failure saying why, with whatever the command said before.

A command is held to the hub's own limits on time and output. Start long work on the node as a unit of its
own and read it back with later commands.

## Examples

    bmo use bmo/node_exec --arg node=my-pc --arg command=hostname
    bmo use bmo/node_exec --json '{"node": "my-pc", "shell": "wsl", "cwd": "~", "command": "systemctl --user status"}'
