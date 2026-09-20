use std::env;
use std::fs;

use rcl::borrowck::BorrowChecker;
use rcl::lexer::Lexer;
use rcl::ownership::OwnershipChecker;
use rcl::hir::HirLowerer;
use rcl::mir::MirLowerer;
use rcl::mir_borrow::MirBorrowAnalyzer;
use rcl::mir_move::MirMoveAnalyzer;
use rcl::mir_opt::MirOptimizer;
use rcl::mir_validate::MirValidator;
use rcl::parser::Parser;
use rcl::sema::SemanticAnalyzer;

fn main() {
    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        Some("check") => {
            let path = match args.next() {
                Some(path) => path,
                None => {
                    eprintln!("usage: rcl check <file.rcl>");
                    std::process::exit(2);
                }
            };

            let source = match fs::read_to_string(&path) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("rcl: cannot read {path}: {error}");
                    std::process::exit(1);
                }
            };

            let tokens = match Lexer::new(&source).tokenize() {
                Ok(tokens) => tokens,
                Err(errors) => {
                    for error in errors {
                        eprintln!("error: {}:{}: {}", error.span.line, error.span.column, error.message);
                    }
                    std::process::exit(1);
                }
            };

            let program = match Parser::new(tokens).parse() {
                Ok(program) => program,
                Err(errors) => {
                    for error in errors {
                        eprintln!("error: {}:{}: {}", error.span.line, error.span.column, error.message);
                    }
                    std::process::exit(1);
                }
            };

            match SemanticAnalyzer::check(&program) {
                Ok(()) => match BorrowChecker::check(&program) {
                    Ok(()) => match OwnershipChecker::check(&program) {
                        Ok(()) => {
                            let hir = HirLowerer::lower(&program);
                            let mut mir = MirLowerer::lower(&hir);
                            MirOptimizer::optimize(&mut mir);

                            match MirValidator::validate(&mir) {
                                Ok(()) => match MirMoveAnalyzer::analyze(&mir) {
                                    Ok(()) => match MirBorrowAnalyzer::analyze(&mir) {
                                        Ok(()) => println!(
                                            "OK: semantic, borrow, ownership and MIR checks passed ({} top-level item(s))",
                                            program.items.len()
                                        ),
                                        Err(errors) => {
                                            for error in errors {
                                                eprintln!("error: MIR borrow: {}", error.message);
                                            }
                                            std::process::exit(1);
                                        }
                                    },
                                    Err(errors) => {
                                        for error in errors {
                                            eprintln!("error: MIR move: {}", error.message);
                                        }
                                        std::process::exit(1);
                                    }
                                },
                                Err(errors) => {
                                    for error in errors {
                                        eprintln!("error: MIR validation: {}", error.message);
                                    }
                                    std::process::exit(1);
                                }
                            }
                        },
                        Err(errors) => {
                            for error in errors {
                                eprintln!("error: {}:{}: {}", error.span.line, error.span.column, error.message);
                            }
                            std::process::exit(1);
                        }
                    },
                    Err(errors) => {
                        for error in errors {
                            eprintln!("error: {}:{}: {}", error.span.line, error.span.column, error.message);
                        }
                        std::process::exit(1);
                    }
                },
                Err(errors) => {
                    for error in errors {
                        eprintln!("error: {}:{}: {}", error.span.line, error.span.column, error.message);
                    }
                    std::process::exit(1);
                }
            }
        }
        _ => {
            println!("Recontrol Lang compiler 0.1.0");
            println!("usage: rcl check <file.rcl>");
        }
    }
}
