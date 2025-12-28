/* SPDX-License-Identifier: GPL-2.0 */
/*
 * compat.bpf.h - sched_ext compatibility header for Morpheus-Hybrid
 *
 * This header provides the macros and definitions needed for sched_ext
 * BPF schedulers. It detects whether vmlinux.h from kernel BTF is available
 * and avoids redeclaring types/functions already present.
 *
 * Copyright (C) 2024 Ankit Kumar Pandey <ankitkpandey1@gmail.com>
 */

#ifndef __SCX_COMPAT_BPF_H
#define __SCX_COMPAT_BPF_H

#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_core_read.h>

/* ============================================================================
 * Basic type definitions (if not from vmlinux.h)
 * ============================================================================ */

#ifndef __kptr
#define __kptr __attribute__((btf_type_tag("kptr")))
#endif

#ifndef __percpu_kptr
#define __percpu_kptr __attribute__((btf_type_tag("percpu_kptr")))
#endif

/* ============================================================================
 * sched_ext enums and constants
 *
 * These may already be defined in vmlinux.h, so we guard them.
 * ============================================================================ */

#ifndef SCX_DSQ_LOCAL
/* Dispatch queue IDs */
#define SCX_DSQ_LOCAL		((u64)-1)
#define SCX_DSQ_GLOBAL		((u64)-2)
#define SCX_DSQ_LOCAL_ON	((u64)-3)
#define SCX_DSQ_LOCAL_CPU_MASK	0xffffffffULL
#endif

#ifndef SCX_SLICE_DFL
/* Default time slice (20ms) */
#define SCX_SLICE_DFL		(20 * 1000 * 1000)
#define SCX_SLICE_INF		(~0ULL)
#endif

#ifndef SCX_KICK_IDLE
/* scx_bpf_kick_cpu flags */
#define SCX_KICK_IDLE		(1ULL << 0)
#define SCX_KICK_PREEMPT	(1ULL << 1)
#define SCX_KICK_WAIT		(1ULL << 2)
#endif

#ifndef SCX_ENQ_WAKEUP
/* scx_bpf_dsq_insert / scx_bpf_dispatch flags */
#define SCX_ENQ_WAKEUP		(1ULL << 0)
#define SCX_ENQ_HEAD		(1ULL << 1)
#define SCX_ENQ_PREEMPT		(1ULL << 2)
#define SCX_ENQ_REENQ		(1ULL << 3)
#define SCX_ENQ_LAST		(1ULL << 4)
#define SCX_ENQ_CLEAR_OPSS	(1ULL << 5)
#define SCX_ENQ_DSQ_PRIQ	(1ULL << 6)
#endif

/* ============================================================================
 * BPF_STRUCT_OPS macros
 *
 * These are used to define sched_ext ops functions.
 * ============================================================================ */

#ifndef BPF_STRUCT_OPS
/*
 * BPF_STRUCT_OPS - Define a sched_ext ops function
 *
 * This macro simplifies defining struct_ops BPF programs by handling
 * the SEC annotation and function declaration.
 */
#define BPF_STRUCT_OPS(name, args...)					\
SEC("struct_ops/"#name)							\
BPF_PROG(name, ##args)
#endif

#ifndef BPF_STRUCT_OPS_SLEEPABLE
#define BPF_STRUCT_OPS_SLEEPABLE(name, args...)				\
SEC("struct_ops.s/"#name)						\
BPF_PROG(name, ##args)
#endif

/* ============================================================================
 * SCX_OPS_DEFINE - Define the sched_ext_ops structure
 * ============================================================================ */

#ifndef SCX_OPS_DEFINE
#define SCX_OPS_DEFINE(name, ...)					\
SEC(".struct_ops.link")							\
struct sched_ext_ops name = {						\
	__VA_ARGS__							\
};
#endif

/* ============================================================================
 * User Exit Info (UEI) macros
 *
 * These handle graceful scheduler exit and error reporting.
 * ============================================================================ */

#ifndef RESIZABLE_ARRAY
#define RESIZABLE_ARRAY(elfsec, arr) arr[]
#endif

#ifndef UEI_DEFINE
/* UEI structure for exit handling */
struct user_exit_info {
	s32 kind;
	s64 exit_code;
	char reason[128];
	char msg[256];
};

/* Define the UEI structure */
#define UEI_DEFINE(name)						\
	char RESIZABLE_ARRAY(data, name##_dump);			\
	const volatile u32 name##_dump_len;				\
	struct user_exit_info name
#endif

#ifndef UEI_RECORD
/* Record exit info */
#define UEI_RECORD(uei, ei) do {					\
	bpf_probe_read_kernel_str((uei).reason, sizeof((uei).reason),	\
				  (const void *)(ei)->reason);		\
	(uei).exit_code = (ei)->exit_code;				\
	(uei).kind = (ei)->kind;					\
} while (0)
#endif

/* ============================================================================
 * Time comparison helpers
 * ============================================================================ */

#ifndef time_before
#define time_before(a, b)	((s64)((a) - (b)) < 0)
#define time_after(a, b)	time_before(b, a)
#endif

#endif /* __SCX_COMPAT_BPF_H */
