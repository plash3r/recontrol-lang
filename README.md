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

# inside a project containing rcl.toml:
rcl check
rcl build
rcl run
rcl test
~~~

The rcl command works directly with .rcl source files.

### Commands

~~~text
rcl check [file.rcl]       Check a file or the current project
rcl build [file.rcl]       Build a file or the current project
rcl run [file.rcl]         Build and run a file or the current project
rcl test                   Build and run tests/*.rcl in the current project
rcl fmt [file.rcl]         Format a file or the current project entry
rcl emit-llvm [file.rcl]   Emit LLVM IR for a file or project
rcl new <name>             Create a new project
rcl --version              Show compiler version
rcl --help                 Show help
~~~

rcl build produces a native executable next to the source file. LLVM IR can be requested explicitly with rcl emit-llvm.

The current compiler uses clang to turn LLVM IR into a native executable. Release bundles include a prebuilt native RCL runtime next to the compiler, so normal `rcl build` and `rcl run` do not invoke `rustc`. No C runtime source is used. Set `RCL_RUNTIME` only when developing with a runtime library stored outside the normal install layout.

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
rcl run
~~~

The generated program should print:

~~~text
Hello, Recontrol!
~~~

Generated project:

~~~text
hello/
├── rcl.toml
├── src/
│   └── main.rcl
└── tests/
~~~

## Example

~~~rcl
fn main() {
    let message: str = "Hello, Recontrol!"
    println(message)
    println(typeof(message))
}
~~~

`typeof(value)` returns the compile-time type name as a string, such as `"str"`, `"i32"`, or `"bool"`.

`len(array)` returns the number of elements in an array as an `i32` value.

Arrays can be printed directly:

~~~rcl
let values = [1, 2, 3]
println(values)
~~~

This prints `1 2 3` for arrays with `str`, `i8`, or `i32` elements.

`print` and `println` accept format strings with `%d`, `%i`, `%s`, or `{}` placeholders:

~~~rcl
println("sum = %d\n", sum)
println("type: {}, value: {}", typeof(sum), sum)
~~~

String escapes include `\n`, `\r`, `\t`, `\\`, and `\"`.

## Local libraries

Source files can import other RCL files with a relative `use` declaration. The `.rcl` extension is optional:

~~~rcl
use "math.rcl"

fn main() {
    println(add(2, 3))
}
~~~

`math.rcl` is compiled as part of the same program. Declarations are private to their source file by default; use `pub fn`, `pub struct`, `pub fn` on methods, and `pub` on struct fields to expose them to another imported file. Private helpers remain usable inside their own source file.

For example, `math.rcl` can export:

~~~rcl
fn internal_helper(): i32 {
    return 40
}

pub fn add(a: i32, b: i32): i32 {
    return a + b
}
~~~

Imports are resolved relative to the file containing the `use` declaration. Cyclic imports, missing files, and cross-file access to private declarations are reported as compiler errors with source locations.

An import can be placed in a namespace:

~~~rcl
use "left.rcl" as left
use "right.rcl" as right

fn main() {
    println(left.value())
    println(right.value())
}
~~~

Namespaced imports do not inject their functions into the importing file's unqualified function namespace. This allows different modules to export functions with the same name. Plain `use "math.rcl"` remains available for unqualified imports.

Enums may carry typed payloads and `match` destructures them safely:

~~~rcl
enum Message {
    Empty
    Number(i32)
    Pair(i32, i32)
}

fn main() {
    let message: Message = Message.Pair(20, 22)

    match message {
        Message.Empty => {
            println(0)
        }
        Message.Number(value) => {
            println(value)
        }
        Message.Pair(left, right) => {
            println(left + right)
        }
    }
}
~~~

The compiler rejects wrong constructor payloads, wrong pattern binding counts,
unknown variants, duplicate arms, and non-exhaustive enum matches.

## Compiler pipeline

RCL source -> Lexer -> Parser -> AST -> Semantic analysis -> Borrow Checker -> Ownership / Move Checker -> HIR -> MIR -> MIR validation -> MIR Move/Dataflow -> MIR Borrow/Dataflow -> MIR optimization -> LLVM IR -> clang/LLVM -> native executable.

## LLVM backend milestone

The LLVM backend covers native scalar values, strings, arithmetic, comparisons, boolean operations, local storage, control-flow blocks, returns, direct function calls, references as pointers, structs, fixed arrays, payload enums, exhaustive match lowering, and print/println runtime calls.

Indirect calls, richer reference lowering, generic monomorphization, target-specific ABI details, and deeper optimization passes remain future backend milestones.


## Defined semantics

The language rules for short-circuit evaluation, integer overflow, integer
division, numeric literal typing, fixed arrays, references, and source
diagnostics are documented in
`docs/language-semantics.md`. These behaviors are regression-tested and should
not be changed accidentally by backend work.
