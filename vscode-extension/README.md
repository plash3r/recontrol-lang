# Recontrol Lang — VS Code

Syntax highlighting for Recontrol Lang source files.

## Install locally

From this directory:

```bash
cd vscode-extension
npm install
npx vsce package
```

If `icons/recontrol.png` does not exist yet, create it with PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\create-icon.ps1
```

Then install the generated `.vsix` in VS Code:

1. Open **Extensions**.
2. Click **...**.
3. Choose **Install from VSIX...**.
4. Select the generated `.vsix` file.

After installation, opening a `.rcl` file automatically selects **Recontrol Lang**.

## Supported syntax

- Keywords: `fn`, `struct`, `impl`, `let`, `mut`
- Control flow: `if`, `else`, `for`, `while`, `do`, `return`
- Primitive types: `i8` through `i256`, `u8` through `u256`, `f32`, `f64`, `f128`, `bool`, `char`, `str`, `void`
- References: `&T`, `&mut T`
- Numbers with integer suffixes such as `42u64`
- Strings and escape sequences
- `//` comments
- Operators and punctuation
- Function definitions and calls
- Builtins `print`, `println`, and `typeof`
- `.rcl` files use the Recontrol icon without replacing the active VS Code file-icon theme.
