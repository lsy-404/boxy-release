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
public static class BoxySelectionWindowOwner {
  [DllImport("user32.dll", SetLastError = true)]
  public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
}
'@

$source = (Resolve-Path -LiteralPath $Executable).Path
$tempBase = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$testRoot = Join-Path $tempBase "boxy-service-selection-$([Guid]::NewGuid().ToString('N'))"
$appDir = Join-Path $testRoot 'app'
$sessionRoot = Join-Path $testRoot 'SV2'
$sessionDir = Join-Path $sessionRoot 'Config'
$tempRoot = Join-Path $testRoot 'isolated-temp'
$fixtureState = Join-Path $testRoot 'fixture-state.json'
$fixturePort = Join-Path $testRoot 'fixture-port.txt'
$rejectState = Join-Path $testRoot 'reject-state.json'
$rejectPort = Join-Path $testRoot 'reject-port.txt'
$fixtureScript = Join-Path $PSScriptRoot 'fixtures/tauri-webview-server.mjs'
$fixtureExecutable = Join-Path $appDir 'boxy-service-smoke.exe'
$sessionPath = Join-Path $sessionDir 'session.bin'
$invalidSessionPath = Join-Path $sessionDir 'invalid-session.bin'
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

function Get-OwnedWindows([uint32]$ProcessId) {
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
      [void][BoxySelectionWindowOwner]::GetWindowThreadProcessId($handle, [ref]$owner)
      if ($owner -eq $ProcessId) { $window }
    } catch {}
  }
}

function Find-ById($Window, [string]$AutomationId) {
  return $Window.FindFirst(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.PropertyCondition]::new(
      [System.Windows.Automation.AutomationElement]::AutomationIdProperty,
      $AutomationId
    )
  )
}

function Find-ServiceSelection($ProcessId) {
  foreach ($window in (Get-OwnedWindows ([uint32]$ProcessId))) {
    $input = Find-ById $window 'service-url'
    $connect = Find-ById $window 'connect'
    if ($null -ne $input -and $null -ne $connect) {
      return @{ Window = $window; Input = $input; Connect = $connect; Error = (Find-ById $window 'error') }
    }

    $edits = $window.FindAll(
      [System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Edit
      )
    )
    $buttons = $window.FindAll(
      [System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Button
      )
    )
    if ($edits.Count -eq 1 -and $buttons.Count -eq 1) {
      return @{ Window = $window; Input = $edits[0]; Connect = $buttons[0]; Error = (Find-ById $window 'error') }
    }
  }
  return $null
}

function Get-ControlValue($Element) {
  if ($null -eq $Element) { return '' }
  $pattern = $null
  if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
    return [string]$pattern.Current.Value
  }
  return [string]$Element.Current.Name
}

function Get-SelectionError($Selection) {
  $parts = [System.Collections.Generic.List[string]]::new()
  if ($null -ne $Selection.Error) {
    $value = Get-ControlValue $Selection.Error
    if (-not [string]::IsNullOrWhiteSpace($value)) { $parts.Add($value) }
  }
  $texts = $Selection.Window.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.PropertyCondition]::new(
      [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
      [System.Windows.Automation.ControlType]::Text
    )
  )
  foreach ($text in $texts) {
    if ($text.Current.AutomationId -eq 'error' -or $text.Current.Name -match 'valid service URL|HTTPS|credentials|path, query|authorization service|rejected the request') {
      if (-not [string]::IsNullOrWhiteSpace($text.Current.Name)) { $parts.Add($text.Current.Name) }
    }
  }
  return ($parts -join ' ')
}

function Set-ControlValue($Element, [string]$Value) {
  $pattern = $null
  if (-not $Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
    throw "The service URL input does not expose UI Automation ValuePattern (AutomationId '$($Element.Current.AutomationId)')."
  }
  $pattern.SetValue($Value)
}

function Invoke-Control($Element) {
  $pattern = $null
  if (-not $Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    throw "The service connect button does not expose UI Automation InvokePattern (AutomationId '$($Element.Current.AutomationId)')."
  }
  $pattern.Invoke()
}

function Find-OwnedFileDialog([uint32]$ProcessId) {
  foreach ($window in (Get-OwnedWindows $ProcessId)) {
    try {
      $edits = $window.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new(
          [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
          [System.Windows.Automation.ControlType]::Edit
        )
      )
      foreach ($edit in $edits) {
        if ($edit.Current.AutomationId -eq '1001' -or $edit.Current.Name -match '^(File name|文件名):?$') {
          return $window
        }
      }
    } catch {}
  }
  return $null
}

