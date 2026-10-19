param(
  [Parameter(Mandatory = $true)]
  [string]$Executable
)

$ErrorActionPreference = 'Stop'

$source = (Resolve-Path -LiteralPath $Executable).Path
$tempBase = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$testRoot = Join-Path $tempBase "boxy-tauri-smoke-$([Guid]::NewGuid().ToString('N'))"
$appDir = Join-Path $testRoot 'app'
$sessionRoot = Join-Path $testRoot 'SV2'
$sessionDir = Join-Path $sessionRoot 'Config'
$tempRoot = Join-Path $testRoot 'isolated-temp'
$fixtureState = Join-Path $testRoot 'fixture-state.json'
$fixturePort = Join-Path $testRoot 'fixture-port.txt'
$fixtureScript = Join-Path $PSScriptRoot 'fixtures/tauri-webview-server.mjs'
$fixtureExecutable = Join-Path $appDir 'boxy-tauri-smoke.exe'
$sessionPath = Join-Path $sessionDir 'session.bin'
$markerPath = Join-Path $sessionRoot 'smoke-marker.txt'
$signerPath = Join-Path $env:LOCALAPPDATA 'SynthVCopilot/Boxy/device-signing-key'
$appLocalDataPath = Join-Path $env:LOCALAPPDATA 'app.boxy.bridge'

