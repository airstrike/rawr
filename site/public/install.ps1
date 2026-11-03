# Installs rawr's prebuilt Windows binaries, rawr and cargo-rawr:
#
#   irm https://rawr.airstrike.fyi/install.ps1 | iex
#
# They go to $env:RAWR_INSTALL_DIR, else $env:CARGO_HOME\bin, else
# ~\.cargo\bin. $env:RAWR_VERSION picks a version such as 0.3.115.
# Windows is best effort: rawr-import, which runs programs with
# dependencies, is not built for Windows yet.
$ErrorActionPreference = 'Stop'

$repository = 'https://github.com/airstrike/rawr'
$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($arch -ne 'X64') {
    throw "rawr: no prebuilt rawr for Windows on $arch"
}
$archive = 'rawr-x86_64-pc-windows-msvc.zip'
if ($env:RAWR_VERSION) {
    $url = "$repository/releases/download/v$($env:RAWR_VERSION.TrimStart('v'))/$archive"
} else {
    $url = "$repository/releases/latest/download/$archive"
}

if ($env:RAWR_INSTALL_DIR) {
    $directory = $env:RAWR_INSTALL_DIR
} elseif ($env:CARGO_HOME) {
    $directory = Join-Path $env:CARGO_HOME 'bin'
} else {
    $directory = Join-Path $HOME '.cargo\bin'
}

$staging = Join-Path ([System.IO.Path]::GetTempPath()) ("rawr-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force $staging | Out-Null
try {
    Write-Host "rawr: downloading $url"
    $file = Join-Path $staging $archive
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $file
    Invoke-WebRequest -UseBasicParsing -Uri "$url.sha256" -OutFile "$file.sha256"
    $expected = ((Get-Content "$file.sha256" -Raw).Trim() -split '\s+')[0]
    $actual = (Get-FileHash -Algorithm SHA256 $file).Hash
    if ($expected -ne $actual.ToLower()) {
        throw "rawr: $archive does not match its published SHA-256"
    }
    Expand-Archive -Path $file -DestinationPath (Join-Path $staging 'files')
    New-Item -ItemType Directory -Force $directory | Out-Null
    foreach ($binary in 'rawr.exe', 'cargo-rawr.exe') {
        Copy-Item -Force (Join-Path $staging "files\$binary") (Join-Path $directory $binary)
    }
} finally {
    Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
}

Write-Host "rawr: installed in $directory"
$path = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not (($path -split ';') -contains $directory)) {
    [Environment]::SetEnvironmentVariable('Path', "$directory;$path", 'User')
    Write-Host "rawr: added $directory to your PATH; open a new terminal to use it"
}
Write-Host 'rawr: start with: rawr new hello; cd hello; rawr run'
