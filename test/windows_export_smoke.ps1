param(
  [Parameter(Mandatory = $true)]
  [string]$Executable
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class BoxyWindowOwner {
  [DllImport("user32.dll", SetLastError = true)]
  public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
}
'@

$source = (Resolve-Path -LiteralPath $Executable).Path
$tempBase = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$testRoot = Join-Path $tempBase "boxy-export-smoke-$([Guid]::NewGuid().ToString('N'))"
$appDir = Join-Path $testRoot 'app'
$sessionRoot = Join-Path $testRoot 'SV2'
$sessionDir = Join-Path $sessionRoot 'Config'
$tempRoot = Join-Path $testRoot 'isolated-temp'
$fixtureState = Join-Path $testRoot 'fixture-state.json'
$fixturePort = Join-Path $testRoot 'fixture-port.txt'
$fixtureScript = Join-Path $PSScriptRoot 'fixtures/tauri-webview-server.mjs'
$fixtureExecutable = Join-Path $appDir 'boxy-tauri-export-smoke.exe'
$sessionPath = Join-Path $sessionDir 'session.bin'
$outputPath = Join-Path $testRoot 'boxy-export-fixture.txt'
$expectedContent = "boxy-export-fixture`n"
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

function Find-OwnedSaveDialog([uint32]$ProcessId) {
  $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
    [System.Windows.Automation.TreeScope]::Children,
    [System.Windows.Automation.PropertyCondition]::new(
      [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
      [System.Windows.Automation.ControlType]::Window
    )
  )
  foreach ($window in $windows) {
    try {
      $handle = [IntPtr]$window.Current.NativeWindowHandle
      if ($handle -eq [IntPtr]::Zero) { continue }
      [uint32]$owner = 0
      [void][BoxyWindowOwner]::GetWindowThreadProcessId($handle, [ref]$owner)
      if ($owner -ne $ProcessId) { continue }

      $edits = $window.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new(
          [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
          [System.Windows.Automation.ControlType]::Edit
        )
      )
      $fileName = $null
      foreach ($edit in $edits) {
        if ($edit.Current.AutomationId -eq '1001' -or $edit.Current.Name -match '^File name:?$') {
          $fileName = $edit
          break
        }
      }
      if ($null -eq $fileName) { continue }

      $buttons = $window.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new(
          [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
          [System.Windows.Automation.ControlType]::Button
        )
      )
      $save = $null
      foreach ($button in $buttons) {
        if ($button.Current.AutomationId -eq '1' -or $button.Current.Name -match '^Save(?:\s|$)') {
          $save = $button
          break
        }
      }
      if ($null -ne $save) { return @{ Window = $window; FileName = $fileName; Save = $save } }
    } catch {
      continue
    }
  }
  return $null
}

function Close-OwnedSaveDialog([uint32]$ProcessId) {
  $owned = Find-OwnedSaveDialog $ProcessId
  if ($null -eq $owned) { return }
  $buttons = $owned.Window.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.PropertyCondition]::new(
      [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
      [System.Windows.Automation.ControlType]::Button
    )
  )
  foreach ($button in $buttons) {
    if ($button.Current.Name -match '^Cancel(?:\s|$)') {
      $pattern = $null
      if ($button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke()
        return
      }
    }
  }
}

New-Item -ItemType Directory -Path $appDir, $sessionDir, $tempRoot -Force | Out-Null
Copy-Item -LiteralPath $source -Destination $fixtureExecutable
[IO.File]::WriteAllBytes($sessionPath, [byte[]](1..16))
$signerBefore = if (Test-Path -LiteralPath $signerPath -PathType Leaf) {
  (Get-FileHash -LiteralPath $signerPath -Algorithm SHA256).Hash
} else { $null }
$appLocalDataBefore = Get-PathInventory $appLocalDataPath

