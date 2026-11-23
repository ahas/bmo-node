
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$t = [char]9
$nl = [char]10
$out = [Console]::Out
# The run's first block, printed before anything slow: it only sets the run's start.
$clock = [Diagnostics.Stopwatch]::StartNew()
try { $out.Write("@@windows$nl@@host$nl$env:COMPUTERNAME$nl@@uptime$nl$($clock.Elapsed.TotalSeconds)$nl@@end$nl"); $out.Flush() } catch { exit }
# The disks and volumes are asked on a runspace of their own, so a storage query that hangs costs only their
# figures: each reading carries the last answers, and an asking still out is not asked again.
$storage = @{ lines = @(); ps = $null; done = $null }
$storage.space = [RunspaceFactory]::CreateRunspace()
$storage.space.Open()
$storage.script = @'
$ErrorActionPreference = 'SilentlyContinue'
$t = [char]9
foreach ($d in Get-PhysicalDisk) { "disk$t$($d.DeviceId)$t$($d.FriendlyName)$t$($d.MediaType)$t$($d.BusType)$t$($d.Size)" }
foreach ($p in Get-Partition) {
  if (-not $p.DriveLetter -or $p.DriveLetter -eq [char]0) { continue }
  $v = Get-Volume -DriveLetter $p.DriveLetter
  if ($v -and $v.Size -gt 0) { "vol$t$($p.DiskNumber)$t$($p.DriveLetter):$t$($v.FileSystem)$t$($v.Size)$t$($v.SizeRemaining)" }
}
'@
function AskStorage {
  if ($storage.ps) { return }
  $storage.ps = [PowerShell]::Create()
  $storage.ps.Runspace = $storage.space
  [void]$storage.ps.AddScript($storage.script)
  $storage.done = $storage.ps.BeginInvoke()
}
function TakeStorage {
  if (-not $storage.ps -or -not $storage.done.IsCompleted) { return }
  try { $storage.lines = @($storage.ps.EndInvoke($storage.done) | ForEach-Object { [string]$_ }) } catch {}
  $storage.ps.Dispose()
  $storage.ps = $null
}
AskStorage

$env:Path = "$env:Path;$env:ProgramFiles\NVIDIA Corporation\NVSMI"
$nvml = $false
try {
Add-Type -TypeDefinition @'
using System;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;
public static class BmoNvml {
  [StructLayout(LayoutKind.Sequential)] struct Utilization { public uint Gpu; public uint Memory; }
  [StructLayout(LayoutKind.Sequential)] struct Memory { public ulong Total; public ulong Free; public ulong Used; }
  [DllImport("nvml.dll")] static extern int nvmlInit_v2();
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetCount_v2(out uint count);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetHandleByIndex_v2(uint index, out IntPtr device);
  [DllImport("nvml.dll", CharSet = CharSet.Ansi)] static extern int nvmlDeviceGetName(IntPtr device, StringBuilder name, uint length);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetUtilizationRates(IntPtr device, out Utilization rates);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetTemperature(IntPtr device, int sensor, out uint celsius);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetClockInfo(IntPtr device, int type, out uint mhz);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetMaxClockInfo(IntPtr device, int type, out uint mhz);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetPowerUsage(IntPtr device, out uint milliwatts);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetEnforcedPowerLimit(IntPtr device, out uint milliwatts);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetMemoryInfo(IntPtr device, out Memory memory);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetFanSpeed(IntPtr device, out uint percent);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetTemperatureThreshold(IntPtr device, int type, out uint celsius);
  [DllImport("nvml.dll")] static extern int nvmlDeviceGetCurrentClocksThrottleReasons(IntPtr device, out ulong reasons);
  const int GRAPHICS = 0;
  const int MEM = 2;
  const int SLOWDOWN = 1;
  const ulong SW_THERMAL = 0x20;
  const ulong HW_THERMAL = 0x40;
  static IntPtr[] devices = new IntPtr[0];
  static string[] names = new string[0];
  public static bool Open() {
    uint count;
    if (nvmlInit_v2() != 0 || nvmlDeviceGetCount_v2(out count) != 0) return false;
    devices = new IntPtr[count];
    names = new string[count];
    for (uint i = 0; i < count; i++) {
      StringBuilder name = new StringBuilder(96);
      nvmlDeviceGetHandleByIndex_v2(i, out devices[i]);
      names[i] = nvmlDeviceGetName(devices[i], name, 96) == 0 ? name.ToString().Replace(",", " ") : "GPU " + i;
    }
    return true;
  }
  static string Cell(int status, double value) {
    return status == 0 ? Math.Round(value, 2).ToString(CultureInfo.InvariantCulture) : "";
  }
  static string Flag(int status, bool on) {
    return status == 0 ? (on ? "Active" : "Not Active") : "";
  }
  public static string[] Read() {
    string[] lines = new string[devices.Length];
    for (int i = 0; i < devices.Length; i++) {
      IntPtr d = devices[i];
      Utilization use; Memory memory;
      uint celsius, mhz, most, memoryMhz, watts, limit, fan, slowdown;
      ulong reasons;
      int useOk = nvmlDeviceGetUtilizationRates(d, out use);
      int celsiusOk = nvmlDeviceGetTemperature(d, 0, out celsius);
      int mhzOk = nvmlDeviceGetClockInfo(d, GRAPHICS, out mhz);
      int mostOk = nvmlDeviceGetMaxClockInfo(d, GRAPHICS, out most);
      int memoryMhzOk = nvmlDeviceGetClockInfo(d, MEM, out memoryMhz);
      int wattsOk = nvmlDeviceGetPowerUsage(d, out watts);
      int limitOk = nvmlDeviceGetEnforcedPowerLimit(d, out limit);
      int memoryOk = nvmlDeviceGetMemoryInfo(d, out memory);
      int fanOk = nvmlDeviceGetFanSpeed(d, out fan);
      int reasonsOk = nvmlDeviceGetCurrentClocksThrottleReasons(d, out reasons);
      int slowdownOk = nvmlDeviceGetTemperatureThreshold(d, SLOWDOWN, out slowdown);
      lines[i] = String.Join(", ", new string[] {
        i.ToString(CultureInfo.InvariantCulture), names[i], Cell(useOk, use.Gpu), Cell(celsiusOk, celsius), Cell(mhzOk, mhz), Cell(mostOk, most),
        Cell(memoryMhzOk, memoryMhz), Cell(wattsOk, watts / 1000.0), Cell(limitOk, limit / 1000.0),
        Cell(memoryOk, Math.Round(memory.Used / 1048576.0)), Cell(memoryOk, Math.Round(memory.Total / 1048576.0)), Cell(fanOk, fan),
        Flag(reasonsOk, (reasons & HW_THERMAL) != 0), Flag(reasonsOk, (reasons & SW_THERMAL) != 0), Cell(slowdownOk, slowdown) });
    }
    return lines;
  }
}
'@
$nvml = [BmoNvml]::Open()
} catch { $nvml = $false }

