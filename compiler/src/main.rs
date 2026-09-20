use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn check_source(path: &str) -> Result<rcl::mir::MirProgram, String> {
    let source = fs::read_to_string(path).map_err(|e| format!("rcl: cannot read {path}: {e}"))?;
    let tokens = Lexer::new(&source).tokenize()
        .map_err(|e| format_errors(e.into_iter().map(|x| format!("{}:{}: {}", x.span.line, x.span.column, x.message)).collect()))?;
    let program = Parser::new(tokens).parse()
        .map_err(|e| format_errors(e.into_iter().map(|x| format!("{}:{}: {}", x.span.line, x.span.column, x.message)).collect()))?;
    SemanticAnalyzer::check(&program).map_err(|e| format_errors(e.into_iter().map(|x| x.message).collect()))?;
    BorrowChecker::check(&program).map_err(|e| format_errors(e.into_iter().map(|x| x.message).collect()))?;
    OwnershipChecker::check(&program).map_err(|e| format_errors(e.into_iter().map(|x| x.message).collect()))?;

    let hir = HirLowerer::lower(&program);
    let mut mir = MirLowerer::lower(&hir);
    MirOptimizer::optimize(&mut mir);
    MirValidator::validate(&mir).map_err(|e| format_errors(e.into_iter().map(|x| x.message).collect()))?;
    MirMoveAnalyzer::analyze(&mir).map_err(|e| format_errors(e.into_iter().map(|x| x.message).collect()))?;
    MirBorrowAnalyzer::analyze(&mir).map_err(|e| format_errors(e.into_iter().map(|x| x.message).collect()))?;
    Ok(mir)
}

fn format_errors(errors: Vec<String>) -> String {
    errors.into_iter().map(|e| format!("error: {e}")).collect::<Vec<_>>().join("\n")
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

fn runtime_library() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| format!("rcl: cannot locate compiler executable: {e}"))?;
    let dir = exe.parent().ok_or_else(|| "rcl: compiler has no executable directory".to_string())?;
    #[cfg(windows)]
    let name = "rcl_runtime.lib";
    #[cfg(not(windows))]
    let name = "librcl_runtime.a";
    let path = dir.join(name);
    if !path.exists() {
        return Err(format!(
            "rcl: bundled runtime not found: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn build_native(source: &str) -> Result<PathBuf, String> {
    let ll = build_llvm(source)?;
    let runtime = runtime_library()?;
    let out = executable_path(source);

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
    println!("Recontrol Lang compiler 0.1.0");
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
        Some("--version") | Some("-V") => println!("recontrolc 0.1.0"),
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
                    let status = Command::new(&out).status().unwrap_or_else(|e| {
                        eprintln!("rcl: cannot run {}: {e}", out.display());
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
