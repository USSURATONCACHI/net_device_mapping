#define _GNU_SOURCE
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main() {
    if (unshare(CLONE_NEWNET) == -1) {
        perror("unshare");
        exit(EXIT_FAILURE);
    }

    printf("Process in new network namespace (PID: %d)\n", getpid());
    system("ip link");
    return 0;
}
