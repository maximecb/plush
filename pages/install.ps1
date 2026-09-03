# Plush installer for Windows.
#
#   irm https://maximecb.github.io/plush/install.ps1 | iex
#
# Install and immediately run an example. The piped form above cannot take
# arguments, so an environment variable is used instead:
#
#   $env:PLUSH_RUN_EXAMPLE = 'tremor'; irm https://maximecb.github.io/plush/install.ps1 | iex
#
# Installs into %USERPROFILE%\.plush and puts plush on your PATH.
#
# Environment variables:
#   PLUSH_HOME          install directory (default: %USERPROFILE%\.plush)
#   PLUSH_VERSION       release tag to install (default: latest)
#   PLUSH_RUN_EXAMPLE   example to run once the install finishes
#   PLUSH_NO_PATH       set to 1 to leave PATH untouched

[CmdletBinding()]
param(
    [string] $RunExample = $env:PLUSH_RUN_EXAMPLE,
    [string] $Version    = $(if ($env:PLUSH_VERSION) { $env:PLUSH_VERSION } else { 'latest' }),
    [string] $PlushHome  = $(if ($env:PLUSH_HOME) { $env:PLUSH_HOME } else { Join-Path $HOME '.plush' }),
    [switch] $Force
)

$ErrorActionPreference = 'Stop'
$Repo = 'maximecb/plush'

