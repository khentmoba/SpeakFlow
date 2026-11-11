<#
.SYNOPSIS
  Measures SpeakFlow startup latency and idle cost from the outside.

.DESCRIPTION
  Launches the built app with `--minimized`, reads the `startup:` markers the app logs,
  and optionally samples the app shell's idle CPU and memory. The ASR engine (python) is
  reported apart from the shell because its cost belongs to the engine.

  The window must stay hidden for the idle numbers to describe the state this app sits in
  between dictations: `--minimized` only hides it when the profile has start-minimized
  (`autostart_minimized`) enabled. The app logs which branch it took and the script fails
  loudly when the window was shown.

  Build the profile you want to compare. Release is what users run, but it needs a
  non-empty engine archive (see src-tauri/build.rs), so debug builds are fine for relative
  comparisons and release builds for shipped numbers:
    pnpm build
    cargo build --manifest-path src-tauri/Cargo.toml --release --locked --features tauri/custom-protocol

.EXAMPLE
  pwsh -File scripts/measure_startup.ps1 -Exe src-tauri/target/release/speak-flow.exe -Runs 3 -IdleSeconds 20
#>
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$Runs = 3,
  [int]$IdleSeconds = 0,
  [int]$WarmSeconds = 40,
  [int]$TimeoutSeconds = 60
)

$ErrorActionPreference = 'Stop'
$exePath = (Resolve-Path $Exe).Path
$logPath = Join-Path $env:LOCALAPPDATA 'com.speak-flow.desktop\logs\app.log'

function Get-ProcessTree([int]$RootId) {
  $all = @(Get-CimInstance -ClassName Win32_Process -Property ProcessId, ParentProcessId)
  $seen = New-Object 'System.Collections.Generic.HashSet[int]'
  $queue = New-Object 'System.Collections.Generic.Queue[int]'
  [void]$seen.Add($RootId)
  $queue.Enqueue($RootId)
  while ($queue.Count -gt 0) {
    $current = $queue.Dequeue()
    foreach ($child in $all) {
      if ($child.ParentProcessId -eq $current -and -not $seen.Contains([int]$child.ProcessId)) {
        [void]$seen.Add([int]$child.ProcessId)
        $queue.Enqueue([int]$child.ProcessId)
      }
    }
  }
  return @($seen)
}

# A child can exit between enumeration and the property read; skip it instead of aborting.
function Get-Snapshot([int]$RootId) {
  $snapshot = @{}
  foreach ($id in (Get-ProcessTree $RootId)) {
    try {
      $process = Get-Process -Id $id -ErrorAction Stop
      $snapshot[$id] = [pscustomobject]@{
        Name    = $process.ProcessName
        CpuMs   = $process.TotalProcessorTime.TotalMilliseconds
        Private = $process.PrivateMemorySize64
        Working = $process.WorkingSet64
      }
    } catch {
      continue
    }
  }
  return $snapshot
}

# The log plugin rotates app.log at startup, which can leave a pre-launch offset past EOF.
function Read-NewLogText([ref]$Offset) {
  if (-not (Test-Path $logPath)) { return '' }
  if ((Get-Item $logPath).Length -lt $Offset.Value) { $Offset.Value = 0 }
  $stream = [System.IO.File]::Open($logPath, 'Open', 'Read', 'ReadWrite')
  try {
    [void]$stream.Seek($Offset.Value, 'Begin')
    return (New-Object System.IO.StreamReader($stream)).ReadToEnd()
  } finally {
    $stream.Dispose()
  }
}

if (@(Get-Process -Name 'speak-flow' -ErrorAction SilentlyContinue).Count -gt 0) {
  throw 'speak-flow is already running; quit it before measuring.'
}

for ($run = 1; $run -le $Runs; $run++) {
  $offset = 0
  if (Test-Path $logPath) { $offset = (Get-Item $logPath).Length }

  $process = Start-Process -FilePath $exePath -ArgumentList '--minimized' -WindowStyle Hidden -PassThru
  $rootId = $process.Id
  try {
    $pageLoadedMs = $null
    $setupDoneMs = $null
    $windowHidden = $false
    $windowShown = $false
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline -and ($null -eq $pageLoadedMs -or $null -eq $setupDoneMs)) {
      Start-Sleep -Milliseconds 100
      $text = Read-NewLogText ([ref]$offset)
      if ($text -match 'startup: main window page loaded in (\d+)ms') { $pageLoadedMs = [int]$Matches[1] }
      if ($text -match 'startup: setup completed in (\d+)ms') { $setupDoneMs = [int]$Matches[1] }
      if ($text -match 'startup: main window stays hidden') { $windowHidden = $true }
      if ($text -match 'startup: main window shown') { $windowShown = $true }
    }

    if ($null -eq $pageLoadedMs -or $null -eq $setupDoneMs) {
      throw "No startup marker within ${TimeoutSeconds}s; check $logPath."
    }
    if ($windowShown -or -not $windowHidden) {
      throw 'The app window was shown, so these numbers describe a visible window. Enable Settings > Start minimized (autostart_minimized) and rerun.'
    }

    $result = [ordered]@{
      run          = $run
      pageLoadedMs = $pageLoadedMs
      setupDoneMs  = $setupDoneMs
    }

    if ($IdleSeconds -gt 0) {
      Start-Sleep -Seconds $WarmSeconds
      $before = Get-Snapshot $rootId
      Start-Sleep -Seconds $IdleSeconds
      $after = Get-Snapshot $rootId

      # The local ASR engine is a python child; report it apart from the shell.
      $shellIds = @($after.Keys | Where-Object { $after[$_].Name -ne 'python' })
      $cpuMs = 0.0
      foreach ($id in $shellIds) {
        # A process that appeared during the window starts from zero.
        $baselineMs = if ($before.ContainsKey($id)) { $before[$id].CpuMs } else { 0.0 }
        $cpuMs += ($after[$id].CpuMs - $baselineMs)
      }
      $cores = [int](Get-CimInstance -ClassName Win32_ComputerSystem).NumberOfLogicalProcessors
      $result.idleCpuPercentOfMachine = [math]::Round(100.0 * $cpuMs / ($IdleSeconds * 1000.0 * $cores), 3)
      $result.shellPrivateMB = [math]::Round((($shellIds | ForEach-Object { $after[$_].Private } | Measure-Object -Sum).Sum) / 1MB, 1)
      $result.shellWorkingSetMB = [math]::Round((($shellIds | ForEach-Object { $after[$_].Working } | Measure-Object -Sum).Sum) / 1MB, 1)
      $result.webview2Processes = @($shellIds | Where-Object { $after[$_].Name -eq 'msedgewebview2' }).Count
      $result.engineLoaded = @($after.Keys | Where-Object { $after[$_].Name -eq 'python' }).Count -gt 0
    }

    [pscustomobject]$result | ConvertTo-Json -Compress
  } finally {
    Stop-Process -Id (Get-ProcessTree $rootId) -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 3
  }
}