$node = (Get-Command node -ErrorAction Stop).Source
$fixtureStart = [Diagnostics.ProcessStartInfo]::new()
$fixtureStart.FileName = $node
$fixtureStart.UseShellExecute = $false
$fixtureStart.CreateNoWindow = $true
foreach ($argument in @($fixtureScript, '--state-file', $fixtureState, '--port-file', $fixturePort, '--enable-export')) {
  $fixtureStart.ArgumentList.Add($argument)
}
$fixtureProcess = [Diagnostics.Process]::Start($fixtureStart)
$child = $null
$normalClose = $false
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
    if ($child.HasExited) { throw "Boxy exited before the Blob export was started (exit $($child.ExitCode))." }
    if (Test-Path -LiteralPath $fixtureState) {
      $state = Get-Content -LiteralPath $fixtureState -Raw | ConvertFrom-Json
      if ($state.requestErrors.Count -gt 0) {
        throw "The protocol fixture rejected a connector request: $($state.requestErrors -join ', ')."
      }
      if ($state.exportStarted) { break }
    }
    Start-Sleep -Milliseconds 200
  }
  if ($null -eq $state -or -not $state.editorLoaded -or -not $state.readyFetch -or -not $state.exportStarted) {
    throw 'The embedded fixture page did not pass readiness and trigger the Blob download.'
  }

  $saveDeadline = [DateTime]::UtcNow.AddSeconds(30)
  $saveDialog = $null
  while ([DateTime]::UtcNow -lt $saveDeadline) {
    $saveDialog = Find-OwnedSaveDialog ([uint32]$child.Id)
    if ($null -ne $saveDialog) { break }
    if ($child.HasExited) { throw "Boxy exited before its save dialog appeared (exit $($child.ExitCode))." }
    Start-Sleep -Milliseconds 200
  }
  if ($null -eq $saveDialog) { throw 'No process-owned native save dialog appeared for the Blob download.' }

  $valuePattern = $null
  if (-not $saveDialog.FileName.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$valuePattern)) {
    throw 'The process-owned save dialog filename control does not expose UI Automation ValuePattern.'
  }
  $valuePattern.SetValue($outputPath)
  $invokePattern = $null
  if (-not $saveDialog.Save.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invokePattern)) {
    throw 'The process-owned save button does not expose UI Automation InvokePattern.'
  }
  $invokePattern.Invoke()

  $writeDeadline = [DateTime]::UtcNow.AddSeconds(20)
  while ([DateTime]::UtcNow -lt $writeDeadline -and -not (Test-Path -LiteralPath $outputPath -PathType Leaf)) {
    if ($child.HasExited) { throw "Boxy exited before the Blob file was saved (exit $($child.ExitCode))." }
    Start-Sleep -Milliseconds 200
  }
  if (-not (Test-Path -LiteralPath $outputPath -PathType Leaf)) { throw 'The Blob download did not create the selected output file.' }
  $actualContent = [IO.File]::ReadAllText($outputPath, [Text.UTF8Encoding]::new($false))
  if ($actualContent -cne $expectedContent) { throw 'The exported Blob file contents differ from the known fixture bytes.' }

  if (-not $child.CloseMainWindow()) { throw 'Windows did not accept WM_CLOSE for the Boxy editor window.' }
  if (-not $child.WaitForExit(30000)) { throw 'Boxy did not exit after the normal window-close request.' }
  if ($child.ExitCode -ne 0) { throw "Boxy exited with error $($child.ExitCode) during normal window close." }
  $normalClose = $true

  $leftovers = @(Get-ChildItem -LiteralPath $tempRoot -Force -ErrorAction SilentlyContinue)
  if ($leftovers.Count -ne 0) { throw "Temporary Boxy runtime data remained after normal close: $($leftovers.Name -join ', ')." }
  $signerAfter = if (Test-Path -LiteralPath $signerPath -PathType Leaf) {
    (Get-FileHash -LiteralPath $signerPath -Algorithm SHA256).Hash
  } else { $null }
  if ($signerBefore -ne $signerAfter) { throw 'The smoke run created or changed a persistent AppData signing key.' }
  $appLocalDataAfter = Get-PathInventory $appLocalDataPath
  if ($appLocalDataBefore -cne $appLocalDataAfter) { throw 'The smoke run created or changed Tauri app-local data.' }

  Write-Output 'Blob anchor download opened the process-owned save dialog, saved the expected bytes, and normal close cleared temporary runtime data.'
}
finally {
  if ($null -ne $child) {
    if (-not $child.HasExited) {
      Close-OwnedSaveDialog ([uint32]$child.Id)
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
  if (-not $normalClose) { Write-Output 'Smoke failed before normal window close; only test-owned fixture resources were cleaned.' }
}
