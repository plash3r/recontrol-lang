# Recontrol Lang

Recontrol Lang (RCL) 0.1.4 — native systems programming language with a Rust compiler implementation and an LLVM IR backend.

## Install

The easiest way to install RCL is from a GitHub Release. The installer downloads only the prebuilt `rcl` compiler.

### Linux x86_64

```bash
curl -fsSL https://raw.githubusercontent.com/plash3r/recontrol-lang/main/install.sh | sh
```

The installer places `rcl` in `~/.local/bin`.

### Windows x86_64

Run PowerShell:

```powershell
irm https://raw.githubusercontent.com/plash3r/recontrol-lang/main/install.ps1 | iex
```

The installer places `rcl.exe` in `%USERPROFILE%\\.rcl\\bin`.

### Build requirements

RCL itself is distributed as a standalone executable. To compile RCL programs, the current native backend uses the host Rust toolchain (`rustc`) for the tiny print/println runtime and Clang for the final LLVM-to-native link.

Install:
- Rust toolchain (`rustc`)
- Clang

Cargo is not required to compile individual `.rcl` programs.

## CLI

```text
rcl check <file.rcl>       Check source
rcl build <file.rcl>       Build a native executable
rcl run <file.rcl>         Build and run a native executable
rcl emit-llvm <file.rcl>   Emit LLVM IR
rcl new <name>              Create a new project
rcl --version               Show compiler version
rcl --help                  Show help
```

## Quick start

```bash
rcl new hello
cd hello
rcl run src/main.rcl
```

Generated program:

```text
Hello, Recontrol!
```

## Example

```rcl
fn main() {
    let message: str = "Hello, Recontrol!"
    println(message)
}
```

## Compiler pipeline

RCL source -> Lexer -> Parser -> AST -> Semantic analysis -> Borrow Checker -> Ownership / Move Checker -> HIR -> MIR -> MIR validation -> MIR Move/Dataflow -> MIR Borrow/Dataflow -> MIR optimization -> LLVM IR -> Clang -> native executable.

The LLVM backend covers native scalar values, strings, arithmetic, comparisons, boolean operations, local storage, control-flow blocks, returns, direct function calls, references as pointers, and print/println runtime calls.

Struct field lowering, arrays, indirect calls, richer reference lowering, target-specific ABI details, and optimization passes remain separate backend milestones.
