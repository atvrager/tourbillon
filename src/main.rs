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
    /// Show provenance hash and cache status
    Status {
        /// Input .tbn file(s)
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Emit process network as Graphviz DOT
    Graph {
        /// Input .tbn file(s)
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Remove the build cache (~/.tbn/store/)
    Clean,
    /// Read an FST waveform trace and dump signal values
    Wave {
        /// Input .fst file
        #[arg(required = true)]
        file: PathBuf,
        /// Signal path filter (substring match, e.g. "tohost" or "cpu_clk")
        #[arg(short, long)]
        filter: Option<String>,
        /// Only show values at or after this timestamp
        #[arg(long)]
        from: Option<u64>,
        /// Only show values at or before this timestamp
        #[arg(long)]
        to: Option<u64>,
        /// List all signal names without reading values
        #[arg(short, long)]
        list: bool,
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
            // Read all input files upfront
            let sources: Vec<(String, String)> = files
                .iter()
                .map(|path| {
                    let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                        eprintln!("error: {}: {e}", path.display());
                        std::process::exit(1);
                    });
                    (path.to_string_lossy().to_string(), src)
                })
                .collect();

            // Compute source root hash
            let file_refs: Vec<(&str, &[u8])> = sources
                .iter()
                .map(|(name, content)| (name.as_str(), content.as_bytes()))
                .collect();
            let root_hash = tbn::provenance::source_root(&file_refs);

            // Write manifest and SV files to cache dir (best-effort)
            let cache = tbn::provenance::cache_dir(&root_hash);
            std::fs::create_dir_all(&cache).ok();
            let manifest = tbn::provenance::source_manifest(&file_refs);
            std::fs::write(
                cache.join("source_manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .ok();

            // Compile each file
            for (name, src) in &sources {
                match tbn::build(src, name, Some(root_hash)) {
                    Ok(sv_files) => {
                        for sv_file in &sv_files {
                            let out_path = output.join(&sv_file.name);
                            if let Err(e) = std::fs::write(&out_path, &sv_file.content) {
                                eprintln!("error writing {}: {e}", out_path.display());
                                std::process::exit(1);
                            }
                            eprintln!("{name}: wrote {}", out_path.display());
                            // Also cache the SV file (best-effort)
                            std::fs::write(cache.join(&sv_file.name), &sv_file.content).ok();
                        }
                    }
                    Err(_) => std::process::exit(1),
                }
            }

            eprintln!("provenance: {}", tbn::provenance::hex(&root_hash));
        }
        Command::Status { files } => {
            let sources: Vec<(String, String)> = files
                .iter()
                .map(|path| {
                    let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                        eprintln!("error: {}: {e}", path.display());
                        std::process::exit(1);
                    });
                    (path.to_string_lossy().to_string(), src)
                })
                .collect();

            let file_refs: Vec<(&str, &[u8])> = sources
                .iter()
                .map(|(name, content)| (name.as_str(), content.as_bytes()))
                .collect();
            let root_hash = tbn::provenance::source_root(&file_refs);
            let hex = tbn::provenance::hex(&root_hash);
            let cache = tbn::provenance::cache_dir(&root_hash);

            println!("source_root: {hex}");
            if cache.exists() {
                println!("cache: {} (exists)", cache.display());
            } else {
                println!("cache: {} (not found)", cache.display());
            }
        }
        Command::Graph { files } => {
            for path in &files {
                match std::fs::read_to_string(path) {
                    Ok(src) => match tbn::emit_graph(&src, path.to_string_lossy().as_ref()) {
                        Ok(dots) => {
                            for dot in dots {
                                print!("{dot}");
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
        Command::Wave {
            file,
            filter,
            from,
            to,
            list,
        } => {
            if let Err(e) = tbn::wave::run(&file, filter.as_deref(), from, to, list) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Command::Clean => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let store = PathBuf::from(home).join(".tbn").join("store");
            if store.exists() {
                match std::fs::remove_dir_all(&store) {
                    Ok(()) => eprintln!("removed {}", store.display()),
                    Err(e) => {
                        eprintln!("error removing {}: {e}", store.display());
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("nothing to clean");
            }
        }
    }
}
