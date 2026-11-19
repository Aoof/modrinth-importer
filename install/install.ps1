<#
.SYNOPSIS
  Modrinth Importer installer for Windows.
.EXAMPLE
  irm https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.ps1 | iex
.EXAMPLE
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.ps1))) -CliOnly
#>
[CmdletBinding()]
param(
  [switch]$CliOnly,
  [switch]$GuiOnly,
  [string]$Version = "",
  [switch]$Uninstall
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
  Remove-Item -Recurse -Force (Split-Path $InstallDir -Parent) -ErrorAction SilentlyContinue
  Remove-FromUserPath $InstallDir
  $u = Join-Path $env:LOCALAPPDATA "Modrinth Importer\uninstall.exe"
  if (Test-Path $u) { Start-Process $u -ArgumentList "/S" -Wait }
  Write-Host "Modrinth Importer removed."
  return
}

$base = if ($Version) { "https://github.com/$Repo/releases/download/v$($Version.TrimStart('v'))" }
        else { "https://github.com/$Repo/releases/latest/download" }

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
