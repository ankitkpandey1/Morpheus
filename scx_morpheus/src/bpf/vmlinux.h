/* SPDX-License-Identifier: GPL-2.0 */
/*
 * vmlinux.h - Minimal kernel type definitions for Morpheus BPF scheduler
 *
 * This file provides the necessary kernel types for sched_ext BPF programs.
 * Generated from kernel BTF and manually filtered to avoid conflicts with
 * bpf_helpers.h macros.
 */

#ifndef __VMLINUX_H__
#define __VMLINUX_H__

/* Basic types */
typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
typedef unsigned long long u64;
typedef signed char s8;
typedef signed short s16;
typedef signed int s32;
typedef signed long long s64;

typedef u64 __u64;
typedef u32 __u32;
typedef u16 __u16;
typedef u8 __u8;
typedef s64 __s64;
typedef s32 __s32;
typedef s16 __s16;
typedef s8 __s8;

typedef int pid_t;
typedef long int intptr_t;
typedef long unsigned int uintptr_t;

#ifndef NULL
#define NULL ((void *)0)
#endif

#ifndef bool
typedef _Bool bool;
#define true 1
#define false 0
#endif

/* Forward declarations */
struct task_struct;
struct cgroup;
struct rq;
struct cpumask;

/* Task structure - minimal definition needed for sched_ext */
struct task_struct {
    volatile long state;
    pid_t pid;
    pid_t tgid;
    /* Additional fields exist but not needed for this scheduler */
};

/* sched_ext constants */
enum {
    SCX_DSQ_LOCAL = 0x8000000000000002ULL,
    SCX_DSQ_LOCAL_ON = 0xC000000000000000ULL,
    SCX_DSQ_LOCAL_CPU_MASK = 0xFFFFFFFFULL,
};

enum scx_kick_flags {
    SCX_KICK_IDLE = 1,
    SCX_KICK_PREEMPT = 2,
    SCX_KICK_WAIT = 4,
};

/* sched_ext exit kinds */
enum scx_exit_kind {
    SCX_EXIT_NONE = 0,
    SCX_EXIT_DONE = 1,
    SCX_EXIT_UNREG = 64,
    SCX_EXIT_UNREG_BPF = 65,
    SCX_EXIT_UNREG_KERN = 66,
    SCX_EXIT_SYSRQ = 67,
    SCX_EXIT_ERROR = 1024,
    SCX_EXIT_ERROR_BPF = 1025,
    SCX_EXIT_ERROR_STALL = 1026,
};

/* sched_ext structures */
struct scx_exit_info {
    enum scx_exit_kind kind;
    s64 exit_code;
    const char *reason;
    unsigned long *bt;
    u32 bt_len;
    char *msg;
    char *dump;
};

struct scx_init_task_args {
    bool fork;
    struct cgroup *cgroup;
};

/* BPF map types - needed before bpf_helpers.h is included */
#define BPF_MAP_TYPE_HASH 1
#define BPF_MAP_TYPE_ARRAY 2
#define BPF_MAP_TYPE_PERCPU_ARRAY 6
#define BPF_MAP_TYPE_TASK_STORAGE 21
#define BPF_MAP_TYPE_RINGBUF 27

/* BPF map flags */
#define BPF_F_NO_PREALLOC (1U << 0)
#define BPF_F_MMAPABLE (1U << 10)
#define BPF_LOCAL_STORAGE_GET_F_CREATE (1U << 0)

/* sched_ext ops structure - for SCX_OPS_DEFINE */
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
    u64 flags;
    u32 timeout_ms;
};

/* SCX_OPS_DEFINE macro */
#define SCX_OPS_DEFINE(name, ...) \
    SEC(".struct_ops.link") \
    struct sched_ext_ops name = { __VA_ARGS__ }

/* UEI (User Exit Info) macros */
struct user_exit_info {
    int kind;
    s64 exit_code;
    char reason[128];
    char msg[1024];
};

#define UEI_DEFINE(name) \
    struct user_exit_info name SEC(".data")

#define UEI_RECORD(uei, ei) do { \
    (uei).kind = (ei)->kind; \
    (uei).exit_code = (ei)->exit_code; \
} while(0)

/* sched_ext BPF kfuncs - declared as extern __ksym */
extern s32 scx_bpf_create_dsq(u64 dsq_id, s32 node) __ksym __weak;
extern void scx_bpf_dispatch(struct task_struct *p, u64 dsq_id, u64 slice, u64 enq_flags) __ksym __weak;
extern bool scx_bpf_consume(u64 dsq_id) __ksym __weak;
extern s32 scx_bpf_select_cpu_dfl(struct task_struct *p, s32 prev_cpu, u64 wake_flags, bool *is_idle) __ksym __weak;
extern void scx_bpf_kick_cpu(s32 cpu, u64 flags) __ksym __weak;
extern s32 scx_bpf_task_cpu(const struct task_struct *p) __ksym __weak;

/* BPF helper function declarations - these come from bpf_helpers.h */
/* Do NOT redefine them here */

#endif /* __VMLINUX_H__ */
