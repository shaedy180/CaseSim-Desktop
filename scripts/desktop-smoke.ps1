param([ValidateSet('inspect', 'controls', 'fullscreen', 'zoom', 'install', 'uninstall', 'diagnostics')][string]$Mode, [int]$ProcessId)
$ErrorActionPreference = 'Stop'
function Get-SmokeSha256([string]$Path) {
  # Avoid module auto-loading differences when Windows PowerShell is launched
  # from the runner's PowerShell 7 environment. The verification stays SHA-256.
  $stream = [IO.File]::OpenRead($Path)
  $algorithm = [Security.Cryptography.SHA256]::Create()
  try { return [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '') }
  finally { $algorithm.Dispose(); $stream.Dispose() }
}
if ($Mode -eq 'diagnostics') {
  $log = Join-Path $env:LOCALAPPDATA 'de.shaedy.casesim.desktop/logs/desktop.log'
  if (Test-Path $log) { Get-Content $log -Tail 80 | Set-Content 'artifacts/desktop/native-events.log' }
  exit 0
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DesktopWindow {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int index);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [StructLayout(LayoutKind.Sequential)] public struct KeyboardInput { public ushort Key, Scan; public uint Flags, Time; public UIntPtr Extra; }
  [StructLayout(LayoutKind.Sequential)] public struct MouseInput { public int X, Y; public uint Data, Flags, Time; public UIntPtr Extra; }
  [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public KeyboardInput Keyboard; [FieldOffset(0)] public MouseInput Mouse; }
  [StructLayout(LayoutKind.Sequential)] public struct Input { public uint Type; public InputUnion Data; }
  [DllImport("user32.dll", SetLastError = true)] public static extern uint SendInput(uint count, Input[] inputs, int size);
  public static void Key(ushort key, bool up) {
    var input = new Input { Type = 1, Data = new InputUnion { Keyboard = new KeyboardInput { Key = key, Flags = up ? 2u : 0u } } };
    if (SendInput(1, new[] { input }, Marshal.SizeOf(typeof(Input))) != 1) throw new InvalidOperationException("Native fixture input was rejected");
  }
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
}
'@
$directory = Join-Path $env:RUNNER_TEMP 'CaseSim-install-smoke'
if ($Mode -eq 'install') {
  $version = (Get-Content src-tauri/tauri.conf.json | ConvertFrom-Json).version
  if (Test-Path $directory) { throw 'Installer smoke directory must be new' }
  $installer = (Resolve-Path "src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/CaseSim_${version}_x64-setup.exe").Path
  $process = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$directory") -WindowStyle Hidden -Wait -PassThru
  if ($process.ExitCode -ne 0 -or !(Test-Path (Join-Path $directory 'CaseSim.exe'))) { throw 'Silent NSIS installation failed' }
  $executable = Join-Path $directory 'CaseSim.exe'
  # Tauri patches the packaged image with its NSIS type, signs that image when
  # configured, then restores the unsigned/unpatched Cargo output. Test the
  # installed image itself rather than comparing these intentionally different files.
  if ((Get-Item $executable).VersionInfo.ProductVersion -ne $version) { throw 'Installed version mismatch' }
  if ($env:DESKTOP_AUTHENTICODE -eq 'true') {
    $signature = Get-AuthenticodeSignature $executable
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $env:DESKTOP_SIGNER_THUMBPRINT -or !$signature.TimeStamperCertificate) { throw 'Installed Authenticode verification failed' }
  }
  @{ installation = 'passed'; executable = $executable; sha256 = Get-SmokeSha256 $executable; webview2 = 'preinstalled runner runtime' } | ConvertTo-Json -Compress
  exit 0
}
if ($Mode -eq 'uninstall') {
  $uninstaller = @(Get-ChildItem $directory -Filter '*uninstall*.exe')
  if ($uninstaller.Count -ne 1) { throw 'Expected exactly one NSIS uninstaller' }
  $process = Start-Process -FilePath $uninstaller[0].FullName -ArgumentList @('/S', "_?=$directory") -WindowStyle Hidden -Wait -PassThru
  if ($process.ExitCode -ne 0 -or (Test-Path (Join-Path $directory 'CaseSim.exe'))) { throw 'Silent NSIS uninstall failed' }
  '{"uninstallation":"passed"}'
  exit 0
}
$process = Get-Process -Id $ProcessId
$handle = $process.MainWindowHandle
if ($handle -eq [IntPtr]::Zero) { throw 'No native main window' }
if ($Mode -ne 'inspect') {
  $null = [DesktopWindow]::ShowWindow($handle, 9)
  $null = [DesktopWindow]::SetForegroundWindow($handle)
  $shell = New-Object -ComObject WScript.Shell
  if (!$shell.AppActivate($ProcessId)) { throw 'Could not activate native window' }
  # AppActivate can report success before foreground transfer finishes. Verify
  # the real OS target, then keep modifiers held while WebView2 handles the key.
  # This uses the same native accelerator as a person, never a JS/native command.
  $foregroundProcess = [uint32]0
  for ($attempt = 0; $attempt -lt 20; $attempt++) {
    $null = [DesktopWindow]::GetWindowThreadProcessId([DesktopWindow]::GetForegroundWindow(), [ref]$foregroundProcess)
    if ($foregroundProcess -eq $ProcessId) { break }
    Start-Sleep -Milliseconds 100
  }
  if ($foregroundProcess -ne $ProcessId) { throw 'The tested application did not become the foreground input target' }
  $keys = switch ($Mode) { controls { @(0x11, 0x10, 0x44) }; fullscreen { @(0x7A) }; zoom { @(0x11, 0x30) } }
  $pressed = @()
  try {
    foreach ($key in $keys) {
      [DesktopWindow]::Key([uint16]$key, $false)
      $pressed += $key
      Start-Sleep -Milliseconds 100
    }
    Start-Sleep -Milliseconds 400
  } finally {
    [array]::Reverse($pressed)
    foreach ($key in $pressed) { [DesktopWindow]::Key([uint16]$key, $true) }
  }
  exit 0
}
$rect = New-Object DesktopWindow+Rect
$null = [DesktopWindow]::GetWindowRect($handle, [ref]$rect)
$style = [DesktopWindow]::GetWindowLong($handle, -16)
$result = @{ menu = [DesktopWindow]::GetMenu($handle) -ne [IntPtr]::Zero; titlebar = ($style -band 0x00C00000) -ne 0; systemMenu = ($style -band 0x00080000) -ne 0; width = $rect.Right - $rect.Left; height = $rect.Bottom - $rect.Top; workingSetBytes = $process.WorkingSet64 }
Add-Type -AssemblyName System.Drawing
$bitmap = [System.Drawing.Bitmap]::new([int]$result.width, [int]$result.height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
  $dc = $graphics.GetHdc()
  try { $captured = [DesktopWindow]::PrintWindow($handle, $dc, 2) } finally { $graphics.ReleaseHdc($dc) }
  if (!$captured) { $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size) }
  $bitmap.Save((Join-Path (Get-Location) 'artifacts/desktop/native-window.png'))
} finally { $graphics.Dispose(); $bitmap.Dispose() }
$result | ConvertTo-Json -Compress
