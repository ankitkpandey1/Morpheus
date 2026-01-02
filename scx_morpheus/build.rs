// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2024 Ankit Kumar Pandey <ankitkpandey1@gmail.com>

//! Build script for scx_morpheus BPF scheduler
//!
//! This script:
//! 1. Generates vmlinux.h from kernel BTF if available
//! 2. Compiles the BPF scheduler using libbpf-cargo
//! 3. Generates Rust bindings for the BPF skeleton

use libbpf_cargo::SkeletonBuilder;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BPF_SRC: &str = "src/bpf/scx_morpheus.bpf.c";

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let skel_path = out_dir.join("scx_morpheus.skel.rs");
    let vmlinux_path = out_dir.join("vmlinux.h");

    // Tell cargo to rerun if sources change
    println!("cargo:rerun-if-changed={}", BPF_SRC);
    println!("cargo:rerun-if-changed=src/bpf/compat.bpf.h");
    println!("cargo:rerun-if-changed=../morpheus-common/include/morpheus_shared.h");

    // Generate vmlinux.h from kernel BTF
    generate_vmlinux_h(&vmlinux_path);

    // Build the BPF skeleton with proper include paths
    SkeletonBuilder::new()
        .source(BPF_SRC)
        .clang_args([
            &format!("-I{}", out_dir.display()),  // For generated vmlinux.h
            "-Isrc/bpf",                           // For compat.bpf.h
            "-I../morpheus-common/include",        // For morpheus_shared.h
            "-Wno-compare-distinct-pointer-types",
            "-D__TARGET_ARCH_x86",
            "-g",
        ])
        .build_and_generate(&skel_path)
        .expect("Failed to build BPF skeleton");
}

/// Generate vmlinux.h from kernel BTF
///
/// This uses bpftool to dump the kernel's BTF information into a header
/// file that contains all kernel type definitions needed for BPF programs.
fn generate_vmlinux_h(output_path: &PathBuf) {
    // Check if /sys/kernel/btf/vmlinux exists
    let btf_path = "/sys/kernel/btf/vmlinux";
    if !std::path::Path::new(btf_path).exists() {
        eprintln!("Error: {} not found. Kernel BTF is required.", btf_path);
        eprintln!("Ensure CONFIG_DEBUG_INFO_BTF=y in your kernel config.");
        panic!("Kernel BTF not found and fallbacks are disabled.");
    }

    // Try to generate vmlinux.h using bpftool
    let result = Command::new("bpftool")
        .args(["btf", "dump", "file", btf_path, "format", "c"])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            // Write the generated header
            fs::write(output_path, &output.stdout)
                .expect("Failed to write vmlinux.h");
            println!("cargo:warning=Generated vmlinux.h from kernel BTF");
        }
        Ok(output) => {
            panic!("bpftool failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Err(e) => {
            panic!("Failed to run bpftool: {}. Install linux-tools-generic.", e);
        }
    }
}
