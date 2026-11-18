# The store

Everything the node service keeps stands in its own folder under the data root, `node/`, and nowhere
else. No other unit reads these files; they reach what is in them through the API (`docs/api.md`).

| File | What it holds | Written by |
|---|---|---|
| `node/node.db` | the node list, the hubs, the hardware history and the settings | the service and the `bmo-node` command |
| `node/hardware-live.db` | the live record (`docs/hardware.md`, 「Live」) | the recorder alone |

Both are SQLite files in write-ahead-log mode. The service runs the store's migrations as it starts; the
command runs them too before it reads or writes. A connection to either file that meets another's
lock waits its turn until the lock is given back, however long it is held.

## The tables

- **`nodes`**: one row per node: `name` (the key), `label`, `kind`, `host`, `port`, `user`, `key_path`,
  `shell`, `agent`, `hub_url`, `ownership` (`shared` by default), `account`, `role`, `callers`, `position`,
  the last check's `state` (`unknown` by default), `detail`, `checked_at` and `connected_at`, and
  `created_at` and `updated_at` (`docs/nodes.md`, 「What a row holds」). `kind` is `ssh` or `hub`, and a hub node's token file is its hub's.
- **`hubs`**: one row per hub: `id` (the key, never given again once used), `url` (each listed once),
  `token_file`, `label` and `created_at`. When the table is first made it holds the local hub, `http://127.0.0.1:8791`, whose token file is the hub's environment file; the
  seed is written once, so a hub removed stays removed.
- **`hardware_samples`**: one row per machine per minute, `machine`, `at` (the minute's start) and
  `sample`, each figure's lowest, highest, sum and count as JSON; keyed by machine and minute.
- **`hardware_rollups`**: the same per half hour, `machine`, `at` and `tally`.
- **`memory_samples`**: one row per machine per minute, `machine`, `at` (the second of that minute's sample with the
  lowest `MemAvailable`) and `sample`, the sample as `GET /machines/{machineName}/memory` answers it as JSON; keyed by
  machine and second, kept seven days (`docs/hardware.md`, 「Who holds the memory」).
- **`settings`**: `key` and `value`, both text:
  - `hardware_tracking:<machine>` and `hardware_cpu_limit:<machine>` (`docs/hardware.md`);
  - `machine_limit:<name>` for each of the three machine limits (`docs/machine.md`);
  - the machine look's standing and the time memory pressure started standing.

A node is read with its hub in one query, so its kind, its agent and its hub's token file always come
together.

## The live record

- **`live_latest`**: each machine's latest reading in full as JSON, or the failure its run met, with the
  second it came.
- **`live_samples`**: one row per machine per second, that reading's figures as JSON.
- **`live_memory`**: one row per machine per look at who holds its memory, the sample as JSON, kept as long as the
  record's reach.

## What the store does not hold

- **No credential:** a row names where a key or a token is kept, never the key or the token.
- **Nothing of another unit's:** a tunnel's state, and whatever another unit keeps of the nodes, are that
  unit's.

## Moving in from the old archive

The rows of the old archive's `nodes`, `node_hubs`, `hardware_samples`, `hardware_rollups` and the
settings named above are moved into this store by the switch-over, which also stops the old service's
recorder. This repository has no data-moving step of its own: a fresh store starts empty but for its
seeded hub.
