$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$tempBase = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$testRoot = Join-Path $tempBase "boxy-v23-native-$([Guid]::NewGuid().ToString('N'))"
$scopeRoot = Join-Path $testRoot 'scope'
$statePath = Join-Path $testRoot 'state.json'
$portPath = Join-Path $testRoot 'port.txt'
$fixture = Join-Path $PSScriptRoot 'fixtures/v23_service.mjs'
$node = (Get-Command node -ErrorAction Stop).Source
$fixtureProcess = $null
$previousUrl = $env:BOXY_RENDER_SMOKE_SERVICE_URL
$previousState = $env:BOXY_RENDER_SMOKE_STATE
$tempPrefix = [IO.Path]::GetFullPath($tempBase).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar

New-Item -ItemType Directory -Path (Join-Path $scopeRoot 'scope') -Force | Out-Null
[IO.File]::WriteAllText((Join-Path $scopeRoot 'scope/probe.txt'), 'isolated probe', [Text.UTF8Encoding]::new($false))

try {
  $fixtureStart = [Diagnostics.ProcessStartInfo]::new()
  $fixtureStart.FileName = $node
  $fixtureStart.UseShellExecute = $false
  $fixtureStart.CreateNoWindow = $true
  $fixtureStart.ArgumentList.Add($fixture)
  $fixtureStart.ArgumentList.Add('--state-file')
  $fixtureStart.ArgumentList.Add($statePath)
  $fixtureStart.ArgumentList.Add('--port-file')
  $fixtureStart.ArgumentList.Add($portPath)
  $fixtureStart.ArgumentList.Add('--root')
  $fixtureStart.ArgumentList.Add($scopeRoot)
  $fixtureProcess = [Diagnostics.Process]::Start($fixtureStart)

  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  while (-not (Test-Path -LiteralPath $portPath) -and [DateTime]::UtcNow -lt $deadline) {
    if ($fixtureProcess.HasExited) { throw 'The v4 fixture exited before startup.' }
    Start-Sleep -Milliseconds 100
  }
  if (-not (Test-Path -LiteralPath $portPath)) { throw 'The v4 fixture did not become ready.' }
  $port = (Get-Content -LiteralPath $portPath -Raw).Trim()
  $env:BOXY_RENDER_SMOKE_SERVICE_URL = "http://127.0.0.1:$port"
  $env:BOXY_RENDER_SMOKE_STATE = $statePath

  cargo +1.98.0 run --locked -p boxy --example desktop_render_smoke
  if ($LASTEXITCODE -ne 0) { throw 'Native startup smoke failed.' }
  $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
  if ($state.errors.Count -gt 0 -or -not $state.receiptValid -or -not $state.closed) {
    throw "The fixture did not verify the v4 request flow: $($state.errors -join ', ')."
  }
}
finally {
  $env:BOXY_RENDER_SMOKE_SERVICE_URL = $previousUrl
  $env:BOXY_RENDER_SMOKE_STATE = $previousState
  if ($null -ne $fixtureProcess) {
    if (-not $fixtureProcess.HasExited) {
      $fixtureProcess.Kill()
      $fixtureProcess.WaitForExit(5000) | Out-Null
    }
    $fixtureProcess.Dispose()
  }
  $fullTestRoot = [IO.Path]::GetFullPath($testRoot)
  if ($fullTestRoot.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase) -and ((Split-Path -Leaf $fullTestRoot) -like 'boxy-v23-native-*')) {
    Remove-Item -LiteralPath $fullTestRoot -Recurse -Force -ErrorAction SilentlyContinue
  }
}
