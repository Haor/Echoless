param(
  [string]$BundleRoot,
  [string]$InstallDir,
  [switch]$SkipInstall
)

$ErrorActionPreference = "Stop"

if ($env:OS -ne "Windows_NT") {
  throw "smoke-windows-installed-app.ps1 must run on Windows."
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$AppDir = (Resolve-Path (Join-Path $ScriptDir "..")).Path
$TauriDir = Join-Path $AppDir "src-tauri"

if ([string]::IsNullOrWhiteSpace($BundleRoot)) {
  $BundleRoot = Join-Path $TauriDir "target\debug\bundle"
} elseif (-not [System.IO.Path]::IsPathRooted($BundleRoot)) {
  $BundleRoot = Join-Path $AppDir $BundleRoot
}

function Find-Installer {
  param([string]$Root)

  if (-not (Test-Path $Root)) {
    throw "Bundle root does not exist: $Root"
  }

  $nsis = Get-ChildItem -Path $Root -Recurse -File -Filter "*.exe" |
    Where-Object { $_.FullName -match "\\nsis\\" -or $_.Name -match "(?i)setup|installer" } |
    Sort-Object FullName |
    Select-Object -First 1
  if ($null -ne $nsis) {
    return @{ Type = "nsis"; Path = $nsis.FullName }
  }

  $msi = Get-ChildItem -Path $Root -Recurse -File -Filter "*.msi" |
    Sort-Object FullName |
    Select-Object -First 1
  if ($null -ne $msi) {
    return @{ Type = "msi"; Path = $msi.FullName }
  }

  throw "No NSIS .exe or MSI installer found under $Root"
}

function Install-Bundle {
  param(
    [hashtable]$Installer,
    [string]$RequestedInstallDir
  )

  Write-Host "windows-installed-smoke: installer=$($Installer.Path)"
  Write-Host "windows-installed-smoke: installer_type=$($Installer.Type)"

  if ($Installer.Type -eq "nsis") {
    $args = @("/S")
    if (-not [string]::IsNullOrWhiteSpace($RequestedInstallDir)) {
      $args += "/D=$RequestedInstallDir"
    }
    $proc = Start-Process -FilePath $Installer.Path -ArgumentList $args -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
      throw "NSIS installer failed with exit code $($proc.ExitCode)"
    }
    return
  }

  if ($Installer.Type -eq "msi") {
    $args = @("/i", $Installer.Path, "/qn", "/norestart")
    if (-not [string]::IsNullOrWhiteSpace($RequestedInstallDir)) {
      $args += "APPLICATIONFOLDER=$RequestedInstallDir"
      $args += "TARGETDIR=$RequestedInstallDir"
    }
    $proc = Start-Process -FilePath "msiexec.exe" -ArgumentList $args -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
      throw "MSI installer failed with exit code $($proc.ExitCode)"
    }
    return
  }

  throw "Unsupported installer type: $($Installer.Type)"
}

function Uninstall-Bundle {
  param(
    [hashtable]$Installer,
    [string]$InstalledDir
  )

  if ($null -eq $Installer) {
    return
  }

  Write-Host "windows-installed-smoke: uninstalling installer_type=$($Installer.Type)"

  if ($Installer.Type -eq "nsis") {
    if ([string]::IsNullOrWhiteSpace($InstalledDir) -or -not (Test-Path $InstalledDir)) {
      Write-Warning "NSIS uninstall skipped; installed directory not found: $InstalledDir"
      return
    }
    $uninstaller = Get-ChildItem -Path $InstalledDir -File -Filter "uninstall*.exe" -ErrorAction SilentlyContinue |
      Sort-Object FullName |
      Select-Object -First 1
    if ($null -eq $uninstaller) {
      Write-Warning "NSIS uninstall skipped; uninstall*.exe not found under $InstalledDir"
      return
    }
    $proc = Start-Process -FilePath $uninstaller.FullName -ArgumentList @("/S") -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
      throw "NSIS uninstaller failed with exit code $($proc.ExitCode)"
    }
    return
  }

  if ($Installer.Type -eq "msi") {
    $proc = Start-Process -FilePath "msiexec.exe" -ArgumentList @("/x", $Installer.Path, "/qn", "/norestart") -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
      throw "MSI uninstall failed with exit code $($proc.ExitCode)"
    }
    return
  }

  throw "Unsupported installer type: $($Installer.Type)"
}

function Find-InstalledApp {
  param([string]$RequestedInstallDir)

  $candidates = @()
  if (-not [string]::IsNullOrWhiteSpace($RequestedInstallDir)) {
    $candidates += $RequestedInstallDir
  }
  $candidates += @(
    (Join-Path $env:LOCALAPPDATA "Programs\Echoless"),
    (Join-Path $env:LOCALAPPDATA "Echoless"),
    (Join-Path $env:ProgramFiles "Echoless")
  )
  if (-not [string]::IsNullOrWhiteSpace(${env:ProgramFiles(x86)})) {
    $candidates += (Join-Path ${env:ProgramFiles(x86)} "Echoless")
  }

  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) {
      return (Resolve-Path $candidate).Path
    }
  }

  $roots = @(
    (Join-Path $env:LOCALAPPDATA "Programs"),
    $env:LOCALAPPDATA,
    $env:ProgramFiles,
    ${env:ProgramFiles(x86)}
  ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and (Test-Path $_) }

  foreach ($root in $roots) {
    $match = Get-ChildItem -Path $root -Directory -Recurse -ErrorAction SilentlyContinue |
      Where-Object {
        $_.Name -match "(?i)echoless" -or
        (Test-Path (Join-Path $_.FullName "echoless.exe")) -or
        (Get-ChildItem -Path $_.FullName -File -Filter "*echoless*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1)
      } |
      Sort-Object FullName |
      Select-Object -First 1
    if ($null -ne $match) {
      return $match.FullName
    }
  }

  throw "Installed Echoless directory not found."
}

$installer = $null
$installed = $null

try {
  if (-not $SkipInstall) {
    $installer = Find-Installer -Root $BundleRoot
    Install-Bundle -Installer $installer -RequestedInstallDir $InstallDir
  }

  $installed = Find-InstalledApp -RequestedInstallDir $InstallDir
  Write-Host "windows-installed-smoke: installed_app=$installed"

  Push-Location $AppDir
  try {
    node ".\scripts\smoke-tauri-bundle.mjs" --installed-app $installed
  } finally {
    Pop-Location
  }
} finally {
  if (-not $SkipInstall) {
    try {
      Uninstall-Bundle -Installer $installer -InstalledDir $installed
    } catch {
      Write-Warning "windows-installed-smoke: uninstall failed: $_"
    }
  }
}
