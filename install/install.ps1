<#
.SYNOPSIS
  Modrinth Importer installer for Windows.
.EXAMPLE
  irm https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.ps1 | iex
.EXAMPLE
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.ps1))) -Uninstall
  Removes the CLI, the app, settings, caches and stored credentials. Add -KeepData to keep settings and credentials.
.EXAMPLE
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.ps1))) -CliOnly
#>
[CmdletBinding()]
param(
  [switch]$CliOnly,
  [switch]$GuiOnly,
  [string]$Version = "",
  [switch]$Uninstall,
  [switch]$KeepData
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Repo = "Aoof/modrinth-importer"

$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -notin @("AMD64", "ARM64")) { throw "Unsupported architecture: $arch" }
# ARM64 Windows runs the x64 build through emulation.
$CliAsset = "mrimport-x86_64-pc-windows-msvc.exe"
$GuiAsset = "ModrinthImporter-windows-x64-setup.exe"

$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\ModrinthImporter\bin"

function Remove-FromUserPath([string]$dir) {
  $p = [Environment]::GetEnvironmentVariable("Path", "User")
  if (-not $p) { return }
  $parts = $p.Split(";") | Where-Object { $_ -and ($_ -ne $dir) }
  [Environment]::SetEnvironmentVariable("Path", ($parts -join ";"), "User")
}

if ($Uninstall) {
  $state = @{ n = 0 }
  function Remove-Path([string]$p) {
    if (Test-Path -LiteralPath $p) {
      Remove-Item -LiteralPath $p -Recurse -Force -ErrorAction SilentlyContinue
      Write-Host "  removed $p"
      $state.n++
    }
  }

  # 1. Settings, caches, webview data and Credential Manager entries.
  if (-not $KeepData) {
    $cli = Join-Path $InstallDir "mrimport.exe"
    if (Test-Path $cli) { & $cli purge --yes | Out-Null }   # the CLI knows what it created
    # Sweep the same locations directly, so this also works when only the app was installed.
    foreach ($p in @(
        (Join-Path $env:APPDATA "com.aoof.modrinthimporter"),
        (Join-Path $env:LOCALAPPDATA "com.aoof.modrinthimporter"),
        (Join-Path $env:APPDATA "Aoof\ModrinthImporter"),
        (Join-Path $env:LOCALAPPDATA "Aoof\ModrinthImporter"))) { Remove-Path $p }
    foreach ($root in @($env:APPDATA, $env:LOCALAPPDATA)) {   # drop the vendor folder only if now empty
      $vendor = Join-Path $root "Aoof"
      if ((Test-Path $vendor) -and -not (Get-ChildItem $vendor -Force)) { Remove-Item $vendor -Force }
    }
    foreach ($t in @("modrinth-token.modrinth-importer", "curseforge-api-key.modrinth-importer")) {
      cmdkey /delete:$t 2>&1 | Out-Null
      if ($LASTEXITCODE -eq 0) { Write-Host "  removed credential $t"; $state.n++ }
    }
  }

  # 2. Desktop app: run its own uninstaller in place (so we can wait for it), then clean up leftovers.
  $appDir = Join-Path $env:LOCALAPPDATA "Modrinth Importer"
  $u = Join-Path $appDir "uninstall.exe"
  if (Test-Path $u) { Start-Process $u -ArgumentList "/S", "_?=$appDir" -Wait }
  Remove-Path $appDir
  Remove-Path (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Modrinth Importer.lnk")
  Remove-Path (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Modrinth Importer")
  foreach ($k in @("HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Modrinth Importer", "HKCU:\Software\aoof\Modrinth Importer")) {
    if (Test-Path $k) { Remove-Item $k -Recurse -Force -ErrorAction SilentlyContinue; Write-Host "  removed registry key $k"; $state.n++ }
  }
  if ((Test-Path "HKCU:\Software\aoof") -and -not (Get-ChildItem "HKCU:\Software\aoof")) { Remove-Item "HKCU:\Software\aoof" -Force }

  # 3. CLI and its PATH entry.
  Remove-Path (Split-Path $InstallDir -Parent)
  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if ($userPath -and ($userPath.Split(";") -contains $InstallDir)) {
    Remove-FromUserPath $InstallDir
    Write-Host "  removed $InstallDir from your PATH"
    $state.n++
  }

  if ($state.n -eq 0) { Write-Host "Nothing to remove." }
  else { Write-Host "Modrinth Importer removed ($($state.n) items). Nothing is left behind." }
  return
}

# Releases are pre-releases (developer builds), which GitHub's "latest" alias skips.
if (-not $Version) {
  $rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases?per_page=1" -Headers @{ "User-Agent" = "modrinth-importer-installer" }
  $Version = @($rel)[0].tag_name
  if (-not $Version) { throw "Could not determine the latest release; try -Version X.Y.Z" }
}
$base = "https://github.com/$Repo/releases/download/v$($Version.TrimStart('v'))"

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("mrimport-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  Write-Host "Fetching checksums..."
  Invoke-WebRequest "$base/SHA256SUMS" -OutFile "$tmp\SHA256SUMS"
  $sums = Get-Content "$tmp\SHA256SUMS"

  function Get-Verified([string]$asset) {
    Write-Host "Downloading $asset..."
    Invoke-WebRequest "$base/$asset" -OutFile "$tmp\$asset"
    $line = $sums | Where-Object { $_ -match "\s\*?$([regex]::Escape($asset))$" } | Select-Object -First 1
    if (-not $line) { throw "$asset is not listed in SHA256SUMS" }
    $want = ($line -split "\s+")[0].ToLower()
    $got = (Get-FileHash "$tmp\$asset" -Algorithm SHA256).Hash.ToLower()
    if ($want -ne $got) { throw "Checksum mismatch for $asset" }
    return "$tmp\$asset"
  }

  if (-not $GuiOnly) {
    $f = Get-Verified $CliAsset
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item $f (Join-Path $InstallDir "mrimport.exe") -Force
    $p = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($p -split ";") -notcontains $InstallDir) {
      [Environment]::SetEnvironmentVariable("Path", ($(if ($p) { "$p;" } else { "" }) + $InstallDir), "User")
      Write-Host "Added $InstallDir to your PATH (restart the terminal to use `mrimport`)."
    }
    Write-Host "Installed CLI: $InstallDir\mrimport.exe"
  }

  if (-not $CliOnly) {
    $f = Get-Verified $GuiAsset
    Write-Host "Installing the desktop app..."
    Start-Process $f -ArgumentList "/S" -Wait
    Write-Host "Installed app (Start menu: Modrinth Importer)."
  }
  Write-Host "Done."
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
