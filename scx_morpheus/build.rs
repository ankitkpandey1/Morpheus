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
        eprintln!("Warning: {} not found. Kernel BTF may not be enabled.", btf_path);
        eprintln!("Ensure CONFIG_DEBUG_INFO_BTF=y in your kernel config.");
        // Create a minimal fallback header
        create_fallback_vmlinux_h(output_path);
        return;
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
            eprintln!("bpftool failed: {}", String::from_utf8_lossy(&output.stderr));
            create_fallback_vmlinux_h(output_path);
        }
        Err(e) => {
            eprintln!("Failed to run bpftool: {}", e);
            eprintln!("Install bpftool: sudo apt install linux-tools-generic");
            create_fallback_vmlinux_h(output_path);
        }
    }
}

/// Create a minimal vmlinux.h fallback for systems without BTF
///
/// This provides essential type definitions needed for the BPF scheduler
/// but may not work on all kernel versions.
fn create_fallback_vmlinux_h(output_path: &PathBuf) {
    println!("cargo:warning=Using fallback vmlinux.h - full kernel BTF recommended");
    
    let content = r#"/* SPDX-License-Identifier: GPL-2.0 */
/*
 * Fallback vmlinux.h for systems without kernel BTF
 *
 * WARNING: This is a minimal fallback. For full compatibility,
 * enable CONFIG_DEBUG_INFO_BTF=y in your kernel and install bpftool.
 */

#ifndef __VMLINUX_H__
#define __VMLINUX_H__

#ifndef BPF_NO_PRESERVE_ACCESS_INDEX
#pragma clang attribute push (__attribute__((preserve_access_index)), apply_to = record)
#endif

#ifndef __ksym
#define __ksym __attribute__((section(".ksyms")))
#endif

#ifndef __weak
#define __weak __attribute__((weak))
#endif

/* Basic types */
typedef unsigned char __u8;
typedef unsigned short __u16;
typedef unsigned int __u32;
typedef unsigned long long __u64;
typedef signed char __s8;
typedef signed short __s16;
typedef signed int __s32;
typedef signed long long __s64;

typedef __u8 u8;
typedef __u16 u16;
typedef __u32 u32;
typedef __u64 u64;
typedef __s8 s8;
typedef __s16 s16;
typedef __s32 s32;
typedef __s64 s64;

typedef _Bool bool;
#define true 1
#define false 0

typedef int pid_t;

/* BPF map types */
enum bpf_map_type {
    BPF_MAP_TYPE_UNSPEC,
    BPF_MAP_TYPE_HASH,
    BPF_MAP_TYPE_ARRAY,
    BPF_MAP_TYPE_PROG_ARRAY,
    BPF_MAP_TYPE_PERF_EVENT_ARRAY,
    BPF_MAP_TYPE_PERCPU_HASH,
    BPF_MAP_TYPE_PERCPU_ARRAY,
    BPF_MAP_TYPE_STACK_TRACE,
    BPF_MAP_TYPE_CGROUP_ARRAY,
    BPF_MAP_TYPE_LRU_HASH,
    BPF_MAP_TYPE_LRU_PERCPU_HASH,
    BPF_MAP_TYPE_LPM_TRIE,
    BPF_MAP_TYPE_ARRAY_OF_MAPS,
    BPF_MAP_TYPE_HASH_OF_MAPS,
    BPF_MAP_TYPE_DEVMAP,
    BPF_MAP_TYPE_SOCKMAP,
    BPF_MAP_TYPE_CPUMAP,
    BPF_MAP_TYPE_XSKMAP,
    BPF_MAP_TYPE_SOCKHASH,
    BPF_MAP_TYPE_CGROUP_STORAGE_DEPRECATED,
    BPF_MAP_TYPE_CGROUP_STORAGE = BPF_MAP_TYPE_CGROUP_STORAGE_DEPRECATED,
    BPF_MAP_TYPE_REUSEPORT_SOCKARRAY,
    BPF_MAP_TYPE_PERCPU_CGROUP_STORAGE,
    BPF_MAP_TYPE_QUEUE,
    BPF_MAP_TYPE_STACK,
    BPF_MAP_TYPE_SK_STORAGE,
    BPF_MAP_TYPE_DEVMAP_HASH,
    BPF_MAP_TYPE_STRUCT_OPS,
    BPF_MAP_TYPE_RINGBUF,
    BPF_MAP_TYPE_INODE_STORAGE,
    BPF_MAP_TYPE_TASK_STORAGE,
    BPF_MAP_TYPE_BLOOM_FILTER,
    BPF_MAP_TYPE_USER_RINGBUF,
    BPF_MAP_TYPE_CGRP_STORAGE,
};

/* BPF map flags */
#define BPF_F_NO_PREALLOC (1U << 0)
#define BPF_F_MMAPABLE    (1U << 10)

/* Sched ext constants */
enum scx_kick_flags {
    SCX_KICK_NO_PREEMPT = 0,
    SCX_KICK_PREEMPT    = 1,
    SCX_KICK_WAIT       = 2,
};

enum scx_dsq_id_flags {
    SCX_DSQ_FLAG_BUILTIN = 1 << 0,
    SCX_DSQ_FLAG_LOCAL   = 1 << 1,
    SCX_DSQ_FLAG_GLOBAL  = 1 << 2,
};

#define SCX_DSQ_LOCAL   0
#define SCX_DSQ_GLOBAL  1

/* kfuncs */
extern void scx_bpf_kick_cpu(s32 cpu, u64 flags) __ksym;
extern s32 scx_bpf_select_cpu_dfl(struct task_struct *p, s32 prev_cpu, u64 wake_flags, bool *is_idle) __ksym;
extern void scx_bpf_dispatch(struct task_struct *p, u64 dsq_id, u64 slice, u64 enq_flags) __ksym;
extern void scx_bpf_consume(u64 dsq_id) __ksym;
extern s32 scx_bpf_create_dsq(u64 dsq_id, s32 cpu) __ksym;
extern s32 scx_bpf_dsq_move_set_slice(struct task_struct *p, u64 dsq_id, u64 slice, u64 enq_flags) __ksym;
extern s32 scx_bpf_dsq_move_set_vtime(struct task_struct *p, u64 dsq_id, u64 vtime, u64 enq_flags) __ksym;
extern u32 scx_bpf_task_cpu(const struct task_struct *p) __ksym;

/* cpumask structure */
struct cpumask {
    unsigned long bits[128 / sizeof(unsigned long)];
};

/* Minimal task_struct for sched_ext */
struct task_struct {
    volatile long state;
    pid_t pid;
    pid_t tgid;
    const char *comm;
    
    /* sched_ext fields */
    struct {
        u64 dsq_vtime;
        u64 slice;
        u32 weight;
        u32 flags;
    } scx;
};

/* sched_ext init task args */
struct scx_init_task_args {
    bool fork;
    bool cgroup;
};

/* sched_ext exit info */
struct scx_exit_info {
    s32 kind;
    s64 exit_code;
    const char *reason;
    const char *msg;
};

/* sched_ext ops structure */
struct sched_ext_ops {
    s32 (*select_cpu)(struct task_struct *p, s32 prev_cpu, u64 wake_flags);
    void (*enqueue)(struct task_struct *p, u64 enq_flags);
    void (*dequeue)(struct task_struct *p, u64 deq_flags);
    void (*dispatch)(s32 cpu, struct task_struct *prev);
    void (*tick)(struct task_struct *p);
    void (*runnable)(struct task_struct *p, u64 enq_flags);
    void (*running)(struct task_struct *p);
    void (*stopping)(struct task_struct *p, bool runnable);
    void (*quiescent)(struct task_struct *p, u64 deq_flags);
    bool (*yield)(struct task_struct *from, struct task_struct *to);
    bool (*core_sched_before)(struct task_struct *a, struct task_struct *b);
    void (*set_weight)(struct task_struct *p, u32 weight);
    void (*set_cpumask)(struct task_struct *p, const struct cpumask *cpumask);
    void (*update_idle)(s32 cpu, bool idle);
    void (*cpu_acquire)(s32 cpu, struct scx_cpu_acquire_args *args);
    void (*cpu_release)(s32 cpu, struct scx_cpu_release_args *args);
    s32 (*init_task)(struct task_struct *p, struct scx_init_task_args *args);
    void (*exit_task)(struct task_struct *p, struct scx_exit_task_args *args);
    void (*enable)(struct task_struct *p);
    void (*disable)(struct task_struct *p);
    void (*dump)(struct scx_dump_ctx *ctx);
    void (*dump_cpu)(struct scx_dump_ctx *ctx, s32 cpu, bool idle);
    void (*dump_task)(struct scx_dump_ctx *ctx, struct task_struct *p);
    s32 (*cgroup_init)(struct cgroup *cgrp, struct scx_cgroup_init_args *args);
    void (*cgroup_exit)(struct cgroup *cgrp);
    s32 (*cgroup_prep_move)(struct task_struct *p, struct cgroup *from, struct cgroup *to);
    void (*cgroup_move)(struct task_struct *p, struct cgroup *from, struct cgroup *to);
    void (*cgroup_cancel_move)(struct task_struct *p, struct cgroup *from, struct cgroup *to);
    void (*cgroup_set_weight)(struct cgroup *cgrp, u32 weight);
    void (*cpu_online)(s32 cpu);
    void (*cpu_offline)(s32 cpu);
    s32 (*init)(void);
    void (*exit)(struct scx_exit_info *ei);
    u32 dispatch_max_batch;
    u64 flags;
    u32 timeout_ms;
    u32 exit_dump_len;
    u64 hotplug_seq;
    char name[128];
};

/* Forward declarations for unused types */
struct scx_cpu_acquire_args;
struct scx_cpu_release_args;
struct scx_exit_task_args;
struct scx_dump_ctx;
struct cgroup;
struct scx_cgroup_init_args;

#ifndef BPF_NO_PRESERVE_ACCESS_INDEX
#pragma clang attribute pop
#endif

#endif /* __VMLINUX_H__ */
"#;

    fs::write(output_path, content)
        .expect("Failed to write fallback vmlinux.h");
}
