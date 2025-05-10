#!/bin/bash -e


ip link del test_veth1_br
ip link del test_veth2_br
ip link del test_veth3_br

ip link del test_bridge0

ip netns del test_ns_1
ip netns del test_ns_2
ip netns del test_ns_3
