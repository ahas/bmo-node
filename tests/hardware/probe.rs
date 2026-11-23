//! What the probe printed, turned into figures: the cases the fixtures' readings do not each stand for —
//! package power across the sample, a counter that wrapped, each part's limit, WSL's fixed clock, output cut
//! off — the live form taken a reading at a time, and the live scripts run as plain POSIX shell.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use hardware::probe::{LiveProbe, Snapshot, Watts, read_sections, sections};

use crate::hardware::fixtures;

/// What the probe prints on a bare Linux box with two cores of two threads, cpufreq, k10temp, an NVMe
/// drive, one NVIDIA GPU with its report, the energy counter refused, and systemd.
fn linux() -> String {
  std::fs::read_to_string(fixtures().join("../samples/linux-bare.txt")).unwrap()
}

fn read(text: &str) -> Option<Snapshot> {
  read_sections(&sections(text))
}

fn without_cpufreq(text: &str) -> String {
  text.lines().filter(|line| !line.contains("cpufreq")).collect::<Vec<_>>().join("\n")
}

#[test]
fn works_out_package_power_from_the_energy_counter_across_the_sample() {
  let text = linux()
    .replace("@@rapl1\ndenied", "@@rapl1\n1000000000")
    .replace("@@rapl2\ndenied", "@@rapl2\n1030000000");

  assert_eq!(read(&linux()).unwrap().cpu_watts, Some(Watts::Denied));
  assert_eq!(read(&text).unwrap().cpu_watts, Some(Watts::Read(60.0)));
  assert_eq!(
    read(&text.replace("@@end", "@@interval\n2\n@@end")).unwrap().cpu_watts,
    Some(Watts::Read(15.0))
  );
}

#[test]
fn a_counter_that_wrapped_says_nothing_and_a_machine_without_one_has_no_power() {
  let wrapped = linux()
    .replace("@@rapl1\ndenied", "@@rapl1\n900")
    .replace("@@rapl2\ndenied", "@@rapl2\n100");
  let missing = linux().replace("@@rapl1\ndenied", "@@rapl1").replace("@@rapl2\ndenied", "@@rapl2");

  assert_eq!(read(&wrapped).unwrap().cpu_watts, None);
  assert_eq!(read(&missing).unwrap().cpu_watts, None);
}

#[test]
fn reads_each_part_s_temperature_limit_and_a_gpu_s_thermal_slowdown() {
  let snapshot = read(&linux()).unwrap();
  let highest = read(&linux().replace("/sys/class/hwmon/hwmon0/temp1_crit:84850\n", "")).unwrap();
  let report_start = linux().find("@@gpureport").unwrap();
  let report_end = linux().find("</nvidia_smi_log>\n").unwrap() + "</nvidia_smi_log>\n".len();
  let mut unreported = linux();

  unreported.replace_range(report_start..report_end, "");

  let slowed =
    read(&unreported.replace("[N/A], Not Active, Not Active", "[N/A], Active, Not Active")).unwrap();

  assert_eq!(snapshot.storage[0].limit_celsius, Some(84.9));
  assert_eq!(snapshot.gpus[0].limit_celsius, Some(95.0));
  assert_eq!(snapshot.gpus[0].throttling, Some(false));
  assert_eq!(highest.sensors[0].limit_celsius, Some(81.9));
  assert_eq!((slowed.gpus[0].limit_celsius, slowed.gpus[0].throttling), (None, Some(true)));
  assert_eq!(read(&linux().replace(", Not Active, Not Active", "")).unwrap().gpus[0].throttling, None);
}

#[test]
fn a_board_input_nothing_is_wired_to_is_no_temperature() {
  assert!(!read(&linux()).unwrap().sensors.iter().any(|one| one.chip == "nct6795"));
}

