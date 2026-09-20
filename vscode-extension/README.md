# Recontrol Lang — VS Code

Official VS Code language support for Recontrol Lang (.rcl).

You do **not** need Node.js, npm, Cargo, or the Rust toolchain to install the extension.

## Install the ready-made extension

The project automatically builds a `.vsix` package in GitHub Actions.

### Option 1 — download the latest VSIX

Open the repository's **Releases** page and download the latest file named:

`recontrol-lang-<version>.vsix`

Then in VS Code:

1. Press `Ctrl+Shift+X`.
2. Open the `...` menu.
3. Select **Install from VSIX...**.
4. Select the downloaded `.vsix`.
5. Reload VS Code if prompted.

### Option 2 — install from a terminal

If the VS Code `code` command is available:

**Windows PowerShell**
```powershell
.\install.ps1
```

**Linux / macOS**
```bash
chmod +x install.sh
./install.sh
```

The scripts download the latest published VSIX automatically and pass it directly to VS Code. There is nothing to build.

## Automatic builds

The repository contains a GitHub Actions workflow at `.github/workflows/vscode-extension.yml`.

It:

- validates the extension manifest;
- packages the extension with `@vscode/vsce`;
- uploads the VSIX as a workflow artifact;
- creates a GitHub Release when a tag matching `vscode-v*` is pushed;
- attaches the ready-to-install `.vsix` to that release.

For a new extension release, the maintainer only needs to create a version tag such as `vscode-v0.1.0`. The build itself is performed by GitHub Actions.

## Supported syntax

- Keywords: `fn`, `struct`, `impl`, `let`, `mut`
- Control flow: `if`, `else`, `for`, `while`, `do`, `return`
- Primitive types: `i8` through `i256`, `u8` through `u256`, `f32`, `f64`, `f128`, `bool`, `char`, `str`, `void`
- References: `&T`, `&mut T`
- Integer and floating-point literals
- Strings and escape sequences
- `//` comments
- Operators and punctuation
- Function definitions and calls
- Builtins `print` and `println`

## Development

You normally do not need to build the extension yourself.

If you want to work on the extension locally, the package can still be built with:

```bash
cd vscode-extension
npx @vscode/vsce package
```
