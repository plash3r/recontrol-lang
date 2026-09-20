# Recontrol Lang

Recontrol Lang (RCL) — native systems programming language with a Rust compiler implementation and an LLVM IR backend.

## Compiler pipeline

RCL source -> Lexer -> Parser -> AST -> Semantic analysis -> Borrow Checker -> Ownership / Move Checker -> HIR -> MIR -> MIR validation -> MIR Move/Dataflow -> MIR Borrow/Dataflow -> MIR optimization -> LLVM IR -> clang/LLVM -> native executable.

## CLI

cargo run -p rcl -- check examples/hello.rcl
cargo run -p rcl -- build examples/hello.rcl
cargo run -p rcl -- run examples/hello.rcl

`rcl build` emits LLVM IR next to the source file as `.ll`.

`rcl run` emits LLVM IR, invokes `clang` with `runtime/rcl_runtime.c`, and runs the native executable.

## Example

fn main() {
    let message: str = "Hello, Recontrol!"
    println(message)
}

## LLVM backend milestone

The first backend covers native scalar values, strings, arithmetic, comparisons, boolean operations, local storage, control-flow blocks, returns, direct function calls, references as pointers, and print/println runtime calls.

Struct field lowering, arrays, indirect calls, richer reference lowering, target-specific ABI details, and optimization passes remain separate backend milestones.
