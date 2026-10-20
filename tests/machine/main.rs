//! This machine's minute look: the kernel's files read, the processes worth naming with their scopes, the
//! standing kept, and memory pressure and the disk under its line each an alert, its start and its end told
//! once.

use std::path::Path;

use hardware::machine::{
  Alert, KernelReading, Limits, Pressure, PressureLine, Process, ScopeUse, Standing, WATCH_MS, judge, kept,
  look, memory_full, parse_meminfo, parse_pressure, read_machine, read_processes,
};
use store::Store;

/// The variable the tests' processes name their scope under.
const SCOPE: &str = "TEST_SCOPE";

#[test]
fn a_pressure_file_reads_as_its_two_lines_and_anything_else_as_none() {
  let read = parse_pressure(
    "some avg10=1.50 avg60=2.25 avg300=0.10 total=123\nfull avg10=0.00 avg60=12.40 avg300=3.00 total=9\n",
  )
  .unwrap();

  assert_eq!(read.some.avg60, 2.25);
  assert_eq!(read.full.unwrap().avg60, 12.4);
  assert_eq!(parse_pressure("some avg10=1.0 avg60=1.0 avg300=1.0\n").unwrap().full, None);
  assert_eq!(parse_pressure("nothing here"), None);
}

#[test]
fn meminfo_reads_each_figure_in_kilobytes() {
  let mem =
    parse_meminfo("MemTotal:       32768000 kB\nMemAvailable:    4096000 kB\nHugePages_Total:       0\n");

  assert_eq!(mem.get("MemTotal"), Some(&32_768_000));
  assert_eq!(mem.get("HugePages_Total"), Some(&0));
}

fn line(avg: f64) -> PressureLine {
  PressureLine {
    avg10: avg,
    avg60: avg,
    avg300: avg,
  }
}

fn reading(full: f64, available: f64, load: f64, disk: Option<f64>) -> KernelReading {
  KernelReading {
    at: 0,
    load1: load,
    threads: 24,
    mem_total_gb: 32.0,
    mem_available_gb: available,
    swap_total_gb: 16.0,
    swap_free_gb: 16.0,
    disk_free_gb: disk,
    memory: Some(Pressure {
      some: line(0.0),
      full: Some(line(full)),
    }),
    cpu: None,
  }
}

fn process(pid: i64, megabytes: f64, browser: bool, scope: Option<&str>) -> Process {
  Process {
    pid,
    name: if browser { "chrome".into() } else { "bun".into() },
    megabytes,
    browser,
    scope: scope.map(str::to_owned),
  }
}

#[test]
fn a_machine_as_read_carries_no_alert_and_counts_its_browsers() {
  let processes: Vec<Process> =
    (1..=3).map(|id| process(id, 400.0, true, Some(&format!("w{id}")))).collect();
  let standing = judge(Some(reading(25.0, 2.0, 74.0, Some(3.0))), &processes);

  assert!(standing.alerts.is_empty());
  assert_eq!(standing.browsers, 3);
  assert_eq!(judge(None, &[]).reading, None);
}

#[test]
fn memory_is_gathered_by_scope_largest_first() {
  let standing = judge(
    None,
    &[
      process(1, 300.4, false, Some("worker")),
      process(2, 1200.0, true, Some("worker")),
      process(3, 2000.0, false, Some("chat")),
      process(4, 900.0, false, None),
    ],
  );

  assert_eq!(
    standing.scopes,
    [
      ScopeUse {
        scope: "chat".into(),
        megabytes: 2000,
        browsers: 0
      },
      ScopeUse {
        scope: "worker".into(),
        megabytes: 1500,
        browsers: 1
      },
    ]
  );
  assert_eq!(standing.largest.iter().map(|one| one.pid).collect::<Vec<_>>(), [3, 2, 4, 1]);
  assert_eq!(standing.largest[0].scope.as_deref(), Some("chat"));
}

#[test]
fn the_limits_are_the_store_s_rows_and_a_row_that_is_no_positive_number_is_the_default() {
  let store = Store::in_memory().unwrap();

  store.set_setting("machine_limit:alert_disk_free_gb", "40").unwrap();
  store.set_setting("machine_limit:alert_memory_full_pct", "-1").unwrap();
  store.set_setting("machine_limit:alert_minutes", "ten").unwrap();

  assert_eq!(
    Limits::read(&store).unwrap(),
    Limits {
      alert_disk_free_gb: 40.0,
      ..Limits::defaults()
    }
  );
}

