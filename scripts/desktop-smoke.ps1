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
  Start-Sleep -Milliseconds 250
  $keys = @{ controls = '^+d'; fullscreen = '{F11}'; zoom = '^0' }
  $shell.SendKeys($keys[$Mode])
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
