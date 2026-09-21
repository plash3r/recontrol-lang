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

#[derive(Debug)]
struct SourceFile {
    path: PathBuf,
    source: String,
}


#[derive(Debug, Clone)]
struct SourceFile {
    path: PathBuf,
    source: String,
}

fn check_source(path: &str) -> Result<rcl::mir::MirProgram, String> {
    let mut sources = Vec::new();
    let program = load_program(Path::new(path), &mut Vec::new(), &mut sources)?;

    SemanticAnalyzer::check(&program).map_err(|errors| {
        format_errors(errors.into_iter()
            .map(|error| format_source_error(&sources, error.span, error.message))
            .collect())
    })?;
    BorrowChecker::check(&program).map_err(|errors| {
        format_errors(errors.into_iter()
            .map(|error| format_borrow_error(&sources, error))
            .collect())
    })?;
    OwnershipChecker::check(&program).map_err(|errors| {
        format_errors(errors.into_iter()
            .map(|error| format_source_error(&sources, error.span, error.message))
            .collect())
    })?;

    let hir = HirLowerer::lower(&program);
    let mut mir = MirLowerer::lower(&hir);
    MirOptimizer::optimize(&mut mir);
    MirValidator::validate(&mir)
        .map_err(|errors| format_errors(errors.into_iter().map(|error| error.message).collect()))?;
    MirMoveAnalyzer::analyze(&mir)
        .map_err(|errors| format_errors(errors.into_iter().map(|error| error.message).collect()))?;
    MirBorrowAnalyzer::analyze(&mir)
        .map_err(|errors| format_errors(errors.into_iter().map(|error| error.message).collect()))?;
    Ok(mir)
}

