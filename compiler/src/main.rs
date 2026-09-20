use std::env;
use std::fs;
use rcl::lexer::Lexer;

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

            match Lexer::new(&source).tokenize() {
                Ok(tokens) => println!("OK: {} tokens", tokens.len()),
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
