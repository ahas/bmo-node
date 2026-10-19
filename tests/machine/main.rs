//! This machine's minute look: the kernel's files read, the processes worth naming with their places,
//! the four hardware launch limits named with their figures, the standing kept, and memory pressure and the
//! disk under its line each told once as an event.

use std::path::Path;
use std::sync::Mutex;

use bmo_node_client::events::EventKind;
use hardware::machine::{
  Event,  Limits, Place, Pressure, PressureLine, Process, Publish, Reading, Standing, WATCH_MS, judge, kept, look,
  memory_full, parse_meminfo, parse_pressure, read_machine, read_processes,
};
use serde_json::Value;
use store::Store;

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

fn reading(full: f64, available: f64, load: f64, disk: Option<f64>) -> Reading {
  Reading {
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

fn process(pid: i64, megabytes: f64, browser: bool, place: Option<&str>) -> Process {
  Process {
    pid,
    name: if browser { "chrome".into() } else { "bun".into() },
    megabytes,
    browser,
    place: place.map(str::to_owned),
  }
}

#[test]
fn a_machine_with_room_holds_nothing() {
  let standing = judge(Some(reading(0.5, 16.0, 20.0, Some(100.0))), &[], &Limits::defaults());

  assert!(standing.held.is_empty());
}

#[test]
fn every_hardware_limit_met_is_named_with_its_figure() {
  let processes: Vec<Process> =
    (1..=3).map(|id| process(id, 400.0, true, Some(&format!("w{id}")))).collect();
  let standing = judge(Some(reading(25.0, 2.0, 74.0, Some(3.0))), &processes, &Limits::defaults());

  assert_eq!(
    standing.held,
    [
      "memory pressure 25% >= 5%",
      "available memory 2GB < 4GB",
      "load 74 >= 24 threads x 2",
      "disk free 3GB < 20GB",
    ]
  );
  assert_eq!(standing.browsers, 3);
}

#[test]
fn a_machine_nobody_could_read_holds_nothing_on_its_reading() {
  let standing = judge(None, &[], &Limits::defaults());

  assert!(standing.held.is_empty());
  assert_eq!(standing.reading, None);
}

#[test]
fn memory_is_gathered_by_place_largest_first() {
  let standing = judge(
    None,
    &[
      process(1, 300.4, false, Some("worker")),
      process(2, 1200.0, true, Some("worker")),
      process(3, 2000.0, false, Some("chat")),
      process(4, 900.0, false, None),
    ],
    &Limits::defaults(),
  );

  assert_eq!(
    standing.places,
    [
      Place {
        place: "chat".into(),
        megabytes: 2000,
        browsers: 0
      },
      Place {
        place: "worker".into(),
        megabytes: 1500,
        browsers: 1
      },
    ]
  );
  assert_eq!(standing.largest.iter().map(|one| one.pid).collect::<Vec<_>>(), [3, 2, 4, 1]);
}

#[test]
fn the_limits_are_the_store_s_rows_and_a_row_that_is_no_positive_number_is_the_default() {
  let store = Store::in_memory().unwrap();

  store.set_setting("machine_limit:launch_disk_free_gb", "40").unwrap();
  store.set_setting("machine_limit:launch_memory_full_pct", "-1").unwrap();
  store.set_setting("machine_limit:alert_minutes", "ten").unwrap();

  assert_eq!(
    Limits::read(&store).unwrap(),
    Limits {
      launch_disk_free_gb: 40.0,
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
  place: Option<&str>,
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
    place
      .map(|place| format!("HOME=/home/user\0BMO_AGENT_PLACE=/data/agent-places/{place}/\0"))
      .unwrap_or_default(),
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
  assert!(read_processes(&dir.path().join("nowhere")).is_empty());
}

#[test]
fn names_every_browser_main_process_and_every_large_process_with_the_agent_place_it_runs_under() {
  let dir = tempfile::tempdir().unwrap();
  let proc = dir.path();

  fake_proc(proc, 1.0, 4, 20_000_000, 0.0, 0);
  fake_process(proc, 1000, "chrome", 400_000, 0, &["--user-data-dir=/tmp/x"], Some("worker-w3"));
  // A renderer is a large process like any other, never a browser of its own.
  fake_process(proc, 1001, "chrome", 300_000, 0, &["--type=renderer"], Some("worker-w3"));
  fake_process(proc, 1002, "claude", 600_000, 100_000, &[], Some("worker-w3"));
  fake_process(proc, 1003, "sleep", 1_000, 0, &[], None);
  fake_process(proc, 1004, "python3", 9_000_000, 0, &[], None);

  let mut found = read_processes(proc);

  found.sort_by_key(|one| one.pid);
  assert_eq!(
    found
      .iter()
      .map(|one| (one.name.as_str(), one.megabytes.round() as i64, one.browser, one.place.as_deref()))
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
fn a_busy_machine_holds_every_launch_on_every_hardware_limit_it_met_with_whose_memory_it_is() {
  let dir = tempfile::tempdir().unwrap();

  fake_proc(dir.path(), 74.0, 24, 1_782_579, 25.0, 0);

  let reading = read_machine(dir.path(), 0, None).unwrap();
  let mut processes: Vec<Process> = (1..7)
    .map(|at| process(2000 + at, 900.0, true, Some(&format!("worker-w{}", at + 1))))
    .collect();

  processes.extend((0..7).map(|at| process(3000 + at, 700.0, false, Some(&format!("worker-w{}", at + 1)))));
  processes.push(Process {
    pid: 4000,
    name: "python3".into(),
    megabytes: 9000.0,
    browser: false,
    place: None,
  });
  processes.push(process(4001, 300.0, false, Some("chat")));

  let standing = judge(Some(reading), &processes, &Limits::defaults());

  assert_eq!(
    standing.held,
    ["memory pressure 25% >= 5%", "available memory 1.7GB < 4GB", "load 74 >= 24 threads x 2"]
  );
  assert_eq!(standing.browsers, 6);
  assert_eq!(
    standing.places[0],
    Place {
      place: "worker-w2".into(),
      megabytes: 1600,
      browsers: 1
    }
  );
  assert_eq!(
    standing.places.last(),
    Some(&Place {
      place: "chat".into(),
      megabytes: 300,
      browsers: 0
    })
  );
  assert_eq!((standing.largest[0].pid, standing.largest[0].name.as_str()), (4000, "python3"));
  assert_eq!(standing.largest.len(), 5);
}

#[test]
fn a_disk_under_the_line_holds_the_launch_read_where_the_caller_names_one() {
  let dir = tempfile::tempdir().unwrap();

  fake_proc(dir.path(), 1.0, 24, 16_000_000, 0.0, 0);
  assert!(read_machine(dir.path(), 5, Some(dir.path())).unwrap().disk_free_gb.unwrap() > 0.0);

  let mut reading = read_machine(dir.path(), 5, None).unwrap();

  reading.disk_free_gb = Some(12.34);

  assert_eq!(judge(Some(reading), &[], &Limits::defaults()).held, ["disk free 12.3GB < 20GB"]);
}

#[test]
fn an_ordinary_working_day_holds_nothing() {
  let dir = tempfile::tempdir().unwrap();

  fake_proc(dir.path(), 30.0, 24, 16_000_000, 0.6, 8_000_000);

  assert!(judge(read_machine(dir.path(), 0, None), &[], &Limits::defaults()).held.is_empty());
}

#[test]
fn the_machine_is_read_out_of_the_kernel_s_files_and_judged_against_the_store_s_limits() {
  let dir = tempfile::tempdir().unwrap();
  let proc = dir.path().join("proc");
  let store = Store::in_memory().unwrap();

  fake_proc(&proc, 74.0, 24, 2_097_152, 25.0, 0);
  fake_process(&proc, 100, "chrome", 300_000, 0, &["--no-sandbox"], Some("worker-w4"));
  fake_process(&proc, 101, "chrome", 100_000, 0, &["--type=renderer"], Some("worker-w4"));
  fake_process(&proc, 102, "python3", 900_000, 0, &[], None);
  fake_process(&proc, 103, "bash", 4_000, 0, &[], Some("chat"));
  store.set_setting("machine_limit:launch_disk_free_gb", "0.000001").unwrap();

  let here = hardware::machine::here(&store, &proc, dir.path(), 0).unwrap();

  assert_eq!(
    here.held,
    ["memory pressure 25% >= 5%", "available memory 2GB < 4GB", "load 74 >= 24 threads x 2"]
  );
  assert_eq!(
    here.places,
    [Place {
      place: "worker-w4".into(),
      megabytes: 391,
      browsers: 1
    }]
  );
  assert_eq!(here.largest.iter().map(|one| one.pid).collect::<Vec<_>>(), [102, 100, 101]);
  assert!(here.reading.unwrap().disk_free_gb.is_some());
}

/// The events published, as a consumer would read them.
#[derive(Default)]
struct Told(Mutex<Vec<Value>>);

impl Publish for Told {
  fn publish(&self, event: Event) {
    assert_eq!(event.kind(), EventKind::MachinePressure);
    self.0.lock().unwrap().push(event.data());
  }
}

impl Told {
  fn facts(&self) -> Vec<Value> {
    self.0.lock().unwrap().clone()
  }
}

fn standing(memory_full: f64, disk_free_gb: Option<f64>) -> Standing {
  Standing {
    reading: Some(Reading {
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
    held: vec![],
    browsers: 6,
    places: vec![Place {
      place: "worker-w4".into(),
      megabytes: 3500,
      browsers: 1,
    }],
    largest: vec![hardware::machine::Largest {
      pid: 7,
      name: "python3".into(),
      megabytes: 9000,
      place: None,
    }],
  }
}

#[test]
fn the_machine_watch_tells_the_disk_and_the_pressure_once_and_keeps_the_standing() {
  let minute = WATCH_MS;

  // The disk under the launch line: told once for the stretch it stays there, and again once it has come
  // back and fallen again.
  let store = Store::in_memory().unwrap();
  let told = Told::default();

  look(&store, &standing(0.0, Some(15.0)), 0, &told).unwrap();
  look(&store, &standing(0.0, Some(15.0)), minute, &told).unwrap();

  let facts = told.facts();

  assert_eq!(facts.len(), 1);
  assert_eq!(
    (facts[0]["cause"].as_str(), facts[0]["freeGb"].as_f64(), facts[0]["limitGb"].as_f64()),
    (Some("disk"), Some(15.0), Some(20.0))
  );
  look(&store, &standing(0.0, Some(120.0)), 2 * minute, &told).unwrap();
  look(&store, &standing(0.0, Some(15.0)), 3 * minute, &told).unwrap();
  assert_eq!(told.facts().len(), 2);

  // Memory pressure: told once it has stood for the alert minutes, with what each place holds.
  let store = Store::in_memory().unwrap();
  let told = Told::default();

  look(&store, &standing(25.0, None), 0, &told).unwrap();
  assert!(told.facts().is_empty());
  look(&store, &standing(25.0, None), minute, &told).unwrap();

  let facts = told.facts();

  assert_eq!(facts.len(), 1);
  assert_eq!(
    (
      facts[0]["cause"].as_str(),
      facts[0]["minutes"].as_i64(),
      facts[0]["fullPct"].as_f64(),
      facts[0]["alertPct"].as_f64()
    ),
    (Some("memory"), Some(2), Some(25.0), Some(10.0))
  );
  assert_eq!(facts[0]["places"][0]["place"], "worker-w4");
  assert_eq!(facts[0]["largest"][0]["name"], "python3");

  // Told once for the stretch however long it stands; falling below and rising again is a new one.
  look(&store, &standing(30.0, None), 2 * minute, &told).unwrap();
  look(&store, &standing(3.0, None), 3 * minute, &told).unwrap();
  look(&store, &standing(25.0, None), 4 * minute, &told).unwrap();
  assert_eq!(told.facts().len(), 1);
  look(&store, &standing(25.0, None), 5 * minute, &told).unwrap();
  assert_eq!(told.facts().len(), 2);

  // Pressure under the line is never told.
  let store = Store::in_memory().unwrap();
  let told = Told::default();

  for at in 0..5 {
    look(&store, &standing(9.0, None), at * minute, &told).unwrap();
  }

  assert!(told.facts().is_empty());

  // The standing kept where the console reads it.
  look(&store, &standing(2.0, None), 0, &told).unwrap();
  assert_eq!(kept(&store).unwrap(), Some(standing(2.0, None)));
}
