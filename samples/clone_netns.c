#define _GNU_SOURCE
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <sys/wait.h>

#define STACK_SIZE (1024 * 1024)

static int child_func(void *arg) {
    printf("Child process in new network namespace (PID: %d)\n", getpid());
    system("ip link");
    return 0;
}

int main() {
    char *stack;
    char *stack_top;
    pid_t pid;

    stack = malloc(STACK_SIZE);
    if (!stack) {
        perror("malloc");
        exit(EXIT_FAILURE);
    }
    stack_top = stack + STACK_SIZE;

    pid = clone(child_func, stack_top, CLONE_NEWNET | SIGCHLD, NULL);
    if (pid == -1) {
        perror("clone");
        free(stack);
        exit(EXIT_FAILURE);
    }

    waitpid(pid, NULL, 0);
    free(stack);
    return 0;
}