# Windows PowerShell 5.1 can still default to TLS 1.0, which GitHub rejects
[Net.ServicePointManager]::SecurityProtocol =
    [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function Say  { param([string] $Message) Write-Host "plush: $Message" }
function Fail { param([string] $Message) throw "plush: error: $Message" }

# The VSCode extension ships inside the release archive. Releases made before
# that was the case have no such file, so this stays quiet for them.
function Say-VSCodeHint {
    $script = Join-Path $PlushHome 'install_vsix.ps1'
    if (-not (Test-Path $script)) {
        return
    }

    Say ''
    Say 'To install the VSCode extension, run this, then restart VSCode:'
    Say ''
    Say "    $script"
}

# We only publish an x64 Windows build. Windows on ARM runs it under
# emulation, so it is offered there too.
function Get-Target {
    switch ($env:PROCESSOR_ARCHITECTURE) {
        'AMD64' { return 'plush-x86_64-windows' }
        'ARM64' { return 'plush-x86_64-windows' }
        default { Fail "unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
    }
}

# Newest release tagged v<number>. The releases/latest endpoint is not used:
# the VSCode extension workflow publishes vsix-* releases carrying no
# plush binaries, and one of those would otherwise be picked.
function Resolve-LatestVersion {
    $url = "https://api.github.com/repos/$Repo/releases?per_page=30"

    try {
        $releases = Invoke-RestMethod -Uri $url -UseBasicParsing
    } catch {
        Fail "could not reach the GitHub API: $($_.Exception.Message)"
    }

    # Releases come back newest first
    foreach ($release in $releases) {
        if ($release.tag_name -match '^v[0-9]') {
            return $release.tag_name
        }
    }

    Fail 'could not find a plush release. Set $env:PLUSH_VERSION to pick one.'
}

function Get-InstalledVersion {
    $exe = Join-Path $PlushHome 'bin\plush.exe'

    if (-not (Test-Path $exe)) {
        $command = Get-Command plush -ErrorAction SilentlyContinue
        if (-not $command) { return $null }
        $exe = $command.Source
    }

    try {
        # Output looks like "plush 0.3.0"
        $output = & $exe --version 2>$null
    } catch {
        return $null
    }

    if ($output -match '^plush\s+(\S+)') { return $Matches[1] }
    return $null
}

function Install-Plush {
    param([string] $Tag)

    $target = Get-Target
    $asset  = "$target.zip"
    $base   = "https://github.com/$Repo/releases/download/$Tag"

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("plush-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null

    try {
        Say "downloading $target ($Tag)"

        $archive = Join-Path $tmp $asset

        try {
            Invoke-WebRequest -Uri "$base/$asset" -OutFile $archive -UseBasicParsing
        } catch {
            Fail "download failed. Does release $Tag have an asset named $asset?"
        }

        # Verify the checksum when the release publishes one
        $sums = Join-Path $tmp 'SHA256SUMS'

        try {
            Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile $sums -UseBasicParsing
        } catch {
            $sums = $null
            Say 'SHA256SUMS not available, skipping checksum verification'
        }

        if ($sums) {
            $line = Select-String -Path $sums -Pattern "\s$([regex]::Escape($asset))$" |
                Select-Object -First 1

            if (-not $line) { Fail "no checksum listed for $asset" }

            $expected = ($line.Line -split '\s+')[0]
            $actual   = (Get-FileHash -Path $archive -Algorithm SHA256).Hash

            if ($actual -ne $expected.ToUpper()) {
                Fail "checksum mismatch for $asset (expected $expected, got $actual)"
            }
        }

        Expand-Archive -Path $archive -DestinationPath $tmp -Force

        $unpacked = Join-Path $tmp $target
        $exe      = Join-Path $unpacked 'bin\plush.exe'

        if (-not (Test-Path $exe)) { Fail 'release archive is missing bin\plush.exe' }

        # Check the binary runs before touching the existing install, so a bad
        # download cannot leave the user with nothing
        $version = & $exe --version 2>$null
        if (-not $version) { Fail 'the downloaded binary did not run' }

        if (Test-Path $PlushHome) {
            Say "removing previous install at $PlushHome"
            Remove-Item -Path $PlushHome -Recurse -Force
        }

        $parent = Split-Path -Parent $PlushHome
        if ($parent -and -not (Test-Path $parent)) {
            New-Item -ItemType Directory -Path $parent -Force | Out-Null
        }

        Move-Item -Path $unpacked -Destination $PlushHome

        Say "installed $version to $PlushHome"
    }
    finally {
        Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Update-UserPath {
    $binDir = Join-Path $PlushHome 'bin'

    [Environment]::SetEnvironmentVariable(
        'PLUSH_EXAMPLES_DIR', (Join-Path $PlushHome 'examples'), 'User')

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $userPath) { $userPath = '' }

    $entries = $userPath -split ';' | Where-Object { $_ -ne '' }

    if ($entries -notcontains $binDir) {
        $updated = (@($entries) + $binDir) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
        Say "added $binDir to your PATH"
        $script:NeedsNewShell = $true
    }

    # Make plush usable in this session too, without a restart
    if (($env:Path -split ';') -notcontains $binDir) {
        $env:Path = "$env:Path;$binDir"
    }

    $env:PLUSH_EXAMPLES_DIR = Join-Path $PlushHome 'examples'
}

# --- Main --------------------------------------------------------------------

if ([string]::IsNullOrWhiteSpace($PlushHome) -or $PlushHome -eq $HOME) {
    Fail "refusing to install to '$PlushHome'"
}

$NeedsNewShell = $false

$tag = if ($Version -eq 'latest') { Resolve-LatestVersion } else { $Version }
$want = $tag -replace '^v', ''
$have = Get-InstalledVersion

if (-not $Force -and $have -and $have -eq $want) {
    Say "plush $have is already installed, skipping download"
} else {
    Install-Plush -Tag $tag
}

if ($env:PLUSH_NO_PATH -eq '1') {
    Say "add $(Join-Path $PlushHome 'bin') to your PATH to finish"
} else {
    Update-UserPath
}

if ($RunExample) {
    # Ahead of the example, which takes over the session until it exits
    Say-VSCodeHint
    Say ''
    Say "running example: $RunExample"
    & (Join-Path $PlushHome 'bin\plush.exe') --run-example $RunExample
    # Deliberately not "exit": piping this script into iex runs it in the
    # caller's session, and exit would close their PowerShell window
    return
}

Say ''
Say 'Try an example:'
Say '    plush --run-example tremor'
Say '    plush --run-example night_ride'
Say ''
Say 'To list all available examples:'
Say '    plush --list-examples'

Say-VSCodeHint
Say ''

if ($NeedsNewShell) {
    Say 'Open a new terminal for the PATH change to take effect.'
    Say ''
}