fn load_program(
    path: &Path,
    stack: &mut Vec<PathBuf>,
    sources: &mut Vec<SourceFile>,
) -> Result<Program, String> {
    let canonical = path.canonicalize()
        .map_err(|e| format!("rcl: cannot resolve {}: {e}", path.display()))?;
    if let Some(index) = stack.iter().position(|item| item == &canonical) {
        let mut cycle = stack[index..]
            .iter()
            .map(|item| item.display().to_string())
            .collect::<Vec<_>>();
        cycle.push(canonical.display().to_string());
        return Err(format!("rcl: cyclic import: {}", cycle.join(" -> ")));
    }

    let source = fs::read_to_string(&canonical)
        .map_err(|e| format!("rcl: cannot read {}: {e}", canonical.display()))?;
    let source_id = sources.len();
    sources.push(SourceFile {
        path: canonical.clone(),
        source: source.clone(),
    });

    let tokens = Lexer::with_source_id(&source, source_id).tokenize()
        .map_err(|errors| format_errors(
            errors.into_iter()
                .map(|error| format_source_error(sources, error.span, error.message))
                .collect(),
        ))?;
    let program = Parser::new(tokens).parse()
        .map_err(|errors| format_errors(
            errors.into_iter()
                .map(|error| format_source_error(sources, error.span, error.message))
                .collect(),
        ))?;

    stack.push(canonical.clone());
    let mut items = Vec::new();
    for item in program.items {
        match item {
            Item::Import(import) => {
                let import_path = resolve_import_path(
                    canonical.parent().unwrap_or(Path::new(".")),
                    &import.path,
                );
                items.extend(load_program(&import_path, stack, sources)?.items);
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
    errors.into_iter()
        .map(|error| format!("error: {error}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_source_error(
    sources: &[SourceFile],
    span: rcl::lexer::Span,
    message: String,
) -> String {
    let Some(file) = sources.get(span.source_id) else {
        return message;
    };

    let line_number = span.line.max(1);
    let line = file.source.lines()
        .nth(line_number.saturating_sub(1))
        .unwrap_or("");
    let column = span.column.max(1);
    let width = if span.end_line == span.line {
        span.length.max(1).min(line.chars().count().saturating_sub(column - 1).max(1))
    } else {
        1
    };
    let marker = format!(
        "{}{}",
        " ".repeat(column.saturating_sub(1)),
        "^".repeat(width),
    );

    format!(
        "{}:{}:{}: {}\n  {} | {}\n    | {}",
        file.path.display(),
        line_number,
        column,
        message,
        line_number,
        line,
        marker,
    )
}

fn format_borrow_error(
    sources: &[SourceFile],
    error: rcl::borrowck::BorrowError,
) -> String {
    let mut text = format_source_error(sources, error.span, error.message);
    if let Some((span, label)) = error.secondary {
        text.push_str("\n  note: ");
        text.push_str(&format_source_error(sources, span, label));
    }
    text
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
    if let Some(path) = env::var_os("RCL_RUNTIME") {
        let path = PathBuf::from(path);
        if path.is_file() { return Ok(path); }
        return Err(format!("rcl: RCL_RUNTIME does not point to a file: {}", path.display()));
    }

    let executable = env::current_exe().map_err(|e| format!("rcl: cannot locate the compiler executable: {e}"))?;
    let bin_dir = executable.parent().unwrap_or(Path::new("."));

    #[cfg(windows)]
    let file_name = "rcl-runtime.lib";
    #[cfg(not(windows))]
    let file_name = "librcl_runtime.a";

    let mut candidates = vec![bin_dir.join(file_name), bin_dir.join("runtime").join(file_name)];
    if let Some(prefix) = bin_dir.parent() {
        candidates.push(prefix.join("lib").join("recontrol").join(file_name));
    }

    for candidate in candidates {
        if candidate.is_file() { return Ok(candidate); }
    }

    Err("rcl: runtime library not found next to the compiler; reinstall Recontrol or set RCL_RUNTIME".into())
}

fn build_native(source: &str) -> Result<PathBuf, String> {
    let ll = build_llvm(source)?;
    let runtime = runtime_library()?;
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

fn project_source() -> Result<PathBuf, String> {
    let root = env::current_dir().map_err(|e| format!("rcl: cannot read current directory: {e}"))?;
    let manifest = root.join("rcl.toml");
    let text = fs::read_to_string(&manifest).map_err(|_| {
        "rcl: no source file was given and rcl.toml was not found in the current directory".to_string()
    })?;

    let mut section = String::new();
    let mut entry: Option<String> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        if section == "build" {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "entry" {
                    let value = value.trim().trim_matches('"').trim_matches('\'');
                    if !value.is_empty() { entry = Some(value.to_string()); }
                }
            }
        }
    }

    let source = root.join(entry.unwrap_or_else(|| "src/main.rcl".into()));
    if !source.is_file() {
        return Err(format!("rcl: project entry source does not exist: {}", source.display()));
    }
    Ok(source)
}

fn resolve_source(argument: Option<String>) -> Result<String, String> {
    let path = match argument {
        Some(path) => PathBuf::from(path),
        None => project_source()?,
    };
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("rcl: source path is not valid UTF-8: {}", path.display()))
}

fn collect_rcl_tests(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() { return Ok(()); }
    for entry in fs::read_dir(dir).map_err(|e| format!("rcl: cannot read {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("rcl: cannot read test entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rcl_tests(&path, out)?;
        } else if path.extension().and_then(|x| x.to_str()) == Some("rcl") {
            out.push(path);
        }
    }
    Ok(())
}

fn run_project_tests() -> Result<(), String> {
    let root = env::current_dir().map_err(|e| format!("rcl: cannot read current directory: {e}"))?;
    if !root.join("rcl.toml").is_file() {
        return Err("rcl: rcl test must be run from a project containing rcl.toml".into());
    }

    let mut tests = Vec::new();
    collect_rcl_tests(&root.join("tests"), &mut tests)?;
    tests.sort();

    if tests.is_empty() {
        let source = project_source()?;
        let source = source.to_string_lossy().into_owned();
        check_source(&source)?;
        println!("test project-entry ... ok");
        return Ok(());
    }

    let mut passed = 0usize;
    for test in tests {
        let source = test.to_string_lossy().into_owned();
        let executable = build_native(&source)?;
        let executable = fs::canonicalize(&executable).unwrap_or(executable);
        let status = Command::new(&executable)
            .status()
            .map_err(|e| format!("rcl: cannot run test {}: {e}", test.display()))?;
        if !status.success() {
            return Err(format!("rcl: test failed: {}", test.display()));
        }
        println!("test {} ... ok", test.display());
        passed += 1;
    }
    println!("{} test program(s) passed", passed);
    Ok(())
}

fn new_project(name: &str) -> Result<(), String> {
    let root = Path::new(name);
    if root.exists() { return Err(format!("rcl: directory already exists: {}", root.display())); }

    fs::create_dir_all(root.join("src")).map_err(|e| format!("rcl: cannot create project: {e}"))?;
    fs::create_dir_all(root.join("tests")).map_err(|e| format!("rcl: cannot create test directory: {e}"))?;
    fs::write(
        root.join("rcl.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.rcl\"\n"),
    ).map_err(|e| format!("rcl: cannot write rcl.toml: {e}"))?;
    fs::write(root.join("src/main.rcl"), "fn main() {\n    println(\"Hello, Recontrol!\")\n}\n")
        .map_err(|e| format!("rcl: cannot write src/main.rcl: {e}"))?;
    println!("Created Recontrol project {name}");
    Ok(())
}

fn print_help() {
    println!("Recontrol Lang compiler 0.1.2");
    println!();
    println!("Usage:");
    println!("  rcl check [file.rcl]       Check a file or the current project");
    println!("  rcl build [file.rcl]       Build a file or the current project");
    println!("  rcl run [file.rcl]         Build and run a file or the current project");
    println!("  rcl test                   Build and run tests/*.rcl in the current project");
    println!("  rcl emit-llvm [file.rcl]   Emit LLVM IR for a file or project");
    println!("  rcl new <name>             Create a new project");
    println!("  rcl --version              Show compiler version");
    println!("  rcl --help                 Show this help");
}

fn main() {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") | Some("-V") => println!("recontrolc 0.1.2"),
        Some("--help") | Some("-h") | None => print_help(),
        Some("new") => match args.next() {
            Some(name) => {
                if let Err(e) = new_project(&name) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
            None => {
                eprintln!("usage: rcl new <project-name>");
                std::process::exit(2);
            }
        },
        Some("check") => match resolve_source(args.next()) {
            Ok(path) => match check_source(&path) {
                Ok(mir) => println!("OK: semantic, borrow, ownership and MIR checks passed ({} function(s))", mir.functions.len()),
                Err(e) => { eprintln!("{e}"); std::process::exit(1); }
            },
            Err(e) => { eprintln!("{e}"); std::process::exit(2); }
        },
        Some("emit-llvm") => match resolve_source(args.next()) {
            Ok(path) => match build_llvm(&path) {
                Ok(out) => println!("LLVM IR: {}", out.display()),
                Err(e) => { eprintln!("{e}"); std::process::exit(1); }
            },
            Err(e) => { eprintln!("{e}"); std::process::exit(2); }
        },
        Some("build") => match resolve_source(args.next()) {
            Ok(path) => match build_native(&path) {
                Ok(out) => println!("Built: {}", out.display()),
                Err(e) => { eprintln!("{e}"); std::process::exit(1); }
            },
            Err(e) => { eprintln!("{e}"); std::process::exit(2); }
        },
        Some("run") => match resolve_source(args.next()) {
            Ok(path) => match build_native(&path) {
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
            Err(e) => { eprintln!("{e}"); std::process::exit(2); }
        },
        Some("test") => {
            if let Err(e) = run_project_tests() {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        Some(command) => {
            eprintln!("rcl: unknown command {command}");
            eprintln!();
            print_help();
            std::process::exit(2);
        }
    }
}
