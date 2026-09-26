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
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class BoxyWindowProbe
{
    private delegate bool EnumWindowsProc(IntPtr window, IntPtr state);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr state);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowTextLengthW(IntPtr window);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowTextW(IntPtr window, StringBuilder text, int capacity);

    [DllImport("user32.dll")]
    private static extern IntPtr GetDlgItem(IntPtr window, int id);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool IsWindowEnabled(IntPtr window);

    [DllImport("user32.dll", EntryPoint = "SendMessageW")]
    private static extern IntPtr SendMessageValue(IntPtr window, uint message, IntPtr first, IntPtr second);

    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)]
    private static extern IntPtr SendMessageText(IntPtr window, uint message, IntPtr capacity, StringBuilder text);

    private static string WindowText(IntPtr window)
    {
        var text = new StringBuilder(GetWindowTextLengthW(window) + 1);
        GetWindowTextW(window, text, text.Capacity);
        return text.ToString();
    }

    public static IntPtr FindWindowForProcess(int expectedProcessId, string expectedTitle)
    {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, state) => {
            uint processId;
            GetWindowThreadProcessId(window, out processId);
            if (processId == (uint)expectedProcessId &&
                IsWindowVisible(window) &&
                WindowText(window) == expectedTitle)
            {
                found = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string ServiceUrl(IntPtr window)
    {
        IntPtr edit = GetDlgItem(window, 101);
        if (edit == IntPtr.Zero)
            throw new Exception("Service URL field is missing");

        int length = checked((int)SendMessageValue(edit, 0x000E, IntPtr.Zero, IntPtr.Zero));
        var text = new StringBuilder(length + 1);
        SendMessageText(edit, 0x000D, (IntPtr)text.Capacity, text);
        return text.ToString();
    }

    public static bool EditorButtonEnabled(IntPtr window)
    {
        IntPtr button = GetDlgItem(window, 201);
        if (button == IntPtr.Zero)
            throw new Exception("Browser editor button is missing");
        return IsWindowEnabled(button);
    }

    public static void Click(IntPtr window, int id)
    {
        IntPtr button = GetDlgItem(window, id);
        if (button == IntPtr.Zero)
            throw new Exception("Boxy button is missing: " + id);
        SendMessageValue(button, 0x00F5, IntPtr.Zero, IntPtr.Zero);
    }
}

public static class BoxyTestServer
{
    private static TcpListener listener;
    private static Thread worker;
    private static int browserRequests;
    private static int port;

    public static int BrowserRequests { get { return Volatile.Read(ref browserRequests); } }

    public static int Start()
    {
        listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        port = ((IPEndPoint)listener.LocalEndpoint).Port;
        worker = new Thread(Serve);
        worker.IsBackground = true;
        worker.Start();
        return port;
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
                int previous = -1;
                int last = -1;
                int beforeLast = -1;
                while (header.Length < 16384)
                {
                    int current = stream.ReadByte();
                    if (current < 0) return;
                    header.WriteByte((byte)current);
                    if (beforeLast == '\r' && last == '\n' && previous == '\r' && current == '\n')
                        break;
                    beforeLast = last;
                    last = previous;
                    previous = current;
                }
                string headerText = Encoding.ASCII.GetString(header.ToArray());
                string[] lines = headerText.Split(new[] { "\r\n" }, StringSplitOptions.None);
                string[] request = lines[0].Split(' ');
                int contentLength = 0;
                foreach (string line in lines)
                {
                    if (line.StartsWith("Content-Length:", StringComparison.OrdinalIgnoreCase))
                        int.TryParse(line.Substring(15).Trim(), out contentLength);
                }
                var buffer = new byte[4096];
                while (contentLength > 0)
                {
                    int count = stream.Read(buffer, 0, Math.Min(buffer.Length, contentLength));
                    if (count <= 0) return;
                    contentLength -= count;
                }

                string body;
                if (request[0] == "POST" && request[1] == "/api/v2/connector/operations")
                {
                    string authorization = new string('A', 43);
                    string route = new string('a', 32);
                    body = "{\"operationId\":\"smoke\",\"browserUrl\":\"http://127.0.0.1:" +
                        port + "/d_" + route + "/?authorization=" + authorization + "\"}";
                }
                else if (request[1].StartsWith("/d_", StringComparison.Ordinal))
                {
                    Interlocked.Increment(ref browserRequests);
                    body = "<html><body>Boxy smoke editor</body></html>";
                }
                else if (request[0] == "GET" && request[1].StartsWith("/api/v2/connector/operations/", StringComparison.Ordinal))
                {
                    body = "{\"state\":\"waiting\",\"directoryTasks\":[]}";
                }
                else
                {
                    body = "{}";
                }
                string contentType = request[1].StartsWith("/d_", StringComparison.Ordinal)
                    ? "text/html" : "application/json";
                byte[] payload = Encoding.UTF8.GetBytes(body);
                byte[] response = Encoding.ASCII.GetBytes(
                    "HTTP/1.1 200 OK\r\nContent-Type: " + contentType +
                    "\r\nContent-Length: " + payload.Length +
                    "\r\nConnection: close\r\n\r\n");
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
$renamed = Join-Path $env:RUNNER_TEMP 'service.example.test.exe'
Copy-Item -LiteralPath $source -Destination $renamed -Force

$cases = @(
  @{ Path = $source; ExpectedUrl = '' },
  @{ Path = $renamed; ExpectedUrl = 'https://service.example.test' }
)

foreach ($case in $cases) {
  $child = Start-Process -FilePath $case.Path -PassThru
  try {
    $window = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
      $child.Refresh()
      if ($child.HasExited) {
        throw "Boxy exited before showing a window: $($case.Path)"
      }
      $window = [BoxyWindowProbe]::FindWindowForProcess($child.Id, 'Boxy remote service')
      if ($window -ne [IntPtr]::Zero) { break }
      Start-Sleep -Milliseconds 500
    }
    if ($window -eq [IntPtr]::Zero) {
      throw "No visible Boxy startup window: $($case.Path)"
    }
    $actual = [BoxyWindowProbe]::ServiceUrl($window)
    if ($actual -cne $case.ExpectedUrl) {
      throw "Unexpected URL for $($case.Path): '$actual'"
    }
    Write-Output "Visible Boxy window with URL '$actual': $($case.Path)"
  }
  finally {
    $child.Refresh()
    if (-not $child.HasExited) {
      Stop-Process -Id $child.Id -Force
      $child.WaitForExit()
    }
    $child.Dispose()
  }
}

$port = [BoxyTestServer]::Start()
try {
  $sessionDirectory = Join-Path $env:RUNNER_TEMP 'boxy-smoke-root\Session'
  New-Item -ItemType Directory -Path $sessionDirectory -Force | Out-Null
  $sessionPath = Join-Path $sessionDirectory 'session.bin'
  [IO.File]::WriteAllBytes($sessionPath, [byte[]](1..8))
  $serverUrl = "http://127.0.0.1:$port"
  $publicKey = 'WGZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmY'
  $arguments = @('--server', $serverUrl, '--server-public-key', $publicKey, '--session', $sessionPath)
  $child = Start-Process -FilePath $source -ArgumentList $arguments -PassThru
  try {
    $setup = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
      $child.Refresh()
      if ($child.HasExited) { throw 'Boxy exited before showing its setup window' }
      $setup = [BoxyWindowProbe]::FindWindowForProcess($child.Id, 'Boxy remote service')
      if ($setup -ne [IntPtr]::Zero) { break }
      Start-Sleep -Milliseconds 500
    }
    if ($setup -eq [IntPtr]::Zero) { throw 'Boxy setup window did not appear' }
    if ([BoxyWindowProbe]::ServiceUrl($setup) -cne $serverUrl) {
      throw 'The local service URL was not prefilled'
    }
    [BoxyWindowProbe]::Click($setup, 103)

    $status = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
      $child.Refresh()
      if ($child.HasExited) { throw 'Boxy exited before showing connection status' }
      $status = [BoxyWindowProbe]::FindWindowForProcess($child.Id, 'Boxy connection status')
      if ($status -ne [IntPtr]::Zero) { break }
      Start-Sleep -Milliseconds 500
    }
    if ($status -eq [IntPtr]::Zero) { throw 'Boxy connection status window did not appear' }

    $editorReady = $false
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
      if ([BoxyWindowProbe]::EditorButtonEnabled($status) -and [BoxyTestServer]::BrowserRequests -gt 0) {
        $editorReady = $true
        break
      }
      $child.Refresh()
      if ($child.HasExited) { throw 'Boxy exited before opening the browser editor' }
      Start-Sleep -Milliseconds 500
    }
    if (-not $editorReady) { throw 'Boxy did not open the browser editor or enable its button' }
    [BoxyWindowProbe]::Click($status, 201)
    [BoxyWindowProbe]::Click($status, 202)
    if (-not $child.WaitForExit(10000)) { throw 'Boxy did not stop after closing the status window' }
    if ($child.ExitCode -ne 0) { throw "Boxy stopped with exit code $($child.ExitCode)" }
    Write-Output 'Boxy status window, automatic browser editor, and reopen button are working'
  }
  finally {
    $child.Refresh()
    if (-not $child.HasExited) {
      Stop-Process -Id $child.Id -Force
      $child.WaitForExit()
    }
    $child.Dispose()
  }
}
finally {
  [BoxyTestServer]::Stop()
}
