# Hardware monitor

Every machine of the platform's hardware: this one and every monitored node of the list, each shared node
and each personal node of an account the configuration names (「Which machines」). Each machine is read by
the same probe over its own road, and every reading is described the same way: one group per piece of
hardware (the CPU with its chip laid out as it physically stands, each GPU, memory, every storage device,
the board), each carrying its own use, capacity, temperature and power, so the piece at fault is the one
that shows it. Every tracked machine is read live, once a second, and kept for a month. A screen draws
what this service answers (`docs/api.md`, 「Machines」).

## Which machines

- **This machine,** named `local`, read with a local shell.
- **Every monitored node of the list,** in the list's order, and nothing else. The monitor keeps no list of
  its own. Whether a node is monitored is read from its ownership and its account (`docs/nodes.md`, 「A
  node's ownership」) against one setting, the monitored accounts (`docs/api.md`, 「Configuration」):
  - A shared node is monitored.
  - A personal node is monitored where its account is one of the monitored accounts, so the PCs the
    deployment's own people run for the platform are read, set up and shown as any shared node; the node's own
    answer carries its ownership and its account, by which a screen names its owner.
  - Any other personal node, another person's own PC or one whose account is not known, is never read, shown or
    set up by the monitor: it is no machine the monitor knows, so the machine routes answer it as no machine at
    all. Its check and its runs go on as any node's. Where no account is configured, no personal node is
    monitored.
  - A node that stops being monitored, made personal or handed to an account that is not monitored, stops being
    read at the recorder's next pass; what was recorded of it ages out as any machine's history does. The
    monitored accounts are read at start, so a changed setting is taken up by a restart.
  - An ssh node is read by logging in the way its check does (batch mode, the node's key alone, the host
    key held to) and running the probe there, in the language the node's login answers in.
  - A hub node is read by handing the probe to its hub's `POST /exec?agent=<agent>`, with the control token
    read out of the hub's token file for that one request: as a `linux` shell command, or, for a hub node
    whose shell is `powershell` (a Windows host, `docs/nodes.md`, 「What a row holds」), the PowerShell probe
    as a `powershell` command, packed as the ssh road packs it, so the command the agent starts stays inside
    what Windows takes. Each agent is a machine of its own; the hub is never read or shown as one.
- The same road runs any other shell script the monitor hands a machine; the Windows setup below is one.
- **A machine is shown once.** A node that is this machine (`docs/nodes.md`, 「This machine」) carries
  `thisMachine`, and a screen folds it into this machine's tile, since the local reading is the same
  probe on the same machine.

## The probe

- **One POSIX shell script, the same on every machine and every road that takes a POSIX shell.** It prints
  sections, each headed by a line `@@<name>`, and every figure is taken out of its output by one parser. A
  section the machine does not have is empty or missing, and the parser reads that as not readable.
- A node whose login answers in Windows PowerShell is read by the Windows probe, which prints sections
  the same parser reads (「A Windows machine」).
- **It runs in two forms, made of the same sections:** once, one reading and done, which is how a machine
  is read where the recorder has nothing fresh of it (「How often」); and live, one run that stays open
  over the machine's road and prints a reading every second (「Live」).
- **The scripts stand written in this repository** and the recorder embeds them: two POSIX live scripts,
  one for a hub's exec, which ends itself after four minutes, and one that runs until it is stopped, and
  the PowerShell live script, which runs until it is stopped.
- **The probe's fixtures** stand beside the tests: each a probe output, from a test sample or a real
  machine, with the readings the parser makes of it written beside it. The parity test reads every output
  and fails where a reading differs from the one written.
- What it reads:
  - host name, kernel release, whether the kernel is WSL's, and the distribution's name and version from
    os-release;
  - `/proc/uptime`, `/proc/loadavg`, the five totals of `/proc/meminfo` the monitor keeps (memory and swap,
    total and available, and the shared memory), the CPU model out of `/proc/cpuinfo`;
  - each logical CPU's package, core, the set of CPUs its L3 is shared with, and its current clock from
    cpufreq;
  - `/proc/stat` twice, which each CPU's use is worked out from: half a second apart in the probe run once,
    one reading apart in the live run;
  - the package energy counter of the powercap interface, twice across the same stretch, which CPU power is
    worked out from, read directly or through `sudo -n` where the account may use sudo without a password
    and the counter is root's alone;
  - every hwmon chip's temperatures and fans, with their labels and the device each chip belongs to, and for
    each temperature the critical and the highest one the chip names for it (「Temperature limits」);
  - `nvidia-smi`, on the path or where WSL puts it, for each NVIDIA GPU's use, clocks, temperature, power,
    memory and fan, and whether thermal slowdown is holding its clocks down; and its whole report as XML,
    where the temperature each GPU slows down at stands, read in a live run's first reading alone;
  - the block devices from `lsblk`, the sysfs device each disk stands on, and from `df` each mounted file
    system a volume can be (「Storage」);
  - on WSL, the Windows side (below);
  - the running and failed services of systemd, the system's and the account's own;
  - in a live run, every five readings, who holds the memory (「Who holds the memory」).
- **A value that cannot be read is null, never nought.** In particular:
  - a CPU's clock is read from cpufreq; where there is none, `/proc/cpuinfo` on a machine that is not WSL,
    and on WSL the Windows side's;
  - CPU power is unreadable where the energy counter is missing; where it refuses the account and sudo is
    not open to it, the power is the code `denied`;
  - a board input nothing is wired to reads nought, -128 or a byte's top; a temperature at or below nought
    or at 127 °C and above is no temperature and is left out.
- A machine that cannot be reached at all (the login refused or broken, the agent not attached) answers a
  failure with what was met, and none of its figures.

## The Windows side

- **On WSL the machine's own figures are read from the Windows it runs on,** since WSL sees a virtual
  machine with the VM's own load, its share of memory, fixed clocks and no sensors. The probe hands a
  PowerShell script, as one encoded command, to the Windows PowerShell under `/mnt/c`, and what it answers
  stands in place of the virtual machine's figures:
  - every logical processor's use and effective clock from the performance counters, in Windows' own order;
  - package power from the Energy Meter counter, or the sensor library's own package power where there is
    no such counter;
  - memory and uptime from the operating system, and its edition and build;
  - each physical disk with its bus, medium and size, and the lettered volumes on it;
  - the CPU's temperatures, the board chip's temperatures and fans, and each disk's temperature, from
    LibreHardwareMonitor, the CPU's sensors given the names the kernel's own drivers use (`Tctl`,
    `Tccd<n>`, `Package id 0`, `Core <n>`), and a figure the library works out from others left out.
- The load average and the services stay the WSL side's, and the WSL virtual disk stays one of the
  machine's storage devices.
- **In the live run the Windows side stays open too:** one PowerShell runs for the whole live run. It
  prints an empty block as soon as it starts, before anything slow, then opens the counters and the
  library once and prints a block every second, which paces the run. Where it ends having printed no
  block past that first empty one (the Windows PowerShell cannot be started from the run, as from a service
  with no WSL interop, or it fails while opening), the run reads the virtual machine's own figures
  instead, as on any Linux machine, so the machine is still read.
  - The edition and the boot time are asked again every thirty readings.
  - **The disks and volumes are asked in the background** every thirty readings, and each reading carries
    the last answers that came in. An asking still out is not asked again, so a storage query that hangs
    costs only the disks' figures and never holds a reading up.
  - **Each disk's temperature is read from the library every thirty readings,** and the readings in
    between carry the last, since reading a drive's sensors opens the drive. The CPU's and the board's
    sensors are read every second.
- **LibreHardwareMonitor and its driver are the monitor's own setup on each Windows machine.** The
  library's release is unpacked under `%ProgramData%` and reads the CPU and the board through the PawnIO
  driver, installed with winget; the place, the release and the driver package are the policy's
  `windows-sensors.toml`, which the sensor script and the setup both read.
- **`bmo-node hardware-setup [<node>…]`** puts both in place on the Windows of each node named, or of every
  monitored node: under a WSL node, or a Windows node itself. A node the monitor does not read named is
  refused, since nothing of the monitor's is installed on a PC it does not monitor. Run again it changes nothing already in place, and a
  node with no Windows is left alone. It is done only where both the library and the driver answered. A
  machine without the library still has every figure the performance counters give, and its board says
  the library is missing.

## A Windows machine

- **A node whose login answers in Windows PowerShell is read by the Windows probe,** with no WSL in between.
- **The road hands the probe to Windows PowerShell as one encoded command** over the node's own login:
  nothing is written on the machine.
- **It prints the sections the parser reads:** a head saying the reading is a Windows machine's, the host
  name and, in the live form, the uptime each reading was taken at; every record of 「The Windows side」;
  the edition with its build and kernel version, the CPU's model with its cores and logical processors,
  and the page file's size and use as the swap; each NVIDIA GPU in the columns the POSIX probe takes from
  `nvidia-smi`, and the temperature it slows down at.
- **Its live form starts and paces itself as the Windows side's does:** a first block of the host and the
  run's own clock as soon as it starts, the disks and volumes asked in the background, and each disk's
  temperature read every thirty readings (「The Windows side」).
- **Every GPU figure comes from NVML called inside the probe's own PowerShell,** opened once per run and
  asked each reading, never from a process started per reading. Where the driver's library is missing the
  GPUs are unreadable.
- **What Windows does not have is unreadable:** the load average and systemd's services.
- **The chip is drawn from the CPU's model and its counts:** Windows numbers a core's logical processors
  next to each other, so each logical processor's core is its place divided by the threads per core.
- **A Windows machine's login is the node's own setup,** a Windows OpenSSH server whose login shell is
  Windows PowerShell, on a port of its own. Until that login is open the node is unreadable, and its
  failure says what the login met.

## Storage

- **Every storage device is its own piece of hardware,** never a total: its model, its kind (`nvme`, `ssd`,
  `hdd`, `usb`, the WSL virtual disk, a network share), its size, each volume on it with its used and total
  space, and its own temperature where it has one.
- On Linux a mount is hung on the disk it stands on by following `lsblk`'s parents, and a bind mount of a
  source already shown is not shown again. A disk connected with nothing mounted is still listed. The
  kernel's own file systems and loop devices are not storage.
- A network share (NFS, SMB, an rclone or sshfs mount) is a device of its own, named by its source.
- **A mount that stopped answering costs only its own figures and never holds a reading up.** Each
  volume is asked on its own, in the background, with the parts that hardly change, and every reading
  carries the answers that have come in. A volume whose asking is still out when it is asked again is left
  out of the readings until it answers, and is not asked twice at once. The answers are kept in a folder
  of the run's own under the machine's temporary directory, removed when the run ends; where no such
  folder can be made, the volumes are asked in the reading itself, one after another.
- A drive's temperature is its own hwmon chip's (`nvme`, `drivetemp`), matched by the sysfs device the chip
  and the disk share; the composite reading where a drive reports several.
- On WSL the Windows disks are the machine's drives, and the Linux side contributes only the virtual disk.

## How often

- **One period, a second, paces everything live:** how often each machine is read, and how far apart two
  readings may stand before a line breaks.
- **The recorder is the one reader.** It keeps every tracked machine's live run open whether a screen is
  open or not, so a machine is read once a second however many screens show it, and a machine whose
  tracking is off is not read at all (「Tracking」).
- **`GET /machines/{machineName}` answers the recorder's latest reading** of the machine. Where the recorder
  has nothing fresh of a tracked machine, no reading or failure within ten seconds, the service reads it
  itself with the probe run once, keeps that reading for one period and joins a read already under way, so
  several askers at once cost the machine one probe.
- **A probe run once is waited on until it gives its first whole reading or its run ends,** whatever
  that takes; a run that ends first is answered as a failure with what it met.

## Units

- **Every figure is kept in its physical unit:** memory, VRAM and storage in GB, power in W, temperature in
  °C, clock in MHz, a fan in RPM. A use, and a GPU fan, are shares.
- Memory, VRAM and a volume are counted in binary gigabytes, the way the machine reports them; a drive's
  own size is its maker's, in decimal units.

## History

- **Every figure is named by the piece of hardware it belongs to and what it measures,** `<piece>|<metric>`:
  the metric is `use`, `temp`, `watts`, `mhz`, `gb` or `rpm`, and the piece `cpu`, `memory`, `gpu<n>`,
  `memory:available`, `memory:shmem`, `swap`, `gpu<n>:vram`, `storage:<key>` or `fan:<chip>:<label>`. A figure
  is kept to a tenth, a store to a hundredth of a gigabyte. The live record and the history use the same names.
- **Every closed minute is folded out of the live record** into `hardware_samples`, once a minute: one row
  per machine per minute holding each figure's lowest, highest, sum and count across that minute's
  readings, under the minute's start. A machine is never read a second time for its history. Each pass
  folds the minutes that closed since the machine's last row, as far back as the live record reaches.
- A figure that was not read is left out of its row, and a minute with no reading leaves a gap.
- **Every closed half hour is rolled up** into `hardware_rollups` the same way, a bounded number at a time,
  so a month of passes with no rollups is taken in over the passes that follow.
- **How long a row is kept:** a minute's row for two days, and only once its half hour has been rolled up;
  a half hour's row for thirty-one days. Each pass removes what has grown older.
- **A reading is asked by its span,** `spanMs`, and each stretch of that width is drawn out of the finest
  rows that hold it (`docs/api.md`, 「Machines」):

  | Span | Drawn out of | Reaching back |
  |---|---|---|
  | whole seconds under a minute | the live record | fifteen minutes |
  | whole minutes | the minutes' rows | two days |
  | whole half hours | the half hours' rows, with the minutes' rows of the half hour not rolled up yet | thirty-one days |

  - The half hour not yet rolled up is read from the minutes' rows, so a week's or a month's last stretch
    is as fresh as the day's.
  - Each stretch carries each figure's lowest, highest and mean over it; a stretch nothing was recorded in
    is left out.
- **Every live reading is published as it is taken,** as `reading.created`, a stretch of one second
  (`docs/api.md`, 「Events」).

## Live

```mermaid
flowchart TD
  M[each machine: the live run, a reading a second] --> B[the recorder]
  B --> R[live record: the latest reading, one row a second]
  R --> A[the machine routes and reading.created]
  R --> H[store: one row a minute, folded out of the record]
```

- **The recorder holds one live run open per tracked machine,** this one and every tracked monitored node,
  and reads the list again every minute, starting a run for a node added and stopping the run of one removed
  or no longer monitored.
  - Each reading carries the whole machine; the parts that hardly change (the host and OS, the CPU's model
    and layout, the block devices, the mounts and the services) are read again only every thirty readings,
    and the recorder lays the last of them under each reading in between.
  - Use and power are worked out between one reading and the one before, across the time the machine
    itself says passed; a run's first reading only sets the start.
  - **A reading's second is counted along the machine's own clock** from the second this machine saw the
    run's first reading at. A count that strays more than five seconds from this machine's clock starts
    again from now.
  - **A run on a local shell or an ssh login lasts until its stream ends:** its script ends, the login
    breaks, or the machine is no longer read. The login's keep-alive tells a machine gone (`docs/nodes.md`,
    「How a node's state is known」), and the hub's stream carries the connection's own keep-alive.
  - **A run through a hub is relieved before the hub stops it:** the hub stops an exec at five minutes, so
    the run's script ends itself after four, the next run starts twenty seconds ahead of that and takes
    over at its first reading.
  - A run that ends after carrying the machine is followed by the next at once. A run that fails or ends
    before its first reading makes the machine's latest the failure with what was met, and a run starts
    again after a wait that grows from a second to thirty while the failures go on.
  - **A run that has given no reading yet is stopped once it has printed nothing for a minute,** and
    counts as a run that failed before its first reading: the machine's latest says it stalled, and the
    next run starts after the same growing wait. Every live script prints its first block within seconds
    of starting, a Windows one before it opens anything, so a minute of silence means the machine is not
    getting on with the run. A run
    started to relieve another that stalls is only dropped while the one it was to relieve still carries
    the machine.
  - **A run that has given a reading and then prints nothing for thirty seconds is left running:** the
    machine's latest says it has gone quiet, once for each quiet stretch, and its next reading stands in
    its place.
  - A run stopped is ended on the machine at once, whatever its road: a local shell or an ssh login is
    killed, and a hub's exec is cancelled without waiting for the script to print again.
- **The live record** is a SQLite file of its own, `hardware-live.db`, written by the recorder alone, on a
  thread of its own, since a commit waits on the disk and a reading lands every second. The service reads
  it over a connection that only reads.
  - It holds each machine's latest reading in full, or the failure its run met, with the second it came;
    and one row per machine per second, holding that reading's figures under the names of 「History」.
- **How much is kept:** fifteen minutes back, and at most 16 KB for every second of that reach. Each
  minute's pass folds the closed minutes first, then removes the rows older than the reach, then the
  oldest beyond the size.

## Who holds the memory

So that a machine that ran out of memory can be explained afterwards, each machine's figures stand beside who held
the memory at that moment.

- **A look at the memory's holders** is taken by the live run itself, every five readings, a machine reaching
  exhaustion in seconds as easily as in hours. It costs the machine a few tens of milliseconds, reads nothing that
  waits on a process's memory, and holds two lists:
  - **the processes,** the eight largest by resident memory and swap together, each with its pid, its name, the unit
    its control group names, its `VmRSS`, its `RssShmem` and its `VmSwap`;
  - **the units,** the six largest service and scope control groups by memory and swap together, each with its
    `memory.current`, the `anon`, `shmem` and `file` of its `memory.stat`, and its `memory.swap.current`.
- **Both lists are needed.** A process's `RssShmem` counts only what it has mapped this moment, and the kernel's
  reclaim drops the mappings of shared memory it cannot free, so shared memory still resident may be charged to a
  unit while no process shows it; the unit's `shmem` names its owner.
- **A machine that cannot say is left out:** a Windows machine has no such lists, and the WSL side of a machine says
  what the WSL virtual machine holds. A section that does not read leaves a list empty.
- **A sample** is such a look with the machine's `MemAvailable`, `Shmem` and swap used at the same reading.
- **The live record keeps every sample for its fifteen minutes** (「Live」), the minute's pass folding each closed minute's
  worst into `memory_samples`: the sample of the lowest `MemAvailable` in that minute, under its own second. A minute's
  sample is kept seven days, and each pass removes what has grown older.
- **It is pulled, never kept on the machine.** The samples stand on this machine from the moment the run prints them,
  so a machine that freezes or falls over takes none with it, and what it printed up to its last reading stays. While
  the machine is too starved to run the probe or carry its output nothing is taken, and the gap says so; the
  machine's five-second samples and its one-second figures show the climb up to it.
- **A machine's samples are read** by `GET /machines/{machineName}/memory` (`docs/api.md`, 「Machines」), the last
  fifteen minutes in five-second samples and the rest of the seven days in the minutes' worst.

## Tracking

- **Every machine's live tracking is a switch, on unless it was turned off,** kept per machine as the
  setting `hardware_tracking:<machine>`. A machine with no setting is tracked, and a setting left by a node
  since removed reads nothing.
- **Off means the machine is not read at all:** the recorder stops its live run, nothing is added to the
  live record, and `GET /machines/{machineName}` answers it as not tracked instead of reading it.
- **The minute rows stop with it,** so a machine whose tracking is off leaves a gap in its history.
- **`PATCH /machines/{machineName}` with `tracking` moves the switch;** the recorder follows it at once, and
  every minute's pass
  reads the settings again as well.

## The chip

- **The chip is described as it physically stands:** the dies where they sit on the package, and inside
  each die the cores, the caches and the rest in their places, each part with an English name a screen
  may word in its own language.
- The CPU model decides the layout, and the machine's own topology decides how many of each part:
  - **AMD Zen 2 desktop** (Ryzen 3000 without a G): up to two CCDs side by side above the I/O die, each
    holding two CCXs, one above the other, half the cores left of the L3 and half right.
  - **AMD Zen 3 desktop** (Ryzen 5000 without a G): the same package; each CCD is one CCX, half its cores
    left of the L3 and half right.
  - **Intel Core with a ring and no efficiency cores** (up to the 10th generation): one die, the GPU on the
    left (switched off on an F part), the cores stacked each beside its slice of the LLC, and the system
    agent on the right.
  - **Anything else:** one die, the cores grouped by the L3 they share.
- Which physical core a logical CPU is cannot be read, so the cores fill the places in the order of their
  core numbers.
- A sensor that names a die is placed on it: AMD's `Tccd<n>` on CCD n-1, `Tctl` on the package, Intel's
  `Package id` on the die and `Core <n>` on core n.

## Temperature limits

- **Each part's temperature carries that part's own limit,** the temperature it starts to slow itself down
  at:
  - a GPU: the temperature its driver says it slows down at, and whether thermal slowdown is holding its
    clocks down;
  - a hwmon temperature: the critical temperature its chip names, or the highest where it names no
    critical one;
  - a CPU whose chip names none (AMD's `k10temp`, a Windows machine's sensor library): the limit set for
    that machine.
- **A machine's CPU limit is the setting `hardware_cpu_limit:<machine>`,** moved by `PATCH
  /machines/{machineName}` with `cpuLimit`. A limit set stands over the one the chip names, and the reading says which of the
  two it is. With neither, the CPU's temperature has no limit.
- **No limit of any part stands in the code.**
