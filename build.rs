use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cargo_metadata::MetadataCommand;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let ebpf_out_dir = out_dir.join("ebpf");
    let ebpf_src = Path::new("ebpf/fork_monitor.bpf.c");
    let vmlinux_h = ebpf_out_dir.join("vmlinux.h");

    let meta = MetadataCommand::new().no_deps().exec().unwrap();
    let profile = env::var("PROFILE").unwrap();
    let bin_dir = meta.target_directory.join(profile).into_std_path_buf();

    // Ensure output directory exists
    fs::create_dir_all(&ebpf_out_dir).expect("Failed to create ebpf output directory");

    // Check for clang
    check_tool(
        "clang",
        &[
            "Ubuntu: sudo apt-get install clang",
            "Fedora: sudo dnf install clang",
            "Arch: sudo pacman -S clang",
        ],
    );

    // Check for bpftool
    check_tool(
        "bpftool",
        &[
            "Ubuntu: sudo apt-get install linux-tools-common",
            "Fedora: sudo dnf install bpftool",
            "Arch: sudo pacman -S bpf",
        ],
    );

    // Generate vmlinux.h
    run_command(
        &format!(
            "bpftool btf dump file /sys/kernel/btf/vmlinux format c > {}",
            vmlinux_h.display()
        ),
        "Failed to generate vmlinux.h",
    );

    // Compile eBPF program
    let ebpf_out_obj = bin_dir.join("ebpf").join("fork_monitor.bpf.o");
    fs::create_dir_all(ebpf_out_obj.parent().unwrap())
        .expect("Failed to create target/ebpf directory");

    run_command(
        &format!(
            "clang -O2 -target bpf -g -c {} -o {} -I{} -Wall -Wextra",
            ebpf_src.display(),
            ebpf_out_obj.display(),
            ebpf_out_dir.display()
        ),
        "Failed to compile eBPF program",
    );

    // Ensure Cargo rebuilds if source file changes
    println!("cargo:rerun-if-changed={}", ebpf_src.display());
}

fn check_tool(tool: &str, install_hints: &[&str]) {
    if !Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        eprintln!("Error: {} is not installed.", tool);
        for hint in install_hints {
            eprintln!("  Hint: {}", hint);
        }
        panic!("{} is required.", tool);
    }
}

fn run_command(cmd: &str, error_msg: &str) {
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .expect(error_msg);

    if !output.status.success() {
        eprintln!("{}: {}", error_msg, String::from_utf8_lossy(&output.stderr));
        panic!("{}", error_msg);
    }
}
