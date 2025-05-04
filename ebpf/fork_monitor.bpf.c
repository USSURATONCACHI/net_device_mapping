#include "vmlinux.h"
#include <bpf/bpf_core_read.h>
#include <bpf/bpf_helpers.h>

struct fork_event {
    u32 parent_pid;
    u32 child_pid;
};

struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 1 << 24); // 16MB ring buffer
} events SEC(".maps");

SEC("tracepoint/sched/sched_process_fork")
int trace_sched_process_fork(struct trace_event_raw_sched_process_fork *ctx) {
    struct fork_event event = {};
    event.parent_pid = ctx->parent_pid;
    event.child_pid = ctx->child_pid;
    bpf_ringbuf_output(&events, &event, sizeof(event), 0);
    return 0;
}

char _license[] SEC("license") = "GPL";