#[test]
fn on_wsl_the_fixed_cpuinfo_figure_is_not_read_as_a_clock() {
  let wsl = without_cpufreq(
    &linux().replace("7.0.0-31-generic\n@@version", "6.6.87.2-microsoft-standard-WSL2\n@@version"),
  );
  let snapshot = read(&wsl).unwrap();

  assert!(snapshot.wsl);
  assert_eq!(snapshot.clock_source, None);
  assert!(snapshot.threads.iter().all(|one| one.mhz.is_none()));
}

#[test]
fn without_cpufreq_a_machine_that_is_not_wsl_reads_its_clocks_out_of_cpuinfo() {
  let snapshot = read(&without_cpufreq(&linux())).unwrap();

  assert_eq!(snapshot.clock_source, Some("cpuinfo"));
  assert_eq!(
    snapshot.threads.iter().map(|one| one.mhz).collect::<Vec<_>>(),
    [Some(4229.0), Some(4249.0), Some(2200.0), Some(2200.0)]
  );
}

#[test]
fn output_cut_off_before_its_end_is_refused() {
  assert!(read(&linux().replace("@@end", "")).is_none());
}

/// A live run's seldom parts, and one reading of it.
const SELDOM: &str = "@@host\nexample-host\n@@kernel\n7.0.0-31-generic\n@@model\nmodel name\t: AMD Ryzen 9 3900X 12-Core Processor\n@@layout\n/sys/devices/system/cpu/cpu0/topology/core_id:0\n@@df\n/dev/nvme0n1p2 ext4 1000 400 600 40% /";

fn often(uptime: i64, busy: i64, energy: i64) -> String {
  format!(
    "@@uptime\n{uptime}\n@@stat\ncpu  {busy} 0 0 {idle} 0 0 0 0\ncpu0 {busy} 0 0 {idle} 0 0 0 0\n@@rapl\n{energy}\n@@clock\n/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq:3600000\n@@end",
    idle = uptime * 100 - busy
  )
}

#[test]
fn each_live_reading_after_the_first_is_whole_the_seldom_parts_laid_under_it_use_and_power_across_the_stretch()
 {
  let mut probe = LiveProbe::default();

  assert!(probe.take(&format!("{SELDOM}\n{}", often(100, 0, 1_000_000))).is_none());

  let snapshot = read_sections(&probe.take(&often(102, 50, 61_000_000)).unwrap()).unwrap();

  assert_eq!(snapshot.host.as_deref(), Some("example-host"));
  assert_eq!(snapshot.cpu_model.as_deref(), Some("AMD Ryzen 9 3900X 12-Core Processor"));
  assert_eq!(snapshot.cpu_usage, Some(0.25));
  assert_eq!(snapshot.cpu_watts, Some(Watts::Read(30.0)));
  assert_eq!(snapshot.threads.len(), 1);
  assert_eq!(
    (snapshot.threads[0].core, snapshot.threads[0].usage, snapshot.threads[0].mhz),
    (Some(0.0), Some(0.25), Some(3600.0))
  );
  assert!(!snapshot.storage.is_empty());
}

#[test]
fn keeps_the_gpus_report_of_the_first_reading_for_every_reading_after_it() {
  let mut probe = LiveProbe::default();
  let gpu = "@@gpu\n0, RTX, 3, 40, 210, 2100, 405, 18.75, 200.00, 33, 8192, 30, Not Active, Not Active";
  let report = "@@gpureport\n<nvidia_smi_log><gpu><temperature><gpu_temp_slow_threshold>91 C</gpu_temp_slow_threshold></temperature></gpu></nvidia_smi_log>";

  probe.take(&format!("{SELDOM}\n{report}\n{gpu}\n{}", often(100, 0, 0)));
  probe.take(&format!("{gpu}\n{}", often(101, 10, 0)));

  let whole = probe.take(&format!("{gpu}\n{}", often(102, 20, 0))).unwrap();

  assert_eq!(read_sections(&whole).unwrap().gpus[0].limit_celsius, Some(91.0));
}

