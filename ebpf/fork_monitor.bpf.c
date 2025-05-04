#include "vmlinux.h"
#include <bpf/bpf_core_read.h>
#include <bpf/bpf_helpers.h>

struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 1 << 24); // 16MB ring buffer
} events SEC(".maps");

char _license[] SEC("license") = "GPL";

#define TYPE_FORK 0
#define TYPE_EXEC 1
#define TYPE_EXIT 2
#define TYPE_CLONE 3
#define TYPE_UNSHARE 4
#define TYPE_SETNS 5

struct event {
    u32 type;
    u32 pid;
    u32 tid;
    u32 uid;
    u32 gid;
    u32 parent_pid;
    char command[TASK_COMM_LEN];
};

void process_generic_event(u32 type) {
    u64 pid_tgid = bpf_get_current_pid_tgid();
    u64 uid_gid = bpf_get_current_uid_gid();
    struct task_struct *task = (struct task_struct *)bpf_get_current_task();

    struct event event = (struct event){
        .type = type,

        .pid = pid_tgid >> 32,
        .tid = pid_tgid & 0xFFFFFFFF,
        .uid = uid_gid & 0xFFFFFFFF,
        .gid = uid_gid >> 32,

        .parent_pid = BPF_CORE_READ(task, real_parent, tgid),
        .command = {0},
    };

    bpf_get_current_comm(&event.command, sizeof(event.command));
    bpf_ringbuf_output(&events, &event, sizeof(event), 0);
}


SEC("tracepoint/sched/sched_process_fork")
int trace_sched_process_fork(struct trace_event_raw_sched_process_fork *ctx) {
    u64 pid_tgid = bpf_get_current_pid_tgid();
    u64 uid_gid  = bpf_get_current_uid_gid();

    struct event event = {
        .type        = TYPE_FORK,
        .pid         = pid_tgid >> 32,
        .tid         = pid_tgid & 0xFFFFFFFF,
        .uid         = uid_gid & 0xFFFFFFFF,
        .gid         = uid_gid >> 32,
        .parent_pid  = ctx->parent_pid,
        .command     = {0},
    };

    bpf_get_current_comm(&event.command, sizeof(event.command));

    bpf_ringbuf_output(&events, &event, sizeof(event), 0);
    return 0;
}

SEC("tracepoint/syscalls/sys_enter_execve")
int trace_exec(struct trace_event_raw_sys_enter *ctx) {
    process_generic_event(TYPE_EXEC);
    return 0;
}

SEC("tracepoint/sched/sched_process_exit")
int trace_exit(struct trace_event_raw_sched_process_exit *ctx) {
    process_generic_event(TYPE_EXIT);
    return 0;
}

SEC("tracepoint/syscalls/sys_enter_clone")
int trace_clone(struct trace_event_raw_sys_enter *ctx) {
    process_generic_event(TYPE_CLONE);
    return 0;
}

SEC("tracepoint/syscalls/sys_enter_unshare")
int trace_unshare(struct trace_event_raw_sys_enter *ctx) {
    process_generic_event(TYPE_UNSHARE);
    return 0;
}

SEC("tracepoint/syscalls/sys_enter_setns")
int trace_setns(struct trace_event_raw_sys_enter *ctx) {
    process_generic_event(TYPE_SETNS);
    return 0;
}
