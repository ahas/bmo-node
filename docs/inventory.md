# Inventory

What this repository took from the old single repository, `bmo` at commit `3e4c032da`, member by member:
where each came from, how many, and what became of it. Files were listed with `git ls-files` on that
commit; tests were listed by their function names in the files named. Nothing here was copied with its
history.

Treatments:

- **carried**: the file's code moved here, with its ties to the old repository cut and nothing of its
  behaviour changed;
- **carried in part**: only the parts named moved; the rest stays with the unit named;
- **realised in Rust**: a TypeScript file whose behaviour the Rust code here already holds, so the
  behaviour came and the file did not;
- **dropped**: not needed here, with the reason;
- **another unit's**: the unit named owns it.

## Rust source: 32 files

| Old place | Files | Here | Treatment |
|---|---|---|---|
| `nodes/node-check/src`: `lib`, `main`, `hub`, `pass`, `rows`, `ssh` | 6 | `crates/node-check` | carried; it reads node's own store and policy crates |
| `nodes/node-check/Cargo.toml` | 1 | `crates/node-check` | carried, its dependencies on the old shared crates removed |
| `nodes/node-check/tests/check.rs` | 1 | `tests/node-check` | carried |
| `llm-server/llm-bridge/src`: `lib`, `main`, `door`, `keys` | 4 | `crates/llm-bridge` | carried |
| `llm-server/llm-bridge/Cargo.toml` | 1 | `crates/llm-bridge` | carried |
| `llm-server/llm-bridge/tests/contract.rs` | 1 | `tests/llm-bridge` | carried |
| `llm-server/bmo-llm-bridge.service` | 1 | `deploy/bmo-llm-bridge.service` | carried, its description made neutral |
| `bot/bmo-server/src/watches/hardware`: `chips`, `history`, `js`, `live`, `mod`, `parity`, `probe`, `record`, `roads`, `storage`, `view`, `windows` | 12 | `crates/hardware` | carried; English codes stand where a screen word stood |
| `bot/bmo-server/src/watches/machine.rs` | 1 | `crates/hardware`, the machine module | carried in part (「The machine watch」 below) |
| `bot/bmo-server/src/control/hardware.rs` | 1 | `crates/server` | carried; its refusals are English |
| `bot/bmo-server/src/control/nodes.rs` | 1 | `crates/server` | carried, apart from the node links route, which is agency's |
| `bot/bmo-server/src/verbs/nodes.rs` | 1 | `crates/server`, the `bmo-node` command | carried |
| `bot/bmo-server/src/verbs/hardware_setup.rs` | 1 | `crates/server`, the `bmo-node` command | carried; it reaches a node through node-check's own shell instead of the file viewer's road |

The query reading a node with its hub, written out in four places in the old repository, is one query of
`crates/store` here.

### The machine watch

The old minute watch over the machine served several units at once. Node carries what is a fact of the
machine; agency keeps what turns on jobs, agents and herdr (`docs/machine.md`).

- **Carried:** reading memory and CPU pressure, `MemAvailable`, swap, the one-minute load against the
  thread count, the free space of the data root's disk, and the processes worth naming with the agent
  place each names; the six machine limits (`launch_memory_full_pct`, `launch_memory_available_gb`,
  `launch_load_per_thread`, `launch_disk_free_gb`, `alert_memory_full_pct`, `alert_minutes`); a launch
  held on the four hardware limits; memory pressure that stands and the disk under its line, each told
  once and again only after it recovered; the standing a console shows.
- **Agency's:** the job-count limits (`launch_jobs`, `launch_browser_jobs`), which process belongs to which
  job, herdr's unanswered calls, handing the queue back once there is room, the work machines and their
  rotation, a heavy run's placement, a node's room read over its link, and the sweep of finished items'
  workbenches.

## The hardware TypeScript package: 73 files

`nodes/hardware`: 19 runtime files, 16 tests, 3 configuration files, 32 probe fixtures and 3 probe
scripts.