#[test]
fn a_reading_cut_off_or_not_moving_on_is_no_reading() {
  let mut probe = LiveProbe::default();

  probe.take(&format!("{SELDOM}\n{}", often(100, 0, 0)));

  assert!(probe.take(&often(101, 10, 0).replace("@@end", "")).is_none());
  assert!(probe.take(&often(100, 10, 0)).is_none());
}

fn script(name: &str) -> String {
  std::fs::read_to_string(
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("crates/hardware/probe-files")
      .join(name),
  )
  .unwrap()
}

#[test]
fn the_live_scripts_are_plain_posix_shell() {
  for name in ["live-posix-240.sh", "live-posix.sh"] {
    let mut check = Command::new("sh").arg("-n").stdin(Stdio::piped()).spawn().unwrap();

    std::io::Write::write_all(&mut check.stdin.take().unwrap(), script(name).as_bytes()).unwrap();
    assert!(check.wait().unwrap().success(), "{name}");
  }
}

#[test]
fn the_live_script_reads_this_machine_a_reading_at_a_time() {
  let mut run = Command::new("sh")
    .arg("-s")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .process_group(0)
    .spawn()
    .unwrap();
  let pid = run.id() as i32;

  std::io::Write::write_all(&mut run.stdin.take().unwrap(), script("live-posix-240.sh").as_bytes())
    .unwrap();

  let mut out = run.stdout.take().unwrap();
  let mut probe = LiveProbe::default();
  let mut text = String::new();
  let mut buffer = [0u8; 16 * 1024];
  let mut whole = None;

  while whole.is_none() {
    let read = out.read(&mut buffer).unwrap();

    if read == 0 {
      break;
    }

    text.push_str(&String::from_utf8_lossy(&buffer[..read]));

    while let Some(end) = text.find("@@end\n") {
      let reading: String = text.drain(..end + "@@end\n".len()).collect();

      if let Some(found) = probe.take(&reading) {
        whole = Some(found);
        break;
      }
    }
  }

  // SAFETY: the run was started as the leader of a process group of its own.
  unsafe {
    libc::kill(-pid, libc::SIGKILL);
  }
  let _ = run.wait();

  let snapshot = read_sections(&whole.expect("a whole reading")).unwrap();

  assert!(!snapshot.threads.is_empty());
}

// "Where it ends having printed no block — the Windows PowerShell cannot be started from the run, as from a
// service with no WSL interop — the run reads the virtual machine's own figures instead": the run stands as WSL
// with a PowerShell that cannot be executed, and still hands readings over.
#[test]
fn a_wsl_run_whose_windows_side_answers_nothing_reads_the_virtual_machine_instead() {
  let dir = tempfile::tempdir().unwrap();
  let powershell = dir.path().join("powershell.exe");

  std::fs::write(&powershell, "#!/bin/sh\necho 'cannot execute binary file' >&2\nexit 126\n").unwrap();
  std::fs::set_permissions(&powershell, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

  for name in ["live-posix-240.sh", "live-posix.sh"] {
    let as_wsl = script(name).replace("grep -qi microsoft /proc/version 2>/dev/null", "true").replace(
      "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
      &powershell.to_string_lossy(),
    );
    let mut run = Command::new("sh")
      .arg("-s")
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .process_group(0)
      .spawn()
      .unwrap();
    let pid = run.id() as i32;

    std::io::Write::write_all(&mut run.stdin.take().unwrap(), as_wsl.as_bytes()).unwrap();

    let mut out = run.stdout.take().unwrap();
    let mut text = String::new();
    let mut buffer = [0u8; 16 * 1024];

    while !text.contains("@@end\n") {
      let read = out.read(&mut buffer).unwrap();

      if read == 0 {
        break;
      }

      text.push_str(&String::from_utf8_lossy(&buffer[..read]));
    }

    // SAFETY: the run was started as the leader of a process group of its own.
    unsafe {
      libc::kill(-pid, libc::SIGKILL);
    }
    let _ = run.wait();

    assert!(text.contains("@@uptime") && text.contains("@@end\n"), "{name}: {text}");
  }
}
