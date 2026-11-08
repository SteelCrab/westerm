# Install or update westerm from GitHub Releases (Windows).
#
#   irm https://raw.githubusercontent.com/SteelCrab/westerm/main/install.ps1 | iex
#
# $env:WESTERM_VERSION       release tag to install, e.g. v0.1.0 (default: latest)
# $env:WESTERM_INSTALL_DIR   folder for westerm.exe (default: %LOCALAPPDATA%\Programs\westerm)
# $env:WESTERM_RELEASES_URL  another copy of the releases page (mirrors, tests)
#
# Errors use `throw`, not `exit`: with `| iex` the script runs in your shell.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue' # Windows PowerShell downloads much faster without it.
# Older Windows PowerShell may not offer TLS 1.2, which GitHub requires.
if ($PSVersionTable.PSVersion.Major -lt 6) {
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
}

$repo = 'SteelCrab/westerm'
$version = if ($env:WESTERM_VERSION) { $env:WESTERM_VERSION } else { 'latest' }
$installDir = if ($env:WESTERM_INSTALL_DIR) { $env:WESTERM_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\westerm' }
$releases = if ($env:WESTERM_RELEASES_URL) { $env:WESTERM_RELEASES_URL } else { "https://github.com/$repo/releases" }

# ARM64 Windows runs the x64 build through its built-in emulation.
if ($env:PROCESSOR_ARCHITECTURE -notin @('AMD64', 'ARM64')) {
  throw "westerm: no build for $($env:PROCESSOR_ARCHITECTURE)"
}
$target = 'x86_64-pc-windows-msvc'
$asset = "westerm-$target.zip"
$url = if ($version -eq 'latest') { "$releases/latest/download/$asset" } else { "$releases/download/$version/$asset" }

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("westerm-" + [Guid]::NewGuid())
New-Item -ItemType Directory $tmp | Out-Null
try {
  Write-Host "westerm: downloading $url"
  $archive = Join-Path $tmp $asset
  Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $archive
  Invoke-WebRequest -UseBasicParsing -Uri "$url.sha256" -OutFile "$archive.sha256"
  $expected = ((Get-Content "$archive.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
  $actual = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLower()
  if ($expected -ne $actual) { throw "westerm: checksum mismatch for $asset" }

  Expand-Archive -Path $archive -DestinationPath $tmp -Force
  $binary = Join-Path $tmp "westerm-$target\westerm.exe"
  if (-not (Test-Path $binary)) { throw 'westerm: the archive has no westerm.exe' }

  New-Item -ItemType Directory -Force $installDir | Out-Null
  $dest = Join-Path $installDir 'westerm.exe'
  $updating = Test-Path $dest
  # Microsoft's ConPTY goes next to westerm.exe: it passes the Kitty graphics
  # of terminal-browser and terminal-code through. A running westerm keeps
  # these files open; they cannot be overwritten, but they can be renamed.
  foreach ($name in 'westerm.exe', 'conpty.dll', 'OpenConsole.exe', 'conpty-LICENSE.txt') {
    $from = Join-Path $tmp "westerm-$target\$name"
    if (-not (Test-Path $from)) { continue }
    $to = Join-Path $installDir $name
    if (Test-Path $to) {
      Remove-Item -Force "$to.old" -ErrorAction SilentlyContinue
      Move-Item -Force $to "$to.old"
    }
    Copy-Item $from $to
    Remove-Item -Force "$to.old" -ErrorAction SilentlyContinue
  }

  Write-Host "westerm: installed $(& $dest --version)"
  Write-Host "westerm: location: $dest"
  if ($updating) {
    Write-Host 'westerm: if westerm is running, restart it. Open shells may come back once as "restored".'
  }

  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (-not $userPath) { $userPath = '' }
  if (($userPath -split ';') -notcontains $installDir) {
    [Environment]::SetEnvironmentVariable('Path', (($userPath.TrimEnd(';') + ";$installDir").TrimStart(';')), 'User')
    Write-Host "westerm: added $installDir to your user PATH. Open a new terminal, then run: westerm"
  } else {
    Write-Host 'westerm: run: westerm   then open http://127.0.0.1:4317'
  }
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
