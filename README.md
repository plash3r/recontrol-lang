# Recontrol Lang

Recontrol Lang (RCL) 0.1.2 — native systems programming language with a Rust compiler implementation and an LLVM IR backend.

## Install

The easiest way to install RCL is from a GitHub Release. The installer downloads a prebuilt rcl compiler, so users do not need Rust or Cargo to install the compiler.

### Linux x86_64

~~~bash
curl -fsSL https://raw.githubusercontent.com/plash3r/recontrol-lang/main/install.sh | sh
~~~

The installer places rcl in ~/.local/bin.

If needed:

~~~bash
export PATH="$HOME/.local/bin:$PATH"
~~~

### Windows x86_64

Run PowerShell:

~~~powershell
irm https://raw.githubusercontent.com/plash3r/recontrol-lang/main/install.ps1 | iex
~~~

The installer places rcl.exe in %USERPROFILE%\\.rcl\\bin and adds that directory to the user PATH.

Open a new terminal afterwards.

### Verify installation

~~~text
rcl --version
~~~

## CLI

Once installed, Cargo is not part of the normal RCL workflow:

~~~bash
rcl check main.rcl
rcl build main.rcl
rcl run main.rcl
rcl emit-llvm main.rcl
rcl new hello
~~~

The rcl command works directly with .rcl source files.

### Commands

~~~text
rcl check <file.rcl>       Check source
rcl build <file.rcl>       Build a native executable
rcl run <file.rcl>         Build and run a native executable
rcl emit-llvm <file.rcl>   Emit LLVM IR
rcl new <name>              Create a new project
rcl --version               Show compiler version
rcl --help                  Show help
~~~

rcl build produces a native executable next to the source file. LLVM IR can be requested explicitly with rcl emit-llvm.

The compiler ships with its native LLVM toolchain. The release installer installs RCL, its Rust runtime library, and a private clang/LLD toolchain beside it. End users do not need Rust, Cargo, clang, LLVM, or a separate C/C++ compiler to build RCL programs. The operating system still provides its normal native system libraries. No C runtime source is used.

## Development installation

If you are developing RCL itself, you can install the compiler from the repository:

~~~bash
git clone https://github.com/plash3r/recontrol-lang.git
cd recontrol-lang
cargo install --path compiler
~~~

This requires the Rust toolchain. Normal RCL users should use the prebuilt installer above.

## Quick start

~~~bash
rcl new hello
cd hello
rcl run src/main.rcl
~~~

The generated program should print:

~~~text
Hello, Recontrol!
~~~

Generated project:

~~~text
hello/
├── rcl.toml
└── src/
    └── main.rcl
~~~

## Example

~~~rcl
fn main() {
    let message: str = "Hello, Recontrol!"
    println(message)
}
~~~

## Compiler pipeline

RCL source -> Lexer -> Parser -> AST -> Semantic analysis -> Borrow Checker -> Ownership / Move Checker -> HIR -> MIR -> MIR validation -> MIR Move/Dataflow -> MIR Borrow/Dataflow -> MIR optimization -> LLVM IR -> bundled clang/LLD -> native executable.

## LLVM backend milestone

The first backend covers native scalar values, strings, arithmetic, comparisons, boolean operations, local storage, control-flow blocks, returns, direct function calls, references as pointers, and print/println runtime calls.

Struct field lowering, arrays, indirect calls, richer reference lowering, target-specific ABI details, and optimization passes remain separate backend milestones.
