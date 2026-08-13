pub mod ast;
pub mod cli;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod typecheck;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let exit_code = match args.get(1).map(|s| s.as_str()) {
        Some("run") => match args.get(2) {
            Some(path) => cli::run_file(path),
            None => {
                eprintln!("usage: ember run <path>.em");
                2
            }
        },
        Some(other) => {
            eprintln!(
                "unknown command '{}'\nusage:\n  ember run <path>.em   (run a file)\n  ember                 (start the REPL)",
                other
            );
            2
        }
        None => {
            cli::repl();
            0
        }
    };
    std::process::exit(exit_code);
}
