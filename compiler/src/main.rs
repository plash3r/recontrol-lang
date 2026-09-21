use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rcl::ast::{Item, Program};
use rcl::borrowck::BorrowChecker;
use rcl::hir::HirLowerer;
use rcl::llvm_backend::LlvmBackend;
use rcl::mir::MirLowerer;
use rcl::mir_borrow::MirBorrowAnalyzer;
use rcl::mir_move::MirMoveAnalyzer;
use rcl::mir_opt::MirOptimizer;
use rcl::mir_validate::MirValidator;
use rcl::lexer::Lexer;
use rcl::ownership::OwnershipChecker;
use rcl::parser::Parser;
use rcl::sema::SemanticAnalyzer;

const RUNTIME_SOURCE: &str = r#"
use std::ffi::CStr;
use std::io::{self, Write};

#[unsafe(no_mangle)]
pub extern "C" fn rcl_print(s: *const u8) {
    if s.is_null() { return; }
    let bytes = unsafe { CStr::from_ptr(s.cast()).to_bytes() };
    let text = String::from_utf8_lossy(bytes);
    print!("{text}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rcl_println(s: *const u8) {
    if s.is_null() { println!(); return; }
    let bytes = unsafe { CStr::from_ptr(s.cast()).to_bytes() };
    let text = String::from_utf8_lossy(bytes);
    println!("{text}");
}

#[unsafe(no_mangle)]
pub extern "C" fn rcl_print_i8(value: i8) { print!("{value}"); let _ = io::stdout().flush(); }

#[unsafe(no_mangle)]
pub extern "C" fn rcl_println_i8(value: i8) { println!("{value}"); let _ = io::stdout().flush(); }

#[unsafe(no_mangle)]
pub extern "C" fn rcl_print_i32(value: i32) { print!("{value}"); let _ = io::stdout().flush(); }

#[unsafe(no_mangle)]
pub extern "C" fn rcl_println_i32(value: i32) { println!("{value}"); let _ = io::stdout().flush(); }

#[unsafe(no_mangle)]
pub extern "C" fn rcl_check_bounds_i32(index: i32, length: i32) {
    if index < 0 || index >= length {
        eprintln!("rcl: array index {index} is out of bounds for length {length}");
        let _ = io::stderr().flush();
        std::process::exit(1);
    }
}
"#;

fn check_source(path: &str) -> Result<rcl::mir::MirProgram, String> {
    let source = fs::read_to_string(path).map_err(|e| format!("rcl: cannot read {path}: {e}"))?;
    let program = load_program(Path::new(path), &mut Vec::new())?;
    SemanticAnalyzer::check(&program).map_err(|e| format_errors(e.into_iter().map(|x| format_source_error(path, &source, x.span, x.message)).collect()))?;
    BorrowChecker::check(&program).map_err(|e| format_errors(e.into_iter().map(|x| format_source_error(path, &source, x.span, x.message)).collect()))?;
    OwnershipChecker::check(&program).map_err(|e| format_errors(e.into_iter().map(|x| format_source_error(path, &source, x.span, x.message)).collect()))?;

    let hir = HirLowerer::lower(&program);
    let mut mir = MirLowerer::lower(&hir);
    MirOptimizer::optimize(&mut mir);
    MirValidator::validate(&mir).map_err(|e| format_errors(e.into_iter().map(|x| x.message).collect()))?;
    MirMoveAnalyzer::analyze(&mir).map_err(|e| format_errors(e.into_iter().map(|x| x.message).collect()))?;
    MirBorrowAnalyzer::analyze(&mir).map_err(|e| format_errors(e.into_iter().map(|x| x.message).collect()))?;
    Ok(mir)
}

fn load_program(path: &Path, stack: &mut Vec<PathBuf>) -> Result<Program, String> {
    let canonical = path.canonicalize().map_err(|e| format!("rcl: cannot resolve {}: {e}", path.display()))?;
    if let Some(index) = stack.iter().position(|item| item == &canonical) {
        let mut cycle = stack[index..].iter().map(|item| item.display().to_string()).collect::<Vec<_>>();
        cycle.push(canonical.display().to_string());
        return Err(format!("rcl: cyclic import: {}", cycle.join(" -> ")));
    }

    let source = fs::read_to_string(&canonical).map_err(|e| format!("rcl: cannot read {}: {e}", canonical.display()))?;
    let tokens = Lexer::new(&source).tokenize()
        .map_err(|e| format_errors(e.into_iter().map(|x| format_source_error(&canonical.display().to_string(), &source, x.span, x.message)).collect()))?;
    let program = Parser::new(tokens).parse()
        .map_err(|e| format_errors(e.into_iter().map(|x| format_source_error(&canonical.display().to_string(), &source, x.span, x.message)).collect()))?;
    stack.push(canonical.clone());
    let mut items = Vec::new();
    for item in program.items {
        match item {
            Item::Import(import) => {
                let import_path = resolve_import_path(canonical.parent().unwrap_or(Path::new(".")), &import);
                items.extend(load_program(&import_path, stack)?.items);
            }
            item => items.push(item),
        }
    }
    stack.pop();
    Ok(Program { items })
}

fn resolve_import_path(base: &Path, import: &str) -> PathBuf {
    let requested = base.join(import);
    if requested.extension().is_none() {
        requested.with_extension("rcl")
    } else {
        requested
    }
}

fn format_errors(errors: Vec<String>) -> String {
    errors.into_iter().map(|e| format!("error: {e}")).collect::<Vec<_>>().join("\n")
}

fn format_source_error(path: &str, source: &str, span: rcl::lexer::Span, message: String) -> String {
    let span = if span.line == 1 && span.column == 1 && span.length == 0 {
        infer_error_span(source, &message).unwrap_or(span)
    } else {
        span
    };
    if span.line == 1 && span.column == 1 && span.length == 0 {
        return format!("{}: {}", path, message);
    }
    let line_number = span.line.max(1);
    let line = source.lines().nth(line_number.saturating_sub(1)).unwrap_or("");
    let column = span.column.max(1);
    let width = span.length.max(1);
    let marker = format!("{}{}", " ".repeat(column.saturating_sub(1)), "^".repeat(width));
    format!("{}:{}:{}: {}\n  {} | {}\n    | {}", path, line_number, column, message, line_number, line, marker)
}

fn infer_error_span(source: &str, message: &str) -> Option<rcl::lexer::Span> {
    let quoted = message.split('\'').nth(1);
    for (line_index, line) in source.lines().enumerate() {
        let Some(column) = (if let Some(name) = quoted {
            line.find(name)
        } else if message.contains("array length mismatch") {
            line.find('[')
        } else if message.contains("type mismatch") {
            line.find(':').or_else(|| line.find('='))
        } else {
            None
        }) else { continue };
        return Some(rcl::lexer::Span { line: line_index + 1, column: column + 1, length: 1 });
    }
    None
}

fn llvm_path(source: &str) -> PathBuf { Path::new(source).with_extension("ll") }

fn executable_path(source: &str) -> PathBuf {
    let path = Path::new(source);
    #[cfg(windows)]
    { path.with_extension("exe") }
    #[cfg(not(windows))]
    { path.with_extension("") }
}

fn build_llvm(source: &str) -> Result<PathBuf, String> {
    let mir = check_source(source)?;
    let llvm = LlvmBackend::emit(&mir)
        .map_err(|e| format_errors(e.into_iter().map(|x| format!("{}: {}", x.function, x.message)).collect()))?;
    let out = llvm_path(source);
    fs::write(&out, llvm).map_err(|e| format!("rcl: cannot write {}: {e}", out.display()))?;
    Ok(out)
}

fn build_runtime() -> Result<PathBuf, String> {
    let temp = env::temp_dir();
    let source = temp.join(format!("rcl_runtime_{}.rs", std::process::id()));
    #[cfg(windows)]
    let library = temp.join("rcl_runtime_rcl.lib");
    #[cfg(not(windows))]
    let library = temp.join("librcl_runtime_rcl.a");

    fs::write(&source, RUNTIME_SOURCE).map_err(|e| format!("rcl: cannot prepare runtime: {e}"))?;

    let status = Command::new("rustc")
        .args(["--edition", "2024", "--crate-name", "rcl_runtime", "--crate-type", "staticlib"])
        .arg(&source).arg("-o").arg(&library).status()
        .map_err(|e| format!("rcl: cannot execute rustc for the runtime: {e}"))?;

    let _ = fs::remove_file(&source);
    if !status.success() { return Err("rcl: failed to build the Rust runtime".into()); }
    if !library.exists() { return Err(format!("rcl: runtime library was not produced: {}", library.display())); }
    Ok(library)
}

fn build_native(source: &str) -> Result<PathBuf, String> {
    let ll = build_llvm(source)?;
    let runtime = build_runtime()?;
    let out = executable_path(source);

    let _ = fs::remove_file(&out);

    let status = Command::new("clang")
        .arg("-x").arg("ir").arg(&ll)
        .arg("-x").arg("none").arg(&runtime)
        .arg("-o").arg(&out).status()
        .map_err(|e| format!("rcl: cannot execute clang: {e}"))?;

    if !status.success() { return Err("rcl: clang failed while producing the native executable".into()); }
    Ok(out)
}

fn new_project(name: &str) -> Result<(), String> {
    let root = Path::new(name);
    if root.exists() { return Err(format!("rcl: directory already exists: {}", root.display())); }

    fs::create_dir_all(root.join("src")).map_err(|e| format!("rcl: cannot create project: {e}"))?;
    fs::write(root.join("rcl.toml"), format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"))
        .map_err(|e| format!("rcl: cannot write rcl.toml: {e}"))?;
    fs::write(root.join("src/main.rcl"), "fn main() {\n    println(\"Hello, Recontrol!\")\n}\n")
        .map_err(|e| format!("rcl: cannot write src/main.rcl: {e}"))?;
    println!("Created Recontrol project {name}");
    Ok(())
}

fn print_help() {
    println!("Recontrol Lang compiler 0.1.1b");
    println!();
    println!("Usage:");
    println!("  rcl check <file.rcl>       Check source without producing an executable");
    println!("  rcl build <file.rcl>       Build a native executable");
    println!("  rcl run <file.rcl>         Build and run a native executable");
    println!("  rcl emit-llvm <file.rcl>   Emit LLVM IR");
    println!("  rcl new <name>              Create a new project");
    println!("  rcl --version               Show compiler version");
    println!("  rcl --help                  Show this help");
}

fn main() {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") | Some("-V") => println!("recontrolc 0.1.1b"),
        Some("--help") | Some("-h") | None => print_help(),
        Some("new") => match args.next() {
            Some(name) => { if let Err(e) = new_project(&name) { eprintln!("{e}"); std::process::exit(1); } }
            None => { eprintln!("usage: rcl new <project-name>"); std::process::exit(2); }
        },
        Some("check") => match args.next() {
            Some(path) => match check_source(&path) {
                Ok(mir) => println!("OK: semantic, borrow, ownership and MIR checks passed ({} function(s))", mir.functions.len()),
                Err(e) => { eprintln!("{e}"); std::process::exit(1); }
            },
            None => { eprintln!("usage: rcl check <file.rcl>"); std::process::exit(2); }
        },
        Some("emit-llvm") => match args.next() {
            Some(path) => match build_llvm(&path) {
                Ok(out) => println!("LLVM IR: {}", out.display()),
                Err(e) => { eprintln!("{e}"); std::process::exit(1); }
            },
            None => { eprintln!("usage: rcl emit-llvm <file.rcl>"); std::process::exit(2); }
        },
        Some("build") => match args.next() {
            Some(path) => match build_native(&path) {
                Ok(out) => println!("Built: {}", out.display()),
                Err(e) => { eprintln!("{e}"); std::process::exit(1); }
            },
            None => { eprintln!("usage: rcl build <file.rcl>"); std::process::exit(2); }
        },
        Some("run") => match args.next() {
            Some(path) => match build_native(&path) {
                Ok(out) => {
                    let executable = fs::canonicalize(&out).unwrap_or(out);
                    let status = Command::new(&executable).status().unwrap_or_else(|e| {
                        eprintln!("rcl: cannot run {}: {e}", executable.display());
                        std::process::exit(1);
                    });
                    std::process::exit(status.code().unwrap_or(1));
                }
                Err(e) => { eprintln!("{e}"); std::process::exit(1); }
            },
            None => { eprintln!("usage: rcl run <file.rcl>"); std::process::exit(2); }
        },
        Some(command) => {
            eprintln!("rcl: unknown command {command}");
            eprintln!();
            print_help();
            std::process::exit(2);
        }
    }
}
