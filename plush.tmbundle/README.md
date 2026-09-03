# Plush TextMate Grammar

A syntax highlighting extension for [Plush](https://github.com/maximecb/plush) (.psh) files in Visual Studio Code.

## Installation

### If you installed Plush with the installer

The extension ships inside the release, so there is nothing to download:

```sh
~/.plush/install_vsix.sh          # macOS and Linux
```
```powershell
~\.plush\install_vsix.ps1         # Windows
```

Then restart VS Code.

### From this repository

Running the same script from `plush.tmbundle/` packages the extension from
source first, then installs it:

```sh
./install_vsix.sh
```

That mode needs Node.js, since it has to build the `.vsix`. Both modes need a
VS Code CLI; set `CODE_CLI` to pick a specific one (e.g.
`CODE_CLI=code-insiders ./install_vsix.sh`).

### Using .vsix

1. Download the `.vsix` file from the [releases](https://github.com/maximecb/plush/releases) page.

2. Install the extension:
    - Press Ctrl+Shift+P (or Cmd+Shift+P on macOS)
    - Type `Extensions: Install from VSIX...` and press Enter
    - Select the `.vsix` file

3. Reload VS Code:
    - Press `Ctrl+Shift+P` (or `Cmd+Shift+P` on macOS)
    - Type `Developer: Reload Window` and press Enter

### Manually

1. Clone this repository

2. Locate your VS Code extensions directory:
    - Windows: `%USERPROFILE%\.vscode\extensions\`
    - macOS/Linux: `~/.vscode/extensions/`

3. Copy `plush.tmbundle` to your VS Code extensions directory

4. Reload VS Code:
    - Press `Ctrl+Shift+P` (or `Cmd+Shift+P` on macOS)
    - Type `Developer: Reload Window` and press Enter

## Limitations

- Dynamic typing: Since TextMate grammars rely on regex-based highlighting, some dynamically typed sections may not highlight properly

## Sources and Guides

- [Your First Extension](https://code.visualstudio.com/api/get-started/your-first-extension) by Visual Studio Code
- [Create Custom Syntax Highlighting in VS Code](https://www.youtube.com/watch?v=5msZv-nKebI) by Tommy Ngo
- [Language Grammars](https://macromates.com/manual/en/language_grammars) by TextMate
