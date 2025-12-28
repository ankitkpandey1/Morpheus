/* SPDX-License-Identifier: GPL-2.0 */
/*
 * vmlinux.h - Kernel type definitions for BPF programs
 *
 * This is a minimal vmlinux.h stub for compilation without full BTF.
 * In production, generate this from your kernel's BTF data:
 *   bpftool btf dump file /sys/kernel/btf/vmlinux format c > vmlinux.h
 */

#ifndef __VMLINUX_H__
#define __VMLINUX_H__

#ifndef __KERNEL__
typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
typedef unsigned long long u64;
typedef signed char s8;
typedef signed short s16;
typedef signed int s32;
typedef signed long long s64;
#endif

typedef u64 __u64;
typedef u32 __u32;
typedef u16 __u16;
typedef u8 __u8;
typedef s64 __s64;
typedef s32 __s32;
typedef s16 __s16;
typedef s8 __s8;

typedef int pid_t;

#ifndef NULL
#define NULL ((void *)0)
#endif

#ifndef bool
typedef _Bool bool;
#define true 1
#define false 0
#endif

/* Task structure - minimal definition */
struct task_struct {
    volatile long state;
    pid_t pid;
    pid_t tgid;
    /* ... many more fields in real kernel ... */
};

/* sched_ext definitions */
#define SCX_DSQ_LOCAL          ((u64)-1)
#define SCX_KICK_PREEMPT       (1 << 0)

struct scx_init_task_args {
    /* Task initialization args */
};

struct scx_exit_info {
    int exit_code;
    const char *exit_msg;
};

/* BPF helpers - these are provided by libbpf */
#define SEC(NAME) __attribute__((section(NAME), used))

/* sched_ext macros - stubs for compilation */
#define SCX_OPS_DEFINE(name, ...) \
    struct sched_ext_ops name = { __VA_ARGS__ }

#define UEI_DEFINE(name) \
    static struct { int dummy; } name

#define UEI_RECORD(name, ei) \
    do { (void)(name); (void)(ei); } while(0)

/* sched_ext BPF helpers - stubs */
static inline s32 scx_bpf_create_dsq(u64 dsq_id, s32 node) { return 0; }
static inline void scx_bpf_dispatch(struct task_struct *p, u64 dsq_id, u64 slice, u64 flags) {}
static inline bool scx_bpf_consume(u64 dsq_id) { return false; }
static inline s32 scx_bpf_select_cpu_dfl(struct task_struct *p, s32 prev_cpu, u64 wake_flags, bool *is_idle) { return 0; }
static inline void scx_bpf_kick_cpu(s32 cpu, u32 flags) {}
static inline s32 scx_bpf_task_cpu(struct task_struct *p) { return 0; }

/* BPF struct ops */
#define BPF_STRUCT_OPS(name, ...) name(__VA_ARGS__)
#define BPF_STRUCT_OPS_SLEEPABLE(name, ...) name(__VA_ARGS__)

/* BPF map types */
#define BPF_MAP_TYPE_ARRAY 2
#define BPF_MAP_TYPE_HASH 1
#define BPF_MAP_TYPE_RINGBUF 27
#define BPF_MAP_TYPE_TASK_STORAGE 21
#define BPF_MAP_TYPE_PERCPU_ARRAY 6

/* BPF map flags */
#define BPF_F_NO_PREALLOC (1U << 0)
#define BPF_F_MMAPABLE (1U << 10)
#define BPF_LOCAL_STORAGE_GET_F_CREATE (1U << 0)

/* sched_ext ops structure */
struct sched_ext_ops {
    s32 (*select_cpu)(struct task_struct *p, s32 prev_cpu, u64 wake_flags);
    void (*enqueue)(struct task_struct *p, u64 enq_flags);
    void (*dispatch)(s32 cpu, struct task_struct *prev);
    void (*running)(struct task_struct *p);
    void (*stopping)(struct task_struct *p, bool runnable);
    void (*tick)(struct task_struct *p);
    s32 (*init_task)(struct task_struct *p, struct scx_init_task_args *args);
    void (*enable)(struct task_struct *p);
    s32 (*init)(void);
    void (*exit)(struct scx_exit_info *ei);
    const char *name;
};

#endif /* __VMLINUX_H__ */
