//! Integration test: compile rv32i.tbn, build Verilator sim, run rv32ui compliance tests.
//!
//! Requires: riscv64-elf-gcc, verilator, make
//! Also requires riscv-tests submodule: git submodule update --init

use std::path::Path;
use std::process::Command;

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn check_tool(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn rv32ui_compliance() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sim_dir = repo_root.join("sim");

    // Check prerequisites
    if !check_tool("verilator") {
        eprintln!("SKIP: verilator not found");
        return;
    }
    if !check_tool("riscv64-elf-gcc") {
        eprintln!("SKIP: riscv64-elf-gcc not found");
        return;
    }

    // Check riscv-tests submodule
    let riscv_tests = repo_root.join("riscv-tests/isa/rv32ui/add.S");
    if !riscv_tests.exists() {
        eprintln!("SKIP: riscv-tests submodule not initialized (run: git submodule update --init)");
        return;
    }

    // Build tbn compiler first
    let status = Command::new("cargo")
        .args(["build"])
        .current_dir(repo_root)
        .status()
        .expect("failed to run cargo build");
    assert!(status.success(), "cargo build failed");

    // Build rv32ui tests with custom env (no CSR support)
    let build_tests = sim_dir.join("build_tests.sh");
    let status = Command::new("bash")
        .arg(&build_tests)
        .current_dir(&sim_dir)
        .status()
        .expect("failed to run build_tests.sh");
    assert!(status.success(), "build_tests.sh failed");

    // Clean first, then build with parallelism
    let status = Command::new("make")
        .arg("clean")
        .current_dir(&sim_dir)
        .status()
        .expect("failed to run make clean");
    assert!(status.success(), "make clean failed");

    let status = Command::new("make")
        .args(["build", &format!("-j{}", num_cpus())])
        .current_dir(&sim_dir)
        .status()
        .expect("failed to run make build");
    assert!(status.success(), "make build failed");

    // Run riscv-tests
    let output = Command::new("make")
        .arg("riscv-tests")
        .current_dir(&sim_dir)
        .output()
        .expect("failed to run make riscv-tests");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{stdout}");
    eprint!("{stderr}");

    assert!(
        output.status.success(),
        "riscv-tests failed:\n{stdout}\n{stderr}"
    );

    // Verify we actually ran tests (not 0 passed, 0 failed)
    assert!(
        stdout.contains("passed") && !stdout.contains("0 passed, 0 failed"),
        "No tests were actually run"
    );
}