| Files | Count | Treatment |
|---|---|---|
| `hardware-chips`, `hardware-history`, `hardware-series`, `hardware-live-record`, `hardware-parity`, `hardware-probe`, `hardware-storage`, `hardware-view`, `hardware-windows` | 9 | realised in Rust by `crates/hardware` |
| `hardware-probe-files` | 1 | realised in Rust: the probe scripts stand written in `crates/hardware` and the recorder embeds them |
| `machine-pressure`, `machine-room` | 2 | realised in Rust by the machine module |
| `jake-hub`, `ssh-login`, `node-shell` | 3 | realised in Rust by `crates/node-check` |
| `hardware-read`, `node-view`, `this-machine` | 3 | realised in Rust by `crates/server`'s hardware reads and node views |
| `process-memory` | 1 | agency's: its `machine memory` verb |
| `hardware-chips.test`, `hardware-history.test`, `hardware-live-record.test`, `hardware-parity.test`, `hardware-probe.test`, `hardware-storage.test`, `hardware-view.test`, `hardware-windows.test` | 8 | each assertion held by a Rust test in `tests/hardware` |
| `jake-hub.test`, `ssh-login.test`, `node-shell.test` | 3 | each assertion held by a Rust test in `tests/node-check` |
| `machine-room.test` | 1 | each assertion held by a Rust test in `tests/machine` |
| `hardware-read.test`, `this-machine.test` | 2 | each assertion held by a Rust test in `tests/server` |
| `nodes.test` | 1 | each assertion held by a Rust test in `tests/cli` |
| `process-memory.test` | 1 | agency's |
| `package.json`, `tsconfig.json`, `bunfig.toml` | 3 | dropped: no TypeScript package stands here |
| `fixtures/probe`: `linux-cpuinfo-clocks`, `linux-generic`, `linux-intel-ring`, `linux-limits`, `linux-no-services`, `linux-storage`, `linux-zen2-two-ccds`, `linux-zen3`, `linux`, `live-limits`, `live-local`, `live-synthetic`, `live-windows`, `windows`, `wsl-no-windows`, `wsl`, each a `.txt` output and the `.json` readings made of it | 32 | carried to `tests/hardware/fixtures`, personal paths made neutral |
| `probe-files`: `live-posix-240.sh`, `live-posix-3600.sh`, `live-powershell-3600.ps1` | 3 | carried to `crates/hardware` |

The two third-party libraries the package used, an event-stream parser and an XML parser, went with the
TypeScript code; the Rust recorder reads the probe's sections and the GPU report itself.

## Archive files: 4

The old archive package's facade over tables other units own; these four were assigned to node.

| File | Treatment |
|---|---|
| `archive/archive/nodes.ts` | realised in Rust: the registry's operations in `crates/store` |
| `archive/archive/hardware-tables.ts` | realised in Rust: the history tables in `crates/store`'s migration |
| `archive/archive/node-state.ts` | realised in Rust: the state words and their tones in `crates/policy` |
| `archive/archive/node-state.test.ts` | its assertion held by a Rust test in `tests/policy` |

## Policy files and words: 5 sources

| Source | Treatment |
|---|---|
| `policy/bmo-policy/policy/node-states.toml` | carried to `crates/policy`; each state keeps its tone, and the human word is the web console's |
| `policy/bmo-policy/policy/ssh.toml` | carried to `crates/policy` |
| `policy/bmo-policy/policy/windows-sensors.toml` | carried to `crates/policy` |
| the node list's words in the old language pack | the `bmo-node` command prints English |
| the launch holds' words in the old language pack | `held` is English, each reason with its figure |

Node is now the only reader of all three policy files. The old shared policy crate and language crate
are not depended on.

## Tests: 62

