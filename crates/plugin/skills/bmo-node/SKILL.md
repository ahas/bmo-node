---
name: bmo-node
description: How the bmo-node MCP tools list the machines a node service knows, say how each is reached, check whether it answers, and read its load and past hardware figures.
---

# bmo-node

A node service lists the machines reachable from where it runs, checks once a minute whether each answers,
and reads every machine's hardware live, keeping a month of it. The `bmo-node` MCP server reads all of it
through the service's routes, one tool for each, so a harness can find, reach and size up a machine with
nothing else running.

## Starting the server

Register the server with the harness as a stdio MCP server:

```
bmo-node mcp --url <the service's base address>
```

The address is the one the deployment names for the service, where its `/health` route stands; never guess one. The service's bearer token goes
in the `BMO_NODE_TOKEN` environment variable of the server, or `--token`; every route needs it, and a wrong
one answers every tool with a 401 `unauthorized` error. On the machine the service runs on,
`bmo-node --config <file> mcp` takes both from the service's own configuration instead.

## Reaching a node

1. `node_nodes`: every node in the list's order, with its `kind` (`ssh`, or `hub` for a machine reached
   through a hub), its `address`, its `ownership` (`shared`, a machine of the platform, or `personal`, a
   person's own PC with its `account`), whether the hardware monitor reads it (`monitored`), its `role` and the
   last check's `state`. `up` says the last check found it connected within the last three minutes. A node
   whose `role` names services is reserved for them; leave it alone unless your work is one of them.
2. `node_target` with `{ "nodeName": "<name>" }` before logging in. For an ssh node, `sshArgs` opens a batch
   login ahead of the command:

   ```
   ssh <sshArgs...> <command>
   ```

   `sshOptions` are the same options for `sftp` or `scp`, each given after `-o`. `shell` is `posix`, or
   `powershell` for a Windows login, which answers in PowerShell. A hub node (`kind` `hub`) is reached by no
   SSH: run a command on it with the hosted tool `bmo/node_exec` (`bmo use bmo/node_exec --describe` gives its
   arguments) and in no other way. Its target names the hub and the `agentName` the hub knows the machine by;
   never read the hub's token file, never ask the hub's routes by hand, and where `bmo/node_exec` refuses, ask
   your supervisor rather than going around it.
3. `node_check` with `{ "body": { "nodeNames": ["<name>"] } }` when you need to know whether a node answers
   now rather than as of the last pass; leave `nodeNames` empty to check every node. Each node comes back as
   it stands after its check.

A target never holds a credential, only where one is kept; a credential is never read into a conversation.

## Before heavy work

`node_machine` with `{ "machineName": "<name>" }`: `local` for the service's own machine, a node's name for a
node. `hardware` is the latest reading, one group per piece of hardware (`cpu`, `memory`, `gpus`, `storage`,
`board`); `failure` says why a machine could not be read; `tracking` false means it is not read at all. For
`local`, `standing` carries the kernel's memory and CPU pressure, the load against the thread count, the free
disk, and `alerts`: a memory pressure or a full disk that stands now. Whether to start the work is yours to
decide from these figures; the service holds nothing back.

`node_machines` gives every machine at once, as the recorder last read each, without reading any of them
again. Only this machine and the monitored nodes are machines: every shared node, and a personal node only
where the deployment monitors its account; any other personal node is never read.

## Past figures

`node_readings` with `{ "machineName": "<name>", "spanMs": 60000, "after": <epoch ms> }` pages a machine's
past figures, oldest first, each stretch a band of `min`, `max` and `mean`. `spanMs` under a minute reads the
last fifteen minutes; whole minutes reach two days back; whole half hours, a month. Pass the last item's `at`
as `after` for the next page while `more` is true.

`node_memory` with `{ "machineName": "<name>", "after": <epoch ms> }` answers who held a machine's memory: its
`MemAvailable`, `Shmem` and swap used, its largest processes with their `rssMb`, `shmemMb` and `swapMb`, and its
largest units. Every five seconds over the last fifteen minutes, and each minute's worst for seven days. Read the
`units` as well as the `processes`: shared memory no process has mapped is charged only to its unit.

## When a tool answers an error

A tool error's text is JSON: `status`, `code` and `error`, the service's English sentence. Match on `code`:

- `not-found`: no node or machine of that name; read `node_nodes` again.
- `incomplete`: the node's row lacks a field its connection type needs; it cannot be reached until a person
  fixes it.
- `bad-field`: a field names nothing listed or is out of range; the sentence names it.
- `bad-request`: the arguments are not the shape the tool takes; the sentence names what is wrong.

An error with no `code` means the service could not be reached at all; check the address and that the
service is running.

The routes behind each tool are stated in the node service's `docs/api.md` and `docs/openapi.json`, whose
`x-tools` lists every tool's input and output.
