param(
  [Parameter(Mandatory = $true)]
  [string]$Executable
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Threading;

public static class BoxyTestServer
{
    private static TcpListener listener;
    private static Thread worker;
    private static int browserRequests;
    private static int operationRequests;
    private static int pollRequests;
    private static int startProtocolVersion;
    private static int port;

    public static int BrowserRequests { get { return Volatile.Read(ref browserRequests); } }
    public static int OperationRequests { get { return Volatile.Read(ref operationRequests); } }
    public static int PollRequests { get { return Volatile.Read(ref pollRequests); } }
    public static int StartProtocolVersion { get { return Volatile.Read(ref startProtocolVersion); } }
    public static int Port { get { return port; } }

    public static void Start()
    {
        listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        port = ((IPEndPoint)listener.LocalEndpoint).Port;
        worker = new Thread(Serve);
        worker.IsBackground = true;
        worker.Start();
    }

    public static void Stop()
    {
        listener.Stop();
        worker.Join(2000);
    }

    private static void Serve()
    {
        while (true)
        {
            TcpClient client;
            try { client = listener.AcceptTcpClient(); }
            catch (SocketException) { return; }
            catch (ObjectDisposedException) { return; }
            ThreadPool.QueueUserWorkItem(_ => Handle(client));
        }
    }

    private static void Handle(TcpClient client)
    {
        try
        {
            using (client)
            using (NetworkStream stream = client.GetStream())
            {
                stream.ReadTimeout = 10000;
                var header = new MemoryStream();
                int previous = -1, last = -1, beforeLast = -1;
                while (header.Length < 16384)
                {
                    int current = stream.ReadByte();
                    if (current < 0) return;
                    header.WriteByte((byte)current);
                    if (beforeLast == '\r' && last == '\n' && previous == '\r' && current == '\n') break;
                    beforeLast = last; last = previous; previous = current;
                }
                string[] lines = Encoding.ASCII.GetString(header.ToArray()).Split(new[] { "\r\n" }, StringSplitOptions.None);
                string[] request = lines[0].Split(' ');
                int contentLength = 0;
                foreach (string line in lines)
                    if (line.StartsWith("Content-Length:", StringComparison.OrdinalIgnoreCase))
                        int.TryParse(line.Substring(15).Trim(), out contentLength);
                var requestBody = new MemoryStream();
                var buffer = new byte[4096];
                while (contentLength > 0)
                {
                    int count = stream.Read(buffer, 0, Math.Min(buffer.Length, contentLength));
                    if (count <= 0) return;
                    requestBody.Write(buffer, 0, count);
                    contentLength -= count;
                }

                string body;
                string contentType = "application/json";
                if (request[0] == "POST" && request[1] == "/api/v2/connector/operations")
                {
                    Interlocked.Increment(ref operationRequests);
                    using (JsonDocument startRequest = JsonDocument.Parse(requestBody.ToArray()))
                        Interlocked.Exchange(ref startProtocolVersion, startRequest.RootElement.GetProperty("protocolVersion").GetInt32());
                    string authorization = new string('A', 43);
                    string route = new string('a', 32);
                    body = "{\"protocolVersion\":3,\"operationId\":\"smoke\",\"browserUrl\":\"http://127.0.0.1:" +
                        port + "/d_" + route + "/?authorization=" + authorization + "\"}";
                }
                else if (request[0] == "GET" && request[1] == "/d_" +
                    new string('a', 32) + "/?authorization=" + new string('A', 43))
                {
                    Interlocked.Increment(ref browserRequests);
                    body = "<html><body>Boxy smoke editor</body></html>";
                    contentType = "text/html";
                }
                else if (request[0] == "GET" && request[1].StartsWith("/api/v2/connector/operations/", StringComparison.Ordinal))
                {
                    Interlocked.Increment(ref pollRequests);
                    body = "{\"protocolVersion\":3,\"state\":\"waiting\",\"directoryTasks\":[]}";
                }
                else
                    body = "{}";

                byte[] payload = Encoding.UTF8.GetBytes(body);
                byte[] response = Encoding.ASCII.GetBytes(
                    "HTTP/1.1 200 OK\r\nContent-Type: " + contentType +
                    "\r\nContent-Length: " + payload.Length + "\r\nConnection: close\r\n\r\n");
                stream.Write(response, 0, response.Length);
                stream.Write(payload, 0, payload.Length);
            }
        }
        catch (IOException) {}
        catch (SocketException) {}
    }
}
'@

$source = (Resolve-Path -LiteralPath $Executable).Path
$fixtureRoot = Join-Path $env:RUNNER_TEMP 'boxy-browser-only-smoke'
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null
$fixtureExecutable = Join-Path $fixtureRoot 'boxy-browser-only.exe'
Copy-Item -LiteralPath $source -Destination $fixtureExecutable -Force
$profilePath = "$($fixtureExecutable).WebView2"
if (Test-Path -LiteralPath $profilePath) {
  throw "Smoke fixture unexpectedly already has a WebView2 profile directory: $profilePath"
}

$sessionDirectory = Join-Path $fixtureRoot 'Session'
New-Item -ItemType Directory -Path $sessionDirectory -Force | Out-Null
$sessionPath = Join-Path $sessionDirectory 'session.bin'
[IO.File]::WriteAllBytes($sessionPath, [byte[]](1..8))

[BoxyTestServer]::Start()
$child = $null
try {
  $serverUrl = "http://127.0.0.1:$([BoxyTestServer]::Port)"
  $arguments = @('--server', $serverUrl, '--session', $sessionPath)
  $startInfo = [Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $fixtureExecutable
  $startInfo.UseShellExecute = $false
  foreach ($argument in $arguments) { $startInfo.ArgumentList.Add($argument) }
  $startInfo.Environment['WEBVIEW2_BROWSER_EXECUTABLE_FOLDER'] = Join-Path $fixtureRoot 'missing-webview-runtime'
  $child = [Diagnostics.Process]::Start($startInfo)

  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  while ([DateTime]::UtcNow -lt $deadline -and [BoxyTestServer]::BrowserRequests -eq 0) {
    if ($child.HasExited) { throw "Boxy exited before the system browser reached the local editor (exit $($child.ExitCode))" }
    Start-Sleep -Milliseconds 250
  }
  if ([BoxyTestServer]::BrowserRequests -eq 0) {
    throw 'The system browser did not reach the fixture editor after the operation was created.'
  }
  if ([BoxyTestServer]::OperationRequests -ne 1) {
    throw "Expected one successful operation creation request; received $([BoxyTestServer]::OperationRequests)."
  }
  $pollDeadline = [DateTime]::UtcNow.AddSeconds(10)
  while ([DateTime]::UtcNow -lt $pollDeadline -and [BoxyTestServer]::PollRequests -eq 0) {
    if ($child.HasExited) { throw "Boxy exited before the protocol-v3 poll completed (exit $($child.ExitCode))" }
    Start-Sleep -Milliseconds 100
  }
  if ([BoxyTestServer]::StartProtocolVersion -ne 3 -or [BoxyTestServer]::PollRequests -eq 0) {
    throw "Expected a protocol-v3 start and poll; start version $([BoxyTestServer]::StartProtocolVersion), polls $([BoxyTestServer]::PollRequests)."
  }
  if (Test-Path -LiteralPath $profilePath) {
    throw "Boxy created a WebView2 profile directory beside its executable: $profilePath"
  }
  Write-Output 'Connector sent protocol v3, fetched the validated browser URL, completed a v3 poll, and created no adjacent WebView2 profile.'
}
finally {
  if ($null -ne $child) {
    if (-not $child.HasExited) {
      $child.Kill()
      $child.WaitForExit()
    }
    $child.Dispose()
  }
  [BoxyTestServer]::Stop()
}