function Get-PathInventory([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path -PathType Container)) { return $null }
  return (Get-ChildItem -LiteralPath $Path -Force -Recurse | Sort-Object FullName | ForEach-Object {
    $relative = $_.FullName.Substring($Path.Length).TrimStart([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    if ($_.PSIsContainer) { "$relative|dir|$($_.LastWriteTimeUtc.Ticks)" }
    else { "$relative|$($_.Length)|$($_.LastWriteTimeUtc.Ticks)" }
  }) -join "`n"
}

New-Item -ItemType Directory -Path $appDir, $sessionDir, $tempRoot -Force | Out-Null
Copy-Item -LiteralPath $source -Destination $fixtureExecutable
[IO.File]::WriteAllBytes($sessionPath, [byte[]](1..16))
[IO.File]::WriteAllText($markerPath, 'boxy-smoke-marker', [Text.UTF8Encoding]::new($false))

$signerBefore = if (Test-Path -LiteralPath $signerPath -PathType Leaf) {
  (Get-FileHash -LiteralPath $signerPath -Algorithm SHA256).Hash
} else { $null }
$appLocalDataBefore = Get-PathInventory $appLocalDataPath

$node = (Get-Command node -ErrorAction Stop).Source
$fixtureStart = [Diagnostics.ProcessStartInfo]::new()
$fixtureStart.FileName = $node
$fixtureStart.UseShellExecute = $false
$fixtureStart.CreateNoWindow = $true
$fixtureStart.ArgumentList.Add($fixtureScript)
$fixtureStart.ArgumentList.Add('--state-file')
$fixtureStart.ArgumentList.Add($fixtureState)
$fixtureStart.ArgumentList.Add('--port-file')
$fixtureStart.ArgumentList.Add($fixturePort)
$fixtureProcess = [Diagnostics.Process]::Start($fixtureStart)
$child = $null
$naturalClose = $false
try {
  $serverDeadline = [DateTime]::UtcNow.AddSeconds(15)
  while ([DateTime]::UtcNow -lt $serverDeadline -and -not (Test-Path -LiteralPath $fixturePort)) {
    if ($fixtureProcess.HasExited) { throw 'The local protocol fixture stopped before it became ready.' }
    Start-Sleep -Milliseconds 100
  }
  if (-not (Test-Path -LiteralPath $fixturePort)) { throw 'The local protocol fixture did not become ready.' }
  $serverUrl = "http://127.0.0.1:$((Get-Content -LiteralPath $fixturePort -Raw).Trim())"

  $startInfo = [Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $fixtureExecutable
  $startInfo.WorkingDirectory = $appDir
  $startInfo.UseShellExecute = $false
  $startInfo.Environment['TEMP'] = $tempRoot
  $startInfo.Environment['TMP'] = $tempRoot
  foreach ($argument in @('--server', $serverUrl, '--session', $sessionPath)) {
    $startInfo.ArgumentList.Add($argument)
  }
  $child = [Diagnostics.Process]::Start($startInfo)

  $deadline = [DateTime]::UtcNow.AddSeconds(60)
  $state = $null
  while ([DateTime]::UtcNow -lt $deadline) {
    if ($child.HasExited) { throw "Boxy exited before the embedded cloud page and signed read receipt were observed (exit $($child.ExitCode))." }
    if (Test-Path -LiteralPath $fixtureState) {
      $state = Get-Content -LiteralPath $fixtureState -Raw | ConvertFrom-Json
      if ($state.requestErrors.Count -gt 0) {
        throw "The protocol fixture rejected a connector request: $($state.requestErrors -join ', ')."
      }
      if ($state.readyFetch -and $state.signedTaskReceipts -gt 0) { break }
    }
    Start-Sleep -Milliseconds 200
  }

  if ($null -eq $state -or -not $state.editorLoaded -or -not $state.readyFetch) {
    throw 'The embedded cloud page did not load and execute its /ready fetch.'
  }
  if ($state.startProtocolVersion -ne 3 -or $state.signedPolls -lt 1) {
    throw 'The local fixture did not observe protocol-v3 start and signed poll.'
  }
  if (-not $state.nativeIpcDenied) { throw 'The remote editor could access a Tauri native IPC command.' }
  if ($state.signedTaskReceipts -ne 1 -or -not $state.readReceiptValid) {
    throw 'The constrained read_files task did not produce the expected signed receipt.'
  }
  if ($child.MainWindowHandle -eq [IntPtr]::Zero -or $child.MainWindowTitle -ne 'Boxy') {
    throw "Expected the visible Boxy editor window, got title '$($child.MainWindowTitle)'."
  }
  if (Get-ChildItem -LiteralPath $appDir -Force | Where-Object { $_.Name -like '*.WebView2' -or $_.Extension -ieq '.dll' }) {
    throw 'The portable executable folder contains a WebView2 profile or adjacent loader DLL.'
  }

  if (-not $child.CloseMainWindow()) { throw 'Windows did not accept WM_CLOSE for the Boxy editor window.' }
  if (-not $child.WaitForExit(30000)) { throw 'Boxy did not exit after the normal window-close request.' }
  if ($child.ExitCode -ne 0) { throw "Boxy exited with error $($child.ExitCode) during normal window close." }
  $naturalClose = $true

  $leftovers = @(Get-ChildItem -LiteralPath $tempRoot -Force -ErrorAction SilentlyContinue)
  if ($leftovers.Count -ne 0) {
    throw "Temporary Boxy runtime data remained after normal close: $($leftovers.Name -join ', ')."
  }
  $signerAfter = if (Test-Path -LiteralPath $signerPath -PathType Leaf) {
    (Get-FileHash -LiteralPath $signerPath -Algorithm SHA256).Hash
  } else { $null }
  if ($signerBefore -ne $signerAfter) {
    throw 'The smoke run created or changed a persistent AppData signing key.'
  }
  $appLocalDataAfter = Get-PathInventory $appLocalDataPath
  if ($appLocalDataBefore -cne $appLocalDataAfter) {
    throw 'The smoke run created or changed Tauri app-local data instead of using its temporary profile.'
  }

  Write-Output 'Embedded cloud page executed JavaScript; protocol-v3 signed poll, read_files receipt, and WM_CLOSE cleanup passed.'
}
finally {
  if ($null -ne $child) {
    if (-not $child.HasExited) {
      if (-not $child.CloseMainWindow()) {
        Write-Warning 'The failed smoke did not accept a normal close request; stopping only the test-owned process for cleanup.'
        $child.Kill()
      }
      $child.WaitForExit(5000) | Out-Null
    }
    $child.Dispose()
  }
  if (-not $fixtureProcess.HasExited) {
    $fixtureProcess.Kill()
    $fixtureProcess.WaitForExit(5000) | Out-Null
  }
  $fixtureProcess.Dispose()
  Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
  if (-not $naturalClose) { Write-Output 'Smoke failed before normal window close; test-owned fixture resources were cleaned.' }
}