| Source | Tests | Here |
|---|---|---|
| `nodes/node-check/tests/check.rs` | 11 | `tests/node-check` |
| `nodes/node-check/src/ssh.rs`, inline | 2 | `tests/node-check` |
| `llm-server/llm-bridge/tests/contract.rs` | 8 | `tests/llm-bridge` |
| `llm-server/llm-bridge/src/keys.rs`, inline | 4 | `tests/llm-bridge` |
| `watches/hardware`, inline: `js` 2, `history` 1, `parity` 1, `roads` 2, `windows` 1, `live` 6 | 13 | `tests/hardware` |
| `control/nodes.rs`, inline | 2 | `tests/server` |
| `watches/machine.rs`, inline | 8 | 7 to `tests/machine`; the rotation's test is agency's |
| `bot/bmo-server/tests/integration/watches.rs` | 6 | below |
| `bot/bmo-server/tests/integration/machine_verbs.rs` | 8 | `tests/cli` |

The six in `watches.rs`:

- the node pass checking every node and telling the screens only when a state moved: `tests/server`;
- the machine watch telling the disk, the pressure and herdr once and handing the queue back: its memory
  and disk parts in `tests/machine`, the rest agency's;
- the machine read out of the kernel's files and judged against the open jobs: its reading and its four
  hardware limits in `tests/machine`, the job counts agency's;
- the recorder holding live runs for the tracked machines alone, the live record keeping each machine's
  latest and pruning by reach and size, and the history folding closed minutes and rolling up closed half
  hours: `tests/hardware`.

The eight in `machine_verbs.rs` are the node list's verbs (listing, an ssh node's defaults, refusals,
registering, changing and removing, hubs and a jake node checked through one, each node's address and
credential place) and the Windows sensor setup (done only where both the library and the driver
answered; a Windows machine handed the setup script in its own PowerShell). The same file's tests of a
node's pane, the remote desktop port, pictures and the work nodes' tokens are other units'.

## Stored state: 6

| What | Old place | Here |
|---|---|---|
| the node list | archive table `nodes` | node's store, table `nodes` |
| the hubs | archive table `node_hubs` | node's store, table `hubs` |
| the minutes' history | archive table `hardware_samples` | node's store, same name |
| the half hours' history | archive table `hardware_rollups` | node's store, same name |
| the settings: `hardware_tracking:<machine>`, `hardware_cpu_limit:<machine>`, `machine_limit:<name>` for the six limits, and the pressure watch's own marks | archive settings | node's store, table `settings`, same keys |
| the live record | `hardware-live.db` beside the archive | `hardware-live.db` in node's data folder |

Moving the rows out of the old archive belongs to the switch-over (`docs/store.md`).

## Routes and events: 22

Every route and event is stated in `docs/api.md`: agency's four calls, the two reads job's calls are
answered from, eight console routes, six registry routes, the event stream and the health route.

## Units: 2

| Unit | Treatment |
|---|---|
| `bmo-node.service` | new: the node service, which runs the routes, the check pass, the recorder and the machine watch in one process |
| `bmo-llm-bridge.service` | carried from `llm-server` |

The old repository started the check pass, the recorder and the machine watch inside its chat receiver's
process; no unit of their own stood for them.

## What other units keep

- **agency:** node-link and a link's state on its own side, a work node's workplace, its setup and its pane
  (`work_node`, `work_node_setup`, `node_pane`), the node workspaces in herdr (`places/nodes`), the work
  node tables (`node_workplaces`, `node_rotation`, `node_assignments`), the machine watch's parts named
  above, the `machine memory` verb and the workbench sweep. Agency reads nodes through the four calls of
  `docs/api.md`.
- **bmo-job:** its node routes are answered through agency.
- **bmo-drive:** the file viewer's road onto a node (`control/files/node`).
- **bmo-explorer:** the remote browser (`verbs/remote_browser`).
- **bmo-web:** the console's node list, node screens, monitor screens and their routes, which call this
  service's API.
- **bmo-voice and bmo-kakaotalk:** their node readers, through the API.
