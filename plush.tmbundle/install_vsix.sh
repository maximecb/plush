#!/usr/bin/env bash
#
# Package the Plush extension into a .vsix and install/update it locally.
# Works on macOS and Linux. Requires Node.js (for npx) and a VS Code CLI.
#
# Usage: ./install_vsix.sh [--keep]
#
#   --keep   Don't delete the generated .vsix after installing
#
# Set CODE_CLI to point at a specific editor CLI, e.g.
#   CODE_CLI=code-insiders ./install_vsix.sh

set -euo pipefail

KEEP_VSIX=0

for arg in "$@"; do
    case "$arg" in
        --keep)
            KEEP_VSIX=1
            ;;
        -h|--help)
            sed -n '3,11p' "$0" | sed 's|^# \{0,1\}||'
            exit 0
            ;;
        *)
            echo "error: unknown option: $arg" >&2
            exit 1
            ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Locate the VS Code command line tool
find_code_cli()
{
    if [ -n "${CODE_CLI:-}" ]; then
        echo "$CODE_CLI"
        return 0
    fi

    local cli
    for cli in code code-insiders codium cursor windsurf; do
        if command -v "$cli" > /dev/null 2>&1; then
            echo "$cli"
            return 0
        fi
    done

    # On macOS the CLI often isn't on PATH, look inside the app bundles
    local path
    for path in \
        "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code" \
        "$HOME/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code" \
        "/Applications/VSCodium.app/Contents/Resources/app/bin/codium" \
        "/Applications/Cursor.app/Contents/Resources/app/bin/cursor"
    do
        if [ -x "$path" ]; then
            echo "$path"
            return 0
        fi
    done

    return 1
}

if ! command -v npx > /dev/null 2>&1; then
    echo "error: npx not found, install Node.js first" >&2
    exit 1
fi

if ! CODE="$(find_code_cli)"; then
    echo "error: no VS Code CLI found" >&2
    echo "On macOS, run 'Shell Command: Install code command in PATH' from the" >&2
    echo "command palette, or set CODE_CLI to the CLI path." >&2
    exit 1
fi

VERSION="$(node -p "require('./package.json').version")"
VSIX="$SCRIPT_DIR/plush-$VERSION.vsix"

echo "Packaging plush $VERSION..."
npx --yes @vscode/vsce package --allow-missing-repository --out "$VSIX"

echo "Installing into $CODE..."
"$CODE" --install-extension "$VSIX" --force

if [ "$KEEP_VSIX" -eq 0 ]; then
    rm -f "$VSIX"
else
    echo "Kept $VSIX"
fi

echo
echo "Done. Reload the window (Developer: Reload Window) to pick up the update."