/// A proc root of the test's own: load, threads, memory and pressure as the kernel words them.
fn fake_proc(
  dir: &Path,
  load1: f64,
  threads: usize,
  available_kb: u64,
  memory_full60: f64,
  swap_free_kb: u64,
) {
  std::fs::create_dir_all(dir.join("pressure")).unwrap();
  std::fs::write(dir.join("loadavg"), format!("{load1} 30.10 28.00 12/7136 80143\n")).unwrap();
  std::fs::write(
    dir.join("stat"),
    format!(
      "cpu  1 2 3 4\n{}intr 1\n",
      (0..threads).map(|cpu| format!("cpu{cpu} 1 2 3 4\n")).collect::<String>()
    ),
  )
  .unwrap();
  std::fs::write(
    dir.join("meminfo"),
    format!(
      "MemTotal:       33554432 kB\nMemFree:          200000 kB\nMemAvailable:   {available_kb} kB\nSwapTotal:      16777216 kB\nSwapFree:       {swap_free_kb} kB\n"
    ),
  )
  .unwrap();
  std::fs::write(
    dir.join("pressure/memory"),
    format!(
      "some avg10=30.00 avg60={} avg300=10.00 total=1\nfull avg10=28.00 avg60={memory_full60} avg300=9.00 total=1\n",
      memory_full60 + 3.0
    ),
  )
  .unwrap();
  std::fs::write(
    dir.join("pressure/cpu"),
    "some avg10=40.00 avg60=35.00 avg300=20.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
  )
  .unwrap();
}

/// A process standing in a proc root of the test's own.
fn fake_process(
  proc: &Path,
  pid: u32,
  name: &str,
  rss_kb: u64,
  swap_kb: u64,
  args: &[&str],
  scope: Option<&str>,
) {
  let dir = proc.join(pid.to_string());

  std::fs::create_dir_all(&dir).unwrap();
  std::fs::write(
    dir.join("status"),
    format!("Name:\t{name}\nVmRSS:\t{rss_kb} kB\nVmSwap:\t{swap_kb} kB\n"),
  )
  .unwrap();
  std::fs::write(dir.join("cmdline"), [name].iter().chain(args).copied().collect::<Vec<_>>().join("\0"))
    .unwrap();
  std::fs::write(
    dir.join("environ"),
    scope.map(|scope| format!("HOME=/srv/user\0{SCOPE}={scope}\0")).unwrap_or_default(),
  )
  .unwrap();
}

#[test]
fn reads_load_threads_memory_swap_and_pressure_out_of_the_kernel_s_files() {
  let dir = tempfile::tempdir().unwrap();

  fake_proc(dir.path(), 74.2, 24, 1_782_579, 25.31, 0);

  let reading = read_machine(dir.path(), 5, None).unwrap();

  assert_eq!(
    (
      reading.at,
      reading.load1,
      reading.threads,
      reading.mem_total_gb,
      reading.swap_total_gb,
      reading.swap_free_gb,
      reading.disk_free_gb
    ),
    (5, 74.2, 24, 32.0, 16.0, 0.0, None)
  );
  assert!((reading.mem_available_gb - 1.7).abs() < 0.05);
  assert_eq!(reading.memory.as_ref().unwrap().full.as_ref().unwrap().avg60, 25.31);
  assert_eq!(reading.cpu.as_ref().unwrap().some.avg60, 35.0);
  assert_eq!(memory_full(Some(&reading)), Some(25.31));
}

#[test]
fn a_machine_whose_files_cannot_be_read_is_no_reading_at_all() {
  let dir = tempfile::tempdir().unwrap();

  assert!(read_machine(&dir.path().join("nowhere"), 0, None).is_none());
  assert!(read_processes(&dir.path().join("nowhere"), Some(SCOPE)).is_empty());
}

#[test]
fn names_every_browser_main_process_and_every_large_process_with_the_scope_it_runs_under() {
  let dir = tempfile::tempdir().unwrap();
  let proc = dir.path();

  fake_proc(proc, 1.0, 4, 20_000_000, 0.0, 0);
  fake_process(proc, 1000, "chrome", 400_000, 0, &["--user-data-dir=/tmp/x"], Some("worker-w3"));
  // A renderer is a large process like any other, never a browser of its own.
  fake_process(proc, 1001, "chrome", 300_000, 0, &["--type=renderer"], Some("worker-w3"));
  fake_process(proc, 1002, "claude", 600_000, 100_000, &[], Some("worker-w3"));
  fake_process(proc, 1003, "sleep", 1_000, 0, &[], None);
  fake_process(proc, 1004, "python3", 9_000_000, 0, &[], None);

  let mut found = read_processes(proc, Some(SCOPE));

  found.sort_by_key(|one| one.pid);
  assert!(
    read_processes(proc, None).iter().all(|one| one.scope.is_none()),
    "with no variable configured no process has a scope"
  );
  assert_eq!(
    found
      .iter()
      .map(|one| (one.name.as_str(), one.megabytes.round() as i64, one.browser, one.scope.as_deref()))
      .collect::<Vec<_>>(),
    [
      ("chrome", 391, true, Some("worker-w3")),
      ("chrome", 293, false, Some("worker-w3")),
      ("claude", 684, false, Some("worker-w3")),
      ("python3", 8789, false, None),
    ]
  );
}

