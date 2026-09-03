#!/usr/bin/env bash
#
# Install or update the Plush VSCode extension. Works on macOS and Linux.
#
# Two modes, picked automatically:
#
#   - If a prebuilt plush.vsix sits next to this script, it is installed as
#     is. This is how the release tarball ships, and needs no Node.js.
#   - Otherwise the extension is packaged from this directory first, which
#     does need Node.js. This is the mode used when developing the grammar.

set -euo pipefail

usage()
{
    cat <<'EOF'
Usage: ./install_vsix.sh [--keep]

Install or update the Plush VSCode extension.

  --keep    Keep the generated .vsix instead of deleting it. Only applies
            when the extension has to be built.

Set CODE_CLI to point at a specific editor CLI, e.g.
  CODE_CLI=code-insiders ./install_vsix.sh
EOF
}

KEEP_VSIX=0

for arg in "$@"; do
    case "$arg" in
        --keep)
            KEEP_VSIX=1
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown option: $arg" >&2
            usage >&2
            exit 1
            ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Locate the VSCode command line tool
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

if ! CODE="$(find_code_cli)"; then
    echo "error: no VSCode installation found" >&2
    echo "If VSCode is installed, run 'Shell Command: Install 'code' command in PATH'" >&2
    echo "from the command palette, or set CODE_CLI to the path of the CLI." >&2
    exit 1
fi

# Shipped alongside this script in the release tarball
VSIX="$SCRIPT_DIR/plush.vsix"
BUILT=0

if [ ! -f "$VSIX" ]; then
    if ! command -v npx > /dev/null 2>&1; then
        echo "error: no plush.vsix next to this script and npx was not found" >&2
        echo "Install Node.js to build the extension from source." >&2
        exit 1
    fi

    VERSION="$(node -p "require('./package.json').version")"
    VSIX="$SCRIPT_DIR/plush-$VERSION.vsix"
    BUILT=1

    echo "Packaging plush $VERSION..."
    npx --yes @vscode/vsce package --allow-missing-repository --out "$VSIX"
fi

echo "Installing the extension into $CODE..."

# The code CLI wrapper unsets NODE_OPTIONS before running, so a Node
# deprecation warning from inside VSCode itself (unrelated to our extension)
# cannot be silenced that way. Filtering it out here is the only way; the
# exit status still comes from $CODE, not from this filtering.
"$CODE" --install-extension "$VSIX" --force \
    2> >(grep -v -e 'DeprecationWarning' -e 'trace-deprecation' >&2)

if [ "$BUILT" -eq 1 ]; then
    if [ "$KEEP_VSIX" -eq 0 ]; then
        rm -f "$VSIX"
    else
        echo "Kept $VSIX"
    fi
fi

echo
echo "Done. Restart VSCode to start using the extension."
