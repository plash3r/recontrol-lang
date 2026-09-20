# Recontrol Lang

Recontrol Lang (RCL) — native systems programming language with a Rust compiler implementation and an LLVM IR backend.

## Compiler pipeline

RCL source -> Lexer -> Parser -> AST -> Semantic analysis -> Borrow Checker -> Ownership / Move Checker -> HIR -> MIR -> MIR validation -> MIR Move/Dataflow -> MIR Borrow/Dataflow -> MIR optimization -> LLVM IR -> clang/LLVM -> native executable.

## CLI

During compiler development you can run the compiler through Cargo:

```bash
cargo run -p rcl -- check examples/hello.rcl
```

For a normal standalone installation, install the compiler itself:

```bash
cargo install --path compiler
```

After installation, Cargo is no longer part of the normal RCL workflow:

```bash
rcl --version
rcl check examples/hello.rcl
rcl build examples/hello.rcl
rcl run examples/hello.rcl
```

The `rcl` command works directly with `.rcl` source files.

### Commands

```text
rcl check <file.rcl>       Check source
rcl build <file.rcl>       Build a native executable
rcl run <file.rcl>         Build and run a native executable
rcl emit-llvm <file.rcl>   Emit LLVM IR
rcl new <name>             Create a new project
rcl --version              Show compiler version
rcl --help                 Show help
```

`rcl build` produces a native executable next to the source file. LLVM IR can be requested explicitly with `rcl emit-llvm`.

The compiler contains the small Rust runtime required by the current `print` and `println` builtins, so the installed CLI no longer depends on the repository checkout or the workspace `rcl-runtime` package. The runtime is compiled directly with `rustc` and linked by `clang`. No C runtime source is used.

## Quick start

Requirements:
- Rust stable toolchain for installing RCL and building its embedded Rust runtime
- LLVM/Clang available as `clang` in PATH

Install:

```bash
git clone https://github.com/plash3r/recontrol-lang.git
cd recontrol-lang
cargo install --path compiler
```

Then:

```bash
rcl new hello
cd hello
rcl run src/main.rcl
```

The program should print:

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

## LLVM backend milestone

The first backend covers native scalar values, strings, arithmetic, comparisons, boolean operations, local storage, control-flow blocks, returns, direct function calls, references as pointers, and print/println runtime calls.

Struct field lowering, arrays, indirect calls, richer reference lowering, target-specific ABI details, and optimization passes remain separate backend milestones.
