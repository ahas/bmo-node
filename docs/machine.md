# The machine

Jobs, their browsers and model training share one machine, and nothing about a free checkout says the
machine has room for another job. So the node service reads the machine every minute, answers whether it
has room for another launch, and tells a pressure that stands. What a job, an agent or the queue does with
that is agency's (agency's `docs/agent-control.md`, 「Room on the machine」).

## What is read

- **The kernel's own figures:** the memory and CPU pressure (`/proc/pressure/memory` and
  `/proc/pressure/cpu`, each line's `avg10`, `avg60` and `avg300`), the one-minute load against the
  machine's thread count, `MemAvailable`, and swap total and free.
- **The disk:** the free space, for a writer that is not the root user, of the disk the data root stands
  on, where the workbenches, the stores' copies, the checkouts' builds and the snapshots write.
- **The processes worth naming:** every browser main process (a Chrome or Chromium process started
  without a `--type=` argument, which its renderers and helpers all carry), and every process holding
  50 MB or more of memory and swap together.
- **Whose a process is:** the agent place its environment names, `BMO_AGENT_PLACE`, which every agent's
  pane carries and every process under it inherits. The service hands the place's name; which job a place
  stands for is agency's.
- **A reading that cannot be had holds nothing.** A machine nobody could read is not judged short, and the
  standing says it could not be read.
- `BMO_PROC_ROOT` names another root for every reading, for a test.

## The limits

- **Each limit is a setting of node's store,** keyed `machine_limit:<name>`; a missing setting, or one that
  is not a positive number, reads as the default.

  | Name | Default | What it is |
  |---|---|---|
  | `launch_memory_full_pct` | 5 | memory `full` over the last minute, in percent, at or above which a launch is held |
  | `launch_memory_available_gb` | 4 | `MemAvailable` under which a launch is held |
  | `launch_load_per_thread` | 2 | the one-minute load per thread at or above which a launch is held |
  | `launch_disk_free_gb` | 20 | the data root's disk's free space under which a launch is held |
  | `alert_memory_full_pct` | 10 | memory `full` at or above which pressure is told |
  | `alert_minutes` | 2 | how long that pressure stands before it is told |

- **The limits on job counts are agency's,** since only agency knows which jobs are open.

## Room for a launch

- **`/machine/room` answers `held`:** every one of the four launch limits this machine meets, each named
  with its figure and its limit (`docs/api.md`, 「Agency's calls」), read now. Empty is room.
- **A machine that could not be read holds nothing.**
- The caller holds a launch while anything is held, beside its own limits.

## Pressure told as an event

- **Every minute the service reads the machine** and keeps the standing, which `GET /machine/standing`
  answers: the reading, what is held, how many browsers run, what each agent place holds, and the five
  largest processes with their places.
- **Memory pressure that stands is told once.** Memory `full` at or above the alert line, standing for the
  alert minutes, publishes `machine-pressure` with cause `memory`: the pressure, the line, the minutes it
  stood, the reading, what each place holds and the largest processes. It is told again only after the
  pressure has fallen below the line and risen again.
- **The disk under the launch line is told once.** The data root's disk under `launch_disk_free_gb`
  publishes `machine-pressure` with cause `disk`, the free space and the line, the first minute it is read
  there, and again only after it has risen to the line and fallen again.
- **What was told is kept,** as settings of node's store, so a restart neither tells a standing pressure
  again nor forgets one that recovered.
- The service tells nobody else: the event is published, and the consumer that tells the chat agent reads
  it from the event stream.

## What stays with agency

The open jobs on this machine and which of them run a browser, the job-count limits, which process
belongs to which job, herdr's unanswered calls, handing the queue back once the machine has room again,
the machines work goes round and their rotation, a work node's room read over its link, and the sweep of
finished items' workbenches.
