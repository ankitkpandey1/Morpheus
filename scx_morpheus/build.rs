use libbpf_cargo::SkeletonBuilder;
use std::env;
use std::path::PathBuf;

const BPF_SRC: &str = "src/bpf/scx_morpheus.bpf.c";

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let skel_path = out_dir.join("scx_morpheus.skel.rs");

    // Tell cargo to rerun if the BPF source changes
    println!("cargo:rerun-if-changed={}", BPF_SRC);
    println!("cargo:rerun-if-changed=../morpheus-common/include/morpheus_shared.h");

    // Build the BPF skeleton
    SkeletonBuilder::new()
        .source(BPF_SRC)
        .clang_args([
            "-I../morpheus-common/include",
            "-Wno-compare-distinct-pointer-types",
        ])
        .build_and_generate(&skel_path)
        .expect("Failed to build BPF skeleton");
}
