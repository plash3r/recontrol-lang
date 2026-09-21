# Recontrol language semantics

This document records behavior that user programs may rely on. Changes to these
rules are language changes and should be covered by whole-program regression
tests.

## Expression evaluation

Operands are evaluated from left to right unless a construct explicitly changes
control flow.

`&&` and `||` are short-circuiting operators:

- `left && right` evaluates `right` only when `left` is `true`.
- `left || right` evaluates `right` only when `left` is `false`.

The right-hand side may therefore contain work that would be invalid or unsafe
when the left-hand side determines the result, such as guarded integer division.

## Loop control

RCL supports condition-controlled `while`, post-tested `do ... while`,
C-style `for (...; ...; ...)`, and Rust-like infinite `loop { ... }`
statements.

`break` exits the innermost loop. `continue` advances the innermost loop
according to its kind:

- in `while`, control returns to the condition;
- in `do ... while`, control advances to the trailing condition;
- in `for`, control advances to the update expression before re-checking the
  condition;
- in `loop`, control returns to the beginning of the loop body.

Using `break` or `continue` outside a loop is a compile-time error.

## Integer arithmetic

RCL integer types have fixed widths: signed and unsigned 8, 16, 32, 64, 128,
and 256 bit integers.

For `+`, `-`, and `*`, arithmetic wraps modulo 2^N where N is the width of
the result type. LLVM code generation intentionally does not attach `nsw` or
`nuw` flags.

Integer `/` and `%` are checked operations:

- a zero divisor terminates the program with an RCL runtime diagnostic;
- signed `MIN / -1` and `MIN % -1` are rejected at runtime as signed
  division overflow rather than being left to LLVM undefined/poison behavior.

Floating-point division and remainder use LLVM/IEEE floating-point behavior and
are not subject to the integer zero-divisor check.

## Numeric literals and conversions

A numeric literal with an explicit suffix has that exact type, for example
`10u64`, `20i256`, or `1.5f64`.

An unsuffixed integer literal is initially represented as `i32`, but it may be
contextually accepted for another integer type when the literal value is within
that type's range. This is literal typing, not a general implicit integer
conversion.

RCL does not currently perform arbitrary implicit numeric conversions between
already-typed integer or floating-point values. Programs should use values of
matching types.

Range checks for `i256` and `u256` operate directly on decimal text and do
not pass through `u128`.

## Arrays

A fixed array's length is part of its type.

`i32[3]` and `i32[100]` are distinct types. An array literal has a fixed
length derived from its element count. Passing a fixed array to a function
therefore preserves the required length in the function signature.

Indexing is bounds checked by the native runtime.

A future dynamically sized slice type will carry both a pointer and a length;
it is intentionally distinct from fixed arrays.

## Enums and match

Enums are first-class value types. Each declared variant has a stable
discriminant chosen by declaration order. Variants may be fieldless or carry
one or more typed payload values, and constructing an enum does not require a
heap allocation.

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

The constructor payload count and payload types are checked at compile time.
A match pattern must bind exactly the number of payload values declared by that
variant. The special binding name `_` discards a payload value.

A `match` over an enum must cover every declared variant exactly once.
Unknown variants and duplicate arms are compile-time errors. A non-exhaustive
match is also a compile-time error, so adding a new enum variant cannot silently
fall through at runtime.

Generic enum parameters are the next type-system layer. Once those are
monomorphized, the same payload representation is intended to power
`Option<T>` and `Result<T, E>`.

## References and ownership

A shared reference `&T` is copyable.

A mutable reference `&mut T` represents exclusive access and is not `Copy`.
Assigning or passing a mutable reference by value therefore follows move
semantics.

While a mutable borrow is active, the borrowed value may not be read, mutated,
or borrowed again. While shared borrows are active, mutation and mutable
borrowing are rejected.

Borrow diagnostics include the conflicting use and, when available, the source
location where the active borrow began.

## Source locations

Lexer tokens and AST nodes carry a source identifier and a source range.
Semantic, ownership, and AST borrow diagnostics use these ranges directly.
Imported files retain their own source identities, so an error in an imported
file is reported against that file rather than guessed by searching message
text.

## Compatibility policy

Whenever one of these rules changes, add or update:

1. a Rust unit/integration test for the affected compiler layer; and
2. a whole-program `.rcl` test when the behavior is observable by user code.