function Close-OwnedFileDialog([uint32]$ProcessId) {
  $window = Find-OwnedFileDialog $ProcessId
  if ($null -eq $window) { return }
  $buttons = $window.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.PropertyCondition]::new(
      [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
      [System.Windows.Automation.ControlType]::Button
    )
  )
  foreach ($button in $buttons) {
    if ($button.Current.Name -match '^(Cancel|取消)') {
      $pattern = $null
      if ($button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke()
        return
      }
    }
  }
}

function Start-Boxy([string[]]$Arguments) {
  $startInfo = [Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $fixtureExecutable
  $startInfo.WorkingDirectory = $appDir
  $startInfo.UseShellExecute = $false
  $startInfo.Environment['TEMP'] = $tempRoot
  $startInfo.Environment['TMP'] = $tempRoot
  [void]$startInfo.Environment.Remove('BOXY_API_URL')
  foreach ($argument in $Arguments) { $startInfo.ArgumentList.Add($argument) }
  return [Diagnostics.Process]::Start($startInfo)
}

function Wait-ServiceSelection([Diagnostics.Process]$Process, [int]$TimeoutSeconds) {
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  while ([DateTime]::UtcNow -lt $deadline) {
    if ($Process.HasExited) { throw "Boxy exited before displaying the service selection page (exit $($Process.ExitCode))." }
    $dialog = Find-OwnedFileDialog ([uint32]$Process.Id)
    if ($null -ne $dialog) { throw 'Boxy opened a native session file dialog before service selection was confirmed.' }
    $selection = Find-ServiceSelection $Process.Id
    if ($null -ne $selection) { return $selection }
    Start-Sleep -Milliseconds 150
  }
  throw 'The blank local service selection page did not become accessible through UI Automation.'
}

function Wait-ServiceError([Diagnostics.Process]$Process, [int]$TimeoutSeconds) {
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  $lastSelection = $null
  while ([DateTime]::UtcNow -lt $deadline) {
    if ($Process.HasExited) { throw "Boxy exited before returning to service selection (exit $($Process.ExitCode))." }
    $selection = Find-ServiceSelection $Process.Id
    if ($null -ne $selection) {
      $lastSelection = $selection
      $message = Get-SelectionError $selection
      if (-not [string]::IsNullOrWhiteSpace($message)) { return $selection }
    }
    Start-Sleep -Milliseconds 150
  }
  $details = if ($null -eq $lastSelection) { 'selection controls not found' } else {
    $controls = $lastSelection.Window.FindAll(
      [System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.Condition]::TrueCondition
    )
    (@($controls | ForEach-Object { "id=$($_.Current.AutomationId),type=$($_.Current.ControlType.ProgrammaticName),name=$($_.Current.Name),enabled=$($_.Current.IsEnabled)" }) -join '; ')
  }
  throw "The failed connection did not return an accessible error to the local service selection page. UIA: $details"
}

function Start-ProtocolFixture([string]$StatePath, [string]$PortPath, [switch]$RejectStart) {
  $node = (Get-Command node -ErrorAction Stop).Source
  $startInfo = [Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $node
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $true
  foreach ($argument in @($fixtureScript, '--state-file', $StatePath, '--port-file', $PortPath)) {
    $startInfo.ArgumentList.Add($argument)
  }
  $startInfo.ArgumentList.Add('--probe-select-service')
  if ($RejectStart) { $startInfo.ArgumentList.Add('--reject-operation-start') }
  $process = [Diagnostics.Process]::Start($startInfo)
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  while ([DateTime]::UtcNow -lt $deadline -and -not (Test-Path -LiteralPath $PortPath)) {
    if ($process.HasExited) { throw 'The local protocol fixture stopped before it became ready.' }
    Start-Sleep -Milliseconds 100
  }
  if (-not (Test-Path -LiteralPath $PortPath)) { throw 'The local protocol fixture did not become ready.' }
  return $process
}

function Wait-FileFixtureState([Diagnostics.Process]$Process) {
  $deadline = [DateTime]::UtcNow.AddSeconds(90)
  while ([DateTime]::UtcNow -lt $deadline) {
    if ($Process.HasExited) { throw "Boxy exited before the selected service completed its signed fixture receipt (exit $($Process.ExitCode))." }
    if (Test-Path -LiteralPath $fixtureState) {
      $state = Get-Content -LiteralPath $fixtureState -Raw | ConvertFrom-Json
      if ($state.requestErrors.Count -gt 0) {
        throw "The protocol fixture rejected a connector request: $($state.requestErrors -join ', ')."
      }
      if ($state.editorLoaded -and $state.readyFetch -and $state.signedTaskReceipts -gt 0) { return $state }
    }
    Start-Sleep -Milliseconds 200
  }
  throw 'The selected loopback service did not load its cloud page and signed file receipt.'
}

function Assert-NormalExit([Diagnostics.Process]$Process, [string]$Phase) {
  if (-not $Process.CloseMainWindow()) { throw "Windows did not accept WM_CLOSE for Boxy during $Phase." }
  if (-not $Process.WaitForExit(30000)) { throw "Boxy did not exit after the normal window-close request during $Phase." }
  if ($Process.ExitCode -ne 0) { throw "Boxy exited with error $($Process.ExitCode) during $Phase." }
  $leftovers = @(Get-ChildItem -LiteralPath $tempRoot -Force -ErrorAction SilentlyContinue)
  if ($leftovers.Count -ne 0) { throw "Temporary Boxy runtime data remained after ${Phase}: $($leftovers.Name -join ', ')." }
}

New-Item -ItemType Directory -Path $appDir, $sessionDir, $tempRoot -Force | Out-Null
Copy-Item -LiteralPath $source -Destination $fixtureExecutable
[IO.File]::WriteAllBytes($sessionPath, [byte[]](1..16))
[IO.File]::WriteAllText($markerPath, 'boxy-smoke-marker', [Text.UTF8Encoding]::new($false))
$signerBefore = if (Test-Path -LiteralPath $signerPath -PathType Leaf) {
  (Get-FileHash -LiteralPath $signerPath -Algorithm SHA256).Hash
} else { $null }
$appLocalDataBefore = Get-PathInventory $appLocalDataPath
$fixtureProcess = $null
$rejectFixtureProcess = $null
$unselectedProcess = $null
$connectedProcess = $null
$invalidSessionLock = $null
$normalUnselectedExit = $false
$normalConnectedExit = $false
try {
  [IO.File]::WriteAllBytes($invalidSessionPath, [byte[]](1, 2, 3, 4))
  $invalidSessionLock = [IO.File]::Open($invalidSessionPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
  $unselectedProcess = Start-Boxy @('--session', $invalidSessionPath)
  $unselected = Wait-ServiceSelection $unselectedProcess 45
  $entered = Get-ControlValue $unselected.Input
  if (-not [string]::IsNullOrWhiteSpace($entered)) { throw "Ordinary release filename unexpectedly prefilled service URL '$entered'." }
  if (-not [string]::IsNullOrWhiteSpace((Get-SelectionError $unselected))) {
    throw 'The invalid session path produced an error before service selection.'
  }
  Start-Sleep -Milliseconds 700
  if ($null -ne (Find-OwnedFileDialog ([uint32]$unselectedProcess.Id))) {
    throw 'The invalid session path opened a native file dialog before service selection.'
  }
  Assert-NormalExit $unselectedProcess 'unselected service exit'
  $normalUnselectedExit = $true
  $invalidSessionLock.Dispose()
  $invalidSessionLock = $null
  $unselectedProcess.Dispose()
  $unselectedProcess = $null

  $rejectFixtureProcess = Start-ProtocolFixture $rejectState $rejectPort -RejectStart
  $rejectUrl = "http://127.0.0.1:$((Get-Content -LiteralPath $rejectPort -Raw).Trim())"

  $connectedProcess = Start-Boxy @('--session', $sessionPath)
  $connected = Wait-ServiceSelection $connectedProcess 45
  if (-not [string]::IsNullOrWhiteSpace((Get-ControlValue $connected.Input))) {
    throw 'The ordinary release filename unexpectedly prefilled the service URL on the connection run.'
  }
  Set-ControlValue $connected.Input 'not-a-service-url'
  Start-Sleep -Milliseconds 150
  Invoke-Control $connected.Connect
  $connected = Wait-ServiceError $connectedProcess 15
  $invalidError = Get-SelectionError $connected
  if ($invalidError -notmatch 'valid service URL') {
    throw "An invalid URL did not produce the expected local validation error. UIA text: $invalidError"
  }

  Set-ControlValue $connected.Input $rejectUrl
  Start-Sleep -Milliseconds 150
  Invoke-Control $connected.Connect
  $connected = Wait-ServiceError $connectedProcess 45
  $startError = Get-SelectionError $connected
  if ($startError -notmatch '503|fixture_start_rejected|status code|HTTP') {
    throw "A failed operation start did not return an actionable error to the local chooser. UIA text: $startError"
  }
  $failedStart = Get-Content -LiteralPath $rejectState -Raw | ConvertFrom-Json
  if ($failedStart.operationStartCount -ne 1) { throw 'The rejecting service did not receive exactly one start request.' }

  $rejectFixtureProcess.Kill()
  $rejectFixtureProcess.WaitForExit(5000) | Out-Null
  $rejectFixtureProcess.Dispose()
  $rejectFixtureProcess = $null

  $fixtureProcess = Start-ProtocolFixture $fixtureState $fixturePort
  $serverUrl = "http://127.0.0.1:$((Get-Content -LiteralPath $fixturePort -Raw).Trim())"
  Set-ControlValue $connected.Input $serverUrl
  Start-Sleep -Milliseconds 250
  Invoke-Control $connected.Connect

  $state = Wait-FileFixtureState $connectedProcess
  Start-Sleep -Seconds 2
  $state = Get-Content -LiteralPath $fixtureState -Raw | ConvertFrom-Json
  if (-not $state.editorLoaded -or -not $state.readyFetch -or -not $state.cookieReturned) {
    throw 'The selected service did not load the remote cloud UI with its fixture cookie.'
  }
  if (-not $state.nativeIpcDenied -or -not $state.remoteSelectServiceDenied) {
    throw 'The remote editor was allowed to invoke a native Tauri command.'
  }
  if ($state.startProtocolVersion -ne 3 -or $state.operationStartCount -ne 1 -or $state.signedPolls -lt 1) {
    throw 'Selecting the service did not create exactly one protocol-v3 operation and signed poll.'
  }
  if ($state.signedTaskReceipts -ne 1 -or -not $state.readReceiptValid) {
    throw 'The selected service did not receive the expected signed read_files receipt.'
  }
  if ($connectedProcess.MainWindowTitle -ne 'Boxy') {
    throw "Expected the connected editor window title 'Boxy', got '$($connectedProcess.MainWindowTitle)'."
  }
  if (Get-ChildItem -LiteralPath $appDir -Force | Where-Object { $_.Name -like '*.WebView2' -or $_.Extension -ieq '.dll' }) {
    throw 'The portable executable folder contains an adjacent WebView2 profile or loader DLL.'
  }

  Assert-NormalExit $connectedProcess 'connected service exit'
  $normalConnectedExit = $true
  $signerAfter = if (Test-Path -LiteralPath $signerPath -PathType Leaf) {
    (Get-FileHash -LiteralPath $signerPath -Algorithm SHA256).Hash
  } else { $null }
  if ($signerBefore -ne $signerAfter) { throw 'The smoke run created or changed a persistent AppData signing key.' }
  $appLocalDataAfter = Get-PathInventory $appLocalDataPath
  if ($appLocalDataBefore -cne $appLocalDataAfter) { throw 'The smoke run created or changed Tauri app-local data.' }

  Write-Output 'Ordinary release opened a blank chooser; invalid session stayed untouched before selection; invalid URL and rejected start recovered after choosing the valid loopback service; protocol-v3 receipt and normal cleanup passed.'
}
finally {
  $cleanupFailures = [System.Collections.Generic.List[string]]::new()
  if ($null -ne $invalidSessionLock) { $invalidSessionLock.Dispose() }
  foreach ($process in @($unselectedProcess, $connectedProcess)) {
    if ($null -eq $process) { continue }
    if (-not $process.HasExited) {
      Close-OwnedFileDialog ([uint32]$process.Id)
      if (-not $process.CloseMainWindow()) {
        $cleanupFailures.Add("Boxy PID $($process.Id) did not accept a normal close request; terminated only this test-owned PID for cleanup.")
        $process.Kill()
      } elseif (-not $process.WaitForExit(30000)) {
        $cleanupFailures.Add("Boxy PID $($process.Id) did not exit after normal close; terminated only this test-owned PID for cleanup.")
        $process.Kill()
      }
      if (-not $process.HasExited) { $process.WaitForExit(5000) | Out-Null }
    }
    $process.Dispose()
  }
  if ($null -ne $fixtureProcess) {
    if (-not $fixtureProcess.HasExited) {
      $fixtureProcess.Kill()
      $fixtureProcess.WaitForExit(5000) | Out-Null
    }
    $fixtureProcess.Dispose()
  }
  if ($null -ne $rejectFixtureProcess) {
    if (-not $rejectFixtureProcess.HasExited) {
      $rejectFixtureProcess.Kill()
      $rejectFixtureProcess.WaitForExit(5000) | Out-Null
    }
    $rejectFixtureProcess.Dispose()
  }
  $resolvedTempBase = [IO.Path]::GetFullPath($tempBase).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
  $testRootLeaf = [IO.Path]::GetFileName($resolvedTestRoot)
  $testRootIsSafe = $resolvedTestRoot.StartsWith(
    $resolvedTempBase + [IO.Path]::DirectorySeparatorChar,
    [StringComparison]::OrdinalIgnoreCase
  ) -and $testRootLeaf -match '^boxy-service-selection-[0-9a-f]{32}$'
  if (-not $testRootIsSafe) {
    throw 'Refusing to remove the smoke directory because its resolved path is outside the expected test-owned temp location.'
  }
  Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force -ErrorAction SilentlyContinue
  if (-not $normalUnselectedExit -or -not $normalConnectedExit) {
    Write-Output 'Smoke failed before both normal Boxy exits; only test-owned fixture resources were cleaned.'
  }
  if ($cleanupFailures.Count -gt 0) { throw ($cleanupFailures -join ' ') }
}
