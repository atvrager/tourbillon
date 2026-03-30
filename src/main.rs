use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(ValueEnum, Clone, Default)]
enum Target {
    #[default]
    Sv,
    Chisel,
}

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
    /// Compile to SystemVerilog or Chisel
    Build {
        /// Input .tbn file(s)
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Output directory
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// Target backend
        #[arg(long, value_enum, default_value = "sv")]
        target: Target,
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

/// Read and concatenate multiple .tbn source files into a single compilation unit.
/// Returns (combined_source, display_name).
fn read_and_concat(files: &[PathBuf]) -> (String, String) {
    let mut combined = String::new();
    for path in files {
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("error: {}: {e}", path.display());
            std::process::exit(1);
        });
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&src);
    }
    let display_name = if files.len() == 1 {
        files[0].to_string_lossy().to_string()
    } else {
        files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("+")
    };
    (combined, display_name)
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { files } => {
            let (combined_src, display_name) = read_and_concat(&files);
            match tbn::check(&combined_src, &display_name) {
                Ok(()) => eprintln!("{display_name}: ok"),
                Err(_) => std::process::exit(1),
            }
        }
        Command::Build {
            files,
            output,
            target,
        } => {
            // Read all input files and compute provenance from individual files
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

            // Cache manifest (best-effort)
            let cache = tbn::provenance::cache_dir(&root_hash);
            std::fs::create_dir_all(&cache).ok();
            let manifest = tbn::provenance::source_manifest(&file_refs);
            std::fs::write(
                cache.join("source_manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .ok();

            // Concatenate all sources into a single compilation unit.
            // Process/type/pipe definitions from earlier files are visible to later ones.
            let (combined_src, display_name) = read_and_concat(&files);

            match target {
                Target::Sv => {
                    match tbn::build(&combined_src, &display_name, Some(root_hash)) {
                        Ok(sv_files) => {
                            for sv_file in &sv_files {
                                let out_path = output.join(&sv_file.name);
                                if let Err(e) = std::fs::write(&out_path, &sv_file.content) {
                                    eprintln!("error writing {}: {e}", out_path.display());
                                    std::process::exit(1);
                                }
                                eprintln!("{display_name}: wrote {}", out_path.display());
                                std::fs::write(cache.join(&sv_file.name), &sv_file.content).ok();
                            }
                        }
                        Err(_) => std::process::exit(1),
                    }
                    eprintln!("provenance: {}", tbn::provenance::hex(&root_hash));
                }
                Target::Chisel => match tbn::build_chisel(&combined_src, &display_name) {
                    Ok(chisel_files) => {
                        for file in &chisel_files {
                            let out_path = output.join(&file.name);
                            if let Err(e) = std::fs::write(&out_path, &file.content) {
                                eprintln!("error writing {}: {e}", out_path.display());
                                std::process::exit(1);
                            }
                            eprintln!("{display_name}: wrote {}", out_path.display());
                        }
                    }
                    Err(_) => std::process::exit(1),
                },
            }
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
            let (combined_src, display_name) = read_and_concat(&files);
            match tbn::emit_graph(&combined_src, &display_name) {
                Ok(dots) => {
                    for dot in dots {
                        print!("{dot}");
                    }
                }
                Err(_) => std::process::exit(1),
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
