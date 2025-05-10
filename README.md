# net_device_mapping (in the works)

Goals:
- a gRPC + capnproto RPC daemon that will track all network namespaces (tracking is already done, only need to add RPC).
- network device mapper + tracker + GUI for it.
- utility to move existing processes between network namespaces.

`samples/` directory is just a C code playground, nothing serious there.