$proc = New-Object Diagnostics.PerformanceCounterCategory 'Processor Information'
$meter = $null
if ([Diagnostics.PerformanceCounterCategory]::Exists('Energy Meter')) { $meter = New-Object Diagnostics.PerformanceCounterCategory 'Energy Meter' }
$avail = New-Object Diagnostics.PerformanceCounter('Memory', 'Available KBytes')
function Sample($cat) { if ($cat) { $cat.ReadCategory() } else { $null } }
function Cooked($old, $new, $counter, $instance) {
  if ($null -eq $old -or $null -eq $new) { return $null }
  $a = $old[$counter]; $b = $new[$counter]
  if ($null -eq $a -or $null -eq $b) { return $null }
  $x = $a[$instance]; $y = $b[$instance]
  if ($null -eq $x -or $null -eq $y) { return $null }
  [Diagnostics.CounterSample]::Calculate($x.Sample, $y.Sample)
}
$lastProc = Sample $proc
$lastMeter = Sample $meter
$threads = @()
foreach ($n in $lastProc['% processor utility'].Keys) {
  if ($n -match '^(\d+),(\d+)$') { $threads += [pscustomobject]@{ name = $n; order = [int]$Matches[1] * 1024 + [int]$Matches[2] } }
}
$threads = @($threads | Sort-Object order | ForEach-Object { $_.name })
$lib = Join-Path $env:ProgramData 'bmo\lhm\LibreHardwareMonitorLib.dll'
$pc = $null
$library = 'missing'
if (Test-Path $lib) {
  try {
    Add-Type -Path $lib
    $pc = New-Object LibreHardwareMonitor.Hardware.Computer
    $pc.IsCpuEnabled = $true; $pc.IsMotherboardEnabled = $true; $pc.IsStorageEnabled = $true
    $pc.Open()
    $library = 'ok'
  } catch { $library = "failed$t$($_.Exception.Message)"; $pc = $null }
}
function Walk($h, $lines) {
  $h.Update()
  $type = [string]$h.HardwareType
  $name = if ($type -eq 'Storage') { 'disk' + $h.Identifier.ToString().Split('/')[-1] } else { $type + ':' + $h.Name }
  foreach ($s in $h.Sensors) {
    if ($null -eq $s.Value) { continue }
    $kind = [string]$s.SensorType
    if ($kind -eq 'Temperature') { $lines.Add("temp$t$name$t$($s.Name)$t$($s.Value)") }
    elseif ($kind -eq 'Fan') { $lines.Add("fan$t$name$t$($s.Name)$t$($s.Value)") }
    elseif ($kind -eq 'Power' -and $type -eq 'Cpu' -and $s.Name -eq 'Package') { $lines.Add("cpupower$t$($s.Value)") }
  }
  foreach ($sub in $h.SubHardware) { Walk $sub $lines }
}
$seldom = @()
$drives = @()
$boot = $null
$totalKb = $null
$k = 0
$next = [DateTime]::UtcNow
# Through a hub the run ends itself after four minutes, inside the five the hub gives an exec.
$end = $next.AddSeconds(240)
while ([DateTime]::UtcNow -lt $end) {
  if ($k % 30 -eq 0) {
    $seldom = New-Object Collections.Generic.List[string]
    $os = Get-CimInstance Win32_OperatingSystem
    if ($os) { $seldom.Add("os$t$($os.Caption)$t$($os.BuildNumber)$t$($os.Version)"); $boot = $os.LastBootUpTime; $totalKb = $os.TotalVisibleMemorySize }
    $cpu = @(Get-CimInstance Win32_Processor)
if ($cpu.Count -gt 0) { $seldom.Add("model$t$($cpu[0].Name.Trim())$t$(($cpu | Measure-Object NumberOfCores -Sum).Sum)$t$(($cpu | Measure-Object NumberOfLogicalProcessors -Sum).Sum)") }
$page = @(Get-CimInstance Win32_PageFileUsage)
if ($page.Count -gt 0) { $seldom.Add("swap$t$(($page | Measure-Object AllocatedBaseSize -Sum).Sum)$t$(($page | Measure-Object CurrentUsage -Sum).Sum)") }
    AskStorage
    # The run's first reading waits up to ten seconds for the disks, so a run starts with them where they answer.
    if ($k -eq 0 -and $storage.ps) { [void]$storage.done.AsyncWaitHandle.WaitOne(10000) }
  }
  $k++
  $next = $next.AddMilliseconds(1000)
  $wait = ($next - [DateTime]::UtcNow).TotalMilliseconds
  if ($wait -gt 0) { Start-Sleep -Milliseconds $wait } else { $next = [DateTime]::UtcNow }
  $nowProc = Sample $proc
  $nowMeter = Sample $meter
  $lines = New-Object Collections.Generic.List[string]
  $lines.Add('@@windows'); $lines.Add('@@host'); $lines.Add($env:COMPUTERNAME); $lines.Add('@@uptime')
  if ($boot) { $lines.Add("$(((Get-Date) - $boot).TotalSeconds)") } else { $lines.Add("$($clock.Elapsed.TotalSeconds)") }
  $lines.Add('@@win')
  foreach ($one in $seldom) { $lines.Add($one) }
  TakeStorage
  foreach ($one in $storage.lines) { $lines.Add($one) }
  if ($totalKb) { $lines.Add("mem$t$totalKb$t$($avail.RawValue)") }
  if ($boot) { $lines.Add("uptime$t$([int]((Get-Date) - $boot).TotalSeconds)") }
  $lines.Add("total$t$(Cooked $lastProc $nowProc '% processor utility' '_Total')")
  $watts = Cooked $lastMeter $nowMeter 'power' 'rapl_package0_pkg'
  if ($null -ne $watts) { $lines.Add("power$t$($watts / 1000)") }
  $index = 0
  foreach ($n in $threads) {
    $use = Cooked $lastProc $nowProc '% processor utility' $n
    $freq = Cooked $lastProc $nowProc 'processor frequency' $n
    $perf = Cooked $lastProc $nowProc '% processor performance' $n
    $lines.Add("cpu$t$index$t$use$t$($freq * $perf / 100)")
    $index++
  }
  $lastProc = $nowProc
  $lastMeter = $nowMeter
  $lines.Add("lhm$t$library")
  # Reading a drive's sensors opens the drive, so the disks' are read every thirty readings and carried between.
  if ($pc) {
    if ($k % 30 -eq 1) { $drives = New-Object Collections.Generic.List[string] }
    foreach ($h in $pc.Hardware) {
      if ([string]$h.HardwareType -ne 'Storage') { Walk $h $lines } elseif ($k % 30 -eq 1) { Walk $h $drives }
    }
    foreach ($one in $drives) { $lines.Add($one) }
  }
  $lines.Add('@@gpu'); if ($nvml) { foreach ($g in [BmoNvml]::Read()) { $lines.Add($g) } }; $lines.Add('@@end')
  try { $out.Write(($lines -join $nl) + $nl); $out.Flush() } catch { break }
}
if ($pc) { $pc.Close() }
# A storage query that never answered still holds its runspace, and the run ends whatever is under way.
[Environment]::Exit(0)
