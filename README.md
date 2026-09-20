# Recontrol Lang

Recontrol Lang (RCL) — native systems programming language with a Rust compiler implementation and an LLVM IR backend.

## Install

The easiest way to install RCL is from a GitHub Release. The installer downloads a prebuilt compiler, detects the platform, verifies the release checksum when available, installs atomically, and does not require Rust or Cargo.

### Linux x86_64

~~~bash
curl -fsSL https://raw.githubusercontent.com/plash3r/recontrol-lang/main/install.sh | sh
~~~

The installer places rcl in ~/.local/bin. You can select a release with RCL_VERSION and a custom directory with RCL_INSTALL_DIR.

If needed:

~~~bash
export PATH="$HOME/.local/bin:$PATH"
~~~

### Windows x86_64

Run PowerShell:

~~~powershell
irm https://raw.githubusercontent.com/plash3r/recontrol-lang/main/install.ps1 | iex
~~~

The installer places rcl.exe in %USERPROFILE%\\.rcl\\bin, adds that directory to the user PATH, and verifies the release checksum when available.

Open a new terminal afterwards.

For a specific release, set RCL_VERSION before running the installer. Private repositories can be installed by setting RCL_GITHUB_TOKEN (or GH_TOKEN) in the environment.

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

The current compiler uses clang to turn LLVM IR into a native executable. The compiler contains the small Rust runtime required by the current print and println builtins. No C runtime source is used.

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

RCL source -> Lexer -> Parser -> AST -> Semantic analysis -> Borrow Checker -> Ownership / Move Checker -> HIR -> MIR -> MIR validation -> MIR Move/Dataflow -> MIR Borrow/Dataflow -> MIR optimization -> LLVM IR -> clang/LLVM -> native executable.

## LLVM backend milestone

The first backend covers native scalar values, strings, arithmetic, comparisons, boolean operations, local storage, control-flow blocks, returns, direct function calls, references as pointers, and print/println runtime calls.

Struct field lowering, arrays, indirect calls, richer reference lowering, target-specific ABI details, and optimization passes remain separate backend milestones.
