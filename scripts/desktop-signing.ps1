param([ValidateSet('prepare', 'verify', 'cleanup')][string]$Mode)
$ErrorActionPreference = 'Stop'
$state = Join-Path $env:RUNNER_TEMP 'desktop-signing-state.json'
$pfx = Join-Path $env:RUNNER_TEMP 'desktop-certificate.pfx'
if ($Mode -eq 'cleanup') {
  if (Test-Path $state) {
    $saved = Get-Content $state -Raw | ConvertFrom-Json
    foreach ($thumbprint in $saved.imported) {
      if ($thumbprint -match '^[A-Fa-f0-9]{40}$') { Remove-Item "Cert:\CurrentUser\My\$thumbprint" -DeleteKey -ErrorAction SilentlyContinue }
    }
    Remove-Item $state -Force
  }
  Remove-Item $pfx -Force -ErrorAction SilentlyContinue
  exit 0
}
if ($Mode -eq 'prepare') {
  $windows = @{}
  if ($env:DESKTOP_CERTIFICATE_PFX) {
    try {
      [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:DESKTOP_CERTIFICATE_PFX))
      $password = if ($env:DESKTOP_CERTIFICATE_PASSWORD) { ConvertTo-SecureString $env:DESKTOP_CERTIFICATE_PASSWORD -AsPlainText -Force } else { [Security.SecureString]::new() }
      $certs = @(Import-PfxCertificate -FilePath $pfx -CertStoreLocation 'Cert:\CurrentUser\My' -Password $password)
      @{ imported = @($certs | ForEach-Object { $_.Thumbprint }); signer = $null } | ConvertTo-Json | Set-Content $state
      $certificate = @($certs | Where-Object { $_.HasPrivateKey -and $_.EnhancedKeyUsageList.ObjectId -contains '1.3.6.1.5.5.7.3.3' })
      if ($certificate.Count -ne 1 -or $certificate[0].NotAfter -le (Get-Date) -or $certificate[0].NotBefore -gt (Get-Date)) { throw 'One current code-signing certificate is required' }
      $thumbprint = $certificate[0].Thumbprint
      @{ imported = @($certs | ForEach-Object { $_.Thumbprint }); signer = $thumbprint } | ConvertTo-Json | Set-Content $state
      $windows = @{ certificateThumbprint = $thumbprint; digestAlgorithm = 'sha256'; timestampUrl = 'https://timestamp.digicert.com'; tsp = $true }
      'DESKTOP_AUTHENTICODE=true' | Out-File $env:GITHUB_ENV -Append
      "DESKTOP_SIGNER_THUMBPRINT=$thumbprint" | Out-File $env:GITHUB_ENV -Append
    } finally { Remove-Item $pfx -Force -ErrorAction SilentlyContinue }
  } else { 'DESKTOP_AUTHENTICODE=false' | Out-File $env:GITHUB_ENV -Append }
  $channel = Get-Content src-tauri/update-channel.json -Raw | ConvertFrom-Json
  if (!$channel.publicKey) { throw 'The compiled public updater key must be configured' }
  @{ bundle = @{ createUpdaterArtifacts = $true; windows = $windows }; plugins = @{ updater = @{ pubkey = $channel.publicKey } } } | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $env:RUNNER_TEMP 'desktop-release-config.json')
} elseif ($Mode -eq 'verify' -and (Test-Path $state)) {
  $thumbprint = (Get-Content $state -Raw | ConvertFrom-Json).signer
  $version = (Get-Content src-tauri/tauri.conf.json | ConvertFrom-Json).version
  # The bundler restores unsigned Cargo output. The installed image is verified
  # separately by desktop-smoke.ps1 after extracting this signed installer.
  foreach ($path in @("src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/CaseSim_${version}_x64-setup.exe")) {
    $signature = Get-AuthenticodeSignature $path
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $thumbprint -or !$signature.TimeStamperCertificate) { throw "Authenticode verification failed: $path" }
  }
}
