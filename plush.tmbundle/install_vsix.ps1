# Install or update the Plush VSCode extension on Windows.
#
# Two modes, picked automatically:
#
#   - If a prebuilt plush.vsix sits next to this script, it is installed as
#     is. This is how the release archive ships, and needs no Node.js.
#   - Otherwise the extension is packaged from this directory first, which
#     does need Node.js. This is the mode used when developing the grammar.
#
# Set CODE_CLI to point at a specific editor CLI, e.g.
#   $env:CODE_CLI = 'code-insiders'; .\install_vsix.ps1

[CmdletBinding()]
param(
    # Keep the generated .vsix instead of deleting it. Only applies when the
    # extension has to be built.
    [switch] $Keep
)

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Locate the VSCode command line tool
function Find-CodeCli {
    if ($env:CODE_CLI) {
        return $env:CODE_CLI
    }

    foreach ($cli in 'code', 'code-insiders', 'codium', 'cursor', 'windsurf') {
        $found = Get-Command $cli -ErrorAction SilentlyContinue
        if ($found) {
            return $found.Source
        }
    }

    # Installed without the "Add to PATH" option checked. Each root is
    # checked for emptiness first: Join-Path throws on a null root, and not
    # every one of these is set on every machine.
    $candidates = @(
        @($env:LOCALAPPDATA,          'Programs\Microsoft VS Code\bin\code.cmd'),
        @($env:ProgramFiles,          'Microsoft VS Code\bin\code.cmd'),
        @(${env:ProgramFiles(x86)},   'Microsoft VS Code\bin\code.cmd'),
        @($env:LOCALAPPDATA,          'Programs\VSCodium\bin\codium.cmd'),
        @($env:LOCALAPPDATA,          'Programs\cursor\resources\app\bin\cursor.cmd')
    )

    foreach ($candidate in $candidates) {
        $root = $candidate[0]
        if ([string]::IsNullOrEmpty($root)) {
            continue
        }

        $path = Join-Path $root $candidate[1]
        if (Test-Path $path) {
            return $path
        }
    }

    return $null
}

$code = Find-CodeCli

if (-not $code) {
    Write-Error @"
no VSCode installation found
If VSCode is installed, run 'Shell Command: Install 'code' command in PATH'
from the command palette, or set CODE_CLI to the path of the CLI.
"@
}

# Shipped alongside this script in the release archive
$vsix = Join-Path $ScriptDir 'plush.vsix'
$built = $false

if (-not (Test-Path $vsix)) {
    if (-not (Get-Command npx -ErrorAction SilentlyContinue)) {
        Write-Error @"
no plush.vsix next to this script and npx was not found
Install Node.js to build the extension from source.
"@
    }

    $version = (Get-Content (Join-Path $ScriptDir 'package.json') | ConvertFrom-Json).version
    $vsix = Join-Path $ScriptDir "plush-$version.vsix"
    $built = $true

    Write-Host "Packaging plush $version..."
    Push-Location $ScriptDir
    try {
        # '@vscode/vsce' must be quoted: a bare leading @ is splatting syntax
        & npx --yes '@vscode/vsce' package --allow-missing-repository --out $vsix
        if ($LASTEXITCODE -ne 0) { throw "vsce package failed" }
    } finally {
        Pop-Location
    }
}

Write-Host "Installing the extension into $code..."
& $code --install-extension $vsix --force
if ($LASTEXITCODE -ne 0) { throw "installing the extension failed" }

if ($built -and -not $Keep) {
    Remove-Item $vsix -Force
} elseif ($built) {
    Write-Host "Kept $vsix"
}

Write-Host ''
Write-Host 'Done. Restart VSCode to start using the extension.'
