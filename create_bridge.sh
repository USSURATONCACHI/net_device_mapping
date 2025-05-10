#!/bin/bash -e

# 0. Create three test namespace
ip netns add test_ns_1
ip netns add test_ns_2
ip netns add test_ns_3

ip netns exec test_ns_1 ip link set lo up
ip netns exec test_ns_2 ip link set lo up
ip netns exec test_ns_3 ip link set lo up

# 1. Create the bridge
ip link add name test_bridge0 type bridge
ip link set test_bridge0 up

# 2. Create three veth-pairs (one end for the bridge, one for each namespace)
ip link add test_veth1_br type veth peer name test_veth1_ns
ip link add test_veth2_br type veth peer name test_veth2_ns
ip link add test_veth3_br type veth peer name test_veth3_ns

# 3. Attach the “-br” ends to the bridge and bring them up
ip link set test_veth1_br master test_bridge0
ip link set test_veth2_br master test_bridge0
ip link set test_veth3_br master test_bridge0

ip link set test_veth1_br up
ip link set test_veth2_br up
ip link set test_veth3_br up

# 5. Move each peer into its namespace
ip link set test_veth1_ns netns test_ns_1
ip link set test_veth2_ns netns test_ns_2
ip link set test_veth3_ns netns test_ns_3

# 6. Inside each namespace, bring up the veth end
ip netns exec test_ns_1 ip link set test_veth1_ns up
ip netns exec test_ns_2 ip link set test_veth2_ns up
ip netns exec test_ns_3 ip link set test_veth3_ns up