#[test]
fn the_machine_is_read_out_of_the_kernel_s_files_with_the_disk_the_caller_names() {
  let dir = tempfile::tempdir().unwrap();
  let proc = dir.path().join("proc");

  fake_proc(&proc, 74.0, 24, 2_097_152, 25.0, 0);
  fake_process(&proc, 100, "chrome", 300_000, 0, &["--no-sandbox"], Some("worker-w4"));
  fake_process(&proc, 101, "chrome", 100_000, 0, &["--type=renderer"], Some("worker-w4"));
  fake_process(&proc, 102, "python3", 900_000, 0, &[], None);
  fake_process(&proc, 103, "bash", 4_000, 0, &[], Some("chat"));

  let here = hardware::machine::here(&proc, dir.path(), Some(SCOPE), 0);

  assert_eq!(
    here.scopes,
    [ScopeUse {
      scope: "worker-w4".into(),
      megabytes: 391,
      browsers: 1
    }]
  );
  assert_eq!(here.largest.iter().map(|one| one.pid).collect::<Vec<_>>(), [102, 100, 101]);
  assert!(here.reading.unwrap().disk_free_gb.is_some());
  assert!(read_machine(&proc, 5, None).unwrap().disk_free_gb.is_none());
}

fn standing(memory_full: f64, disk_free_gb: Option<f64>) -> Standing {
  Standing {
    reading: Some(KernelReading {
      at: 0,
      load1: 31.0,
      threads: 24,
      mem_total_gb: 32.0,
      mem_available_gb: 2.0,
      swap_total_gb: 16.0,
      swap_free_gb: 0.0,
      disk_free_gb,
      memory: Some(Pressure {
        some: line(memory_full),
        full: Some(line(memory_full)),
      }),
      cpu: None,
    }),
    alerts: vec![],
    browsers: 6,
    scopes: vec![],
    largest: vec![],
  }
}

/// Whether a look at the standing moved its alerts, and the alerts it kept.
fn looked(store: &Store, standing: Standing, at: i64) -> (bool, Vec<Alert>) {
  let looked = look(store, standing, at).unwrap();

  (looked.alerts_moved, looked.standing.alerts)
}

#[test]
fn the_minute_look_tells_an_alert_as_it_starts_and_as_it_ends_and_keeps_the_standing() {
  let minute = WATCH_MS;

  // The disk under its line: an alert for the stretch it stays there, moved once as it starts and once as
  // it ends.
  let store = Store::in_memory().unwrap();

  assert_eq!(
    looked(&store, standing(0.0, Some(15.04)), 0),
    (
      true,
      vec![Alert::Disk {
        free_gb: 15.0,
        limit_gb: 20.0
      }]
    )
  );
  assert!(!looked(&store, standing(0.0, Some(15.0)), minute).0);
  assert_eq!(looked(&store, standing(0.0, Some(120.0)), 2 * minute), (true, vec![]));
  assert!(looked(&store, standing(0.0, Some(15.0)), 3 * minute).0);

  // Memory pressure: an alert once it has stood for the alert minutes.
  let store = Store::in_memory().unwrap();

  assert_eq!(looked(&store, standing(25.0, None), 0), (false, vec![]));
  assert_eq!(
    looked(&store, standing(25.0, None), minute),
    (
      true,
      vec![Alert::Memory {
        full_pct: 25.0,
        alert_pct: 10.0,
        minutes: 2
      }]
    )
  );

  // The minutes counting up move nothing; falling below the line ends it, and rising again starts anew.
  assert_eq!(
    looked(&store, standing(30.0, None), 2 * minute),
    (
      false,
      vec![Alert::Memory {
        full_pct: 30.0,
        alert_pct: 10.0,
        minutes: 3
      }]
    )
  );
  assert_eq!(looked(&store, standing(3.0, None), 3 * minute), (true, vec![]));
  assert_eq!(looked(&store, standing(25.0, None), 4 * minute), (false, vec![]));
  assert!(looked(&store, standing(25.0, None), 5 * minute).0);

  // Pressure under the line is never an alert, and the standing is kept where every reader reads it.
  let store = Store::in_memory().unwrap();

  for at in 0..5 {
    assert_eq!(looked(&store, standing(9.0, None), at * minute), (false, vec![]));
  }

  look(&store, standing(2.0, None), 0).unwrap();
  assert_eq!(kept(&store).unwrap(), Some(standing(2.0, None)));
}
