#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <linux/netlink.h>
#include <linux/rtnetlink.h>

#define BUFLEN 8192

int main(void) {
    int sock = socket(AF_NETLINK, SOCK_RAW, NETLINK_ROUTE);
    if (sock < 0) {
        perror("socket");
        return 1;
    }

    // 1) Bind the socket, subscribing to the NSID group
    struct sockaddr_nl addr = { .nl_family = AF_NETLINK,
        .nl_pid    = 0,               // let kernel assign
        .nl_groups = RTNLGRP_NSID };  // join NSID group
    if (bind(sock, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        perror("bind");
        close(sock);
        return 1;
    }

    // Enable receiving NSID from all namespaces
    int listen_all = 1;
    if (setsockopt(sock, SOL_NETLINK, NETLINK_LISTEN_ALL_NSID,
                   &listen_all, sizeof(listen_all)) < 0) {
        perror("setsockopt LISTEN_ALL_NSID");
        close(sock);
        return 1;
    }

    // Join NSID multicast group
    int grp = RTNLGRP_NSID;
    if (setsockopt(sock, SOL_NETLINK, NETLINK_ADD_MEMBERSHIP,
                   &grp, sizeof(grp)) < 0) {
        perror("setsockopt ADD_MEMBERSHIP");
        close(sock);
        return 1;
    }

    while (1) {
        char buf[BUFLEN];
        struct iovec iov = { buf, sizeof(buf) };
        struct sockaddr_nl sa;
        char cbuf[CMSG_SPACE(sizeof(int))];
        struct msghdr msg = {
            .msg_name    = &sa,
            .msg_namelen = sizeof(sa),
            .msg_iov     = &iov,
            .msg_iovlen  = 1,
            .msg_control = cbuf,
            .msg_controllen = sizeof(cbuf),
        };

        int len = recvmsg(sock, &msg, 0);
        if (len < 0) {
            perror("recvmsg");
            break;
        }

        int nsid = -1;
        for (struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
             c; c = CMSG_NXTHDR(&msg, c)) {
            if (c->cmsg_level == SOL_NETLINK &&
                c->cmsg_type  == NETLINK_LISTEN_ALL_NSID) {
                nsid = *(int *)CMSG_DATA(c);
                break;
            }
        }

        for (struct nlmsghdr *nh = (struct nlmsghdr *)buf;
             NLMSG_OK(nh, len);
             nh = NLMSG_NEXT(nh, len)) {

            if (nh->nlmsg_type == RTM_NEWNSID) {
                printf("NSID %d assigned\n", nsid);
            } else if (nh->nlmsg_type == RTM_DELNSID) {
                printf("NSID %d removed\n", nsid);
            }
        }
    }

    close(sock);
    return 0;
}
