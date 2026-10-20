# The machine

Work of every kind shares this machine, and how loaded it is cannot be told from outside it. So the node
service reads this machine every minute, keeps what it read as the machine's standing, and tells when a
pressure starts or stops standing. Whether a launch may start, and what to do about a pressure, is the
caller's own policy, read from the machine (`docs/api.md`, 「Machines」).

## What is read

- **The kernel's own figures:** the memory and CPU pressure (`/proc/pressure/memory` and
  `/proc/pressure/cpu`, each line's `avg10`, `avg60` and `avg300`), the one-minute load against the
  machine's thread count, `MemAvailable`, and swap total and free.
- **The disk:** the free space, for a writer that is not the root user, of the disk the data root stands
  on.
- **The processes worth naming:** every browser main process (a Chrome or Chromium process started
  without a `--type=` argument, which its renderers and helpers all carry), and every process holding
  50 MB or more of memory and swap together.
- **Whose a process is:** its scope, the value of the environment variable the configuration's
  `scope_variable` names, which every process under the one it was set in inherits. The scope is the
  caller's opaque string, handed as it stands; the service never reads it. Where no variable is
  configured, no process has a scope.
- **A reading that cannot be had holds nothing.** A machine nobody could read raises no alert, and the
  standing says it could not be read.
- `BMO_PROC_ROOT` names another root for every reading, for a test.

## The limits

- **Each limit is a setting of node's store,** keyed `machine_limit:<name>`; a missing setting, or one that
  is not a positive number, reads as the default.

  | Name | Default | What it is |
  |---|---|---|
  | `alert_memory_full_pct` | 10 | memory `full` over the last minute, in percent, at or above which memory pressure stands |
  | `alert_minutes` | 2 | how long that pressure stands before it is an alert |
  | `alert_disk_free_gb` | 20 | the data root's disk's free space under which the disk is an alert |

## The standing and its alerts

- **Every minute the service reads the machine** and keeps the standing, which `GET /machines/local`
  answers as the machine's `standing`: the reading, the alerts standing now, how many browsers run, what
  each scope holds, and the five largest processes with their scopes.
- **Memory pressure that stands is an alert.** Memory `full` at or above the alert line, standing for the
  alert minutes, is the `memory` alert, with the pressure, the line and the minutes it has stood, for as
  long as it stays at or above the line.
- **The disk under its line is an alert.** The data root's disk under `alert_disk_free_gb` is the `disk`
  alert, with the free space and the line, for as long as it stays there.
- **An alert that starts or ends is told once,** as `machine.changed` with the machine (`docs/api.md`,
  「Events」). The minutes counting up under a standing alert move nothing.
- **What was told is kept,** as the standing and the time memory pressure started standing, settings of
  node's store, so a restart neither tells a standing alert again nor forgets one that ended.
- The service tells nobody else: the event is published, and a consumer that tells anyone reads it from
  the event stream.

## What stays with the caller

Whether a launch may start on the machine and the limits that decide it, which work runs on the machine
and what each scope stands for, handing work back once the machine has room again, the machines work goes
round, and the room of another machine read over its own login.
