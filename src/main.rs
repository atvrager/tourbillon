use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "tbn", about = "Tourbillon HDL compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Type-check and deadlock-analyse without codegen
    Check {
        /// Input .tbn file(s)
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Compile to SystemVerilog
    Build {
        /// Input .tbn file(s)
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Output directory
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { files } => {
            for path in &files {
                match std::fs::read_to_string(path) {
                    Ok(src) => match tbn::check(&src, path.to_string_lossy().as_ref()) {
                        Ok(()) => eprintln!("{}: ok", path.display()),
                        Err(_) => std::process::exit(1),
                    },
                    Err(e) => {
                        eprintln!("error: {}: {e}", path.display());
                        std::process::exit(1);
                    }
                }
            }
        }
        Command::Build { files, output } => {
            for path in &files {
                match std::fs::read_to_string(path) {
                    Ok(src) => match tbn::build(&src, path.to_string_lossy().as_ref()) {
                        Ok(sv_files) => {
                            for sv_file in &sv_files {
                                let out_path = output.join(&sv_file.name);
                                if let Err(e) = std::fs::write(&out_path, &sv_file.content) {
                                    eprintln!("error writing {}: {e}", out_path.display());
                                    std::process::exit(1);
                                }
                                eprintln!("{}: wrote {}", path.display(), out_path.display());
                            }
                        }
                        Err(_) => std::process::exit(1),
                    },
                    Err(e) => {
                        eprintln!("error: {}: {e}", path.display());
                        std::process::exit(1);
                    }
                }
            }
        }
    }
}
