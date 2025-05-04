#define _GNU_SOURCE
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <fcntl.h>

int main(int argc, char *argv[]) {
    int fd;

    if (argc != 2) {
        fprintf(stderr, "Usage: %s <netns-path>\n", argv[0]);
        exit(EXIT_FAILURE);
    }

    fd = open(argv[1], O_RDONLY);
    if (fd == -1) {
        perror("open");
        exit(EXIT_FAILURE);
    }

    if (setns(fd, CLONE_NEWNET) == -1) {
        perror("setns");
        close(fd);
        exit(EXIT_FAILURE);
    }

    close(fd);

    printf("Joined network namespace: %s (PID: %d)\n", argv[1], getpid());
    system("ip link");
    return 0;
}
