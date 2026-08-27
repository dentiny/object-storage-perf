# Performance investigation

This document summarizes the main correctness and performance problems found
while building the benchmark, how they were diagnosed, and the changes that
produced reliable results. It is intentionally focused on conclusions that are
useful for future benchmark runs.

## Final benchmark environment

- Kubernetes cluster: Crusoe `hark-norway-gpu`
- Object storage: Crusoe Object Storage
- Bucket: `hjiang-test-bucket`
- Location: `eu-norway1-a`
- Endpoint: `https://object.eu-norway1-a.crusoecloudcompute.com`
- Benchmark node: Crusoe `s2a.80x`, with 80 vCPUs and an approximately
  200 Gbps network path
- Read request size: 10 MiB
- Source object size: 512 MiB
- Read concurrency: 64

The endpoint is private and location-specific. The benchmark Pod must run on
Crusoe compute in `eu-norway1-a`, and it must use a dedicated Crusoe Object
Storage API key.

## Problems and solutions

### The original concurrency model hid the real request count

The first implementation used one general concurrency setting together with
multipart concurrency. Their product could create far more part writes than
the configured value suggested, while read and stat workloads needed different
limits.

The benchmark now has independent limits for:

- in-flight range reads;
- in-flight multipart part writes;
- in-flight stat requests.

Write concurrency is one global semaphore shared by all multipart uploads. It
is not multiplied by the number of objects being uploaded.

### A single source object could measure a hot-object limit

Reading repeatedly from one object could measure backend caching or a
per-object limit instead of aggregate storage performance.

The benchmark now prepares one 512 MiB source object per read worker and
distributes range reads across all of them. Source preparation happens before
the measurement window.

### Benchmark deadlines could interrupt multipart writer state

Cancelling an in-flight multipart part write at the phase deadline could leave
the writer in an invalid state. Aborting many such writers caused high memory
usage, hangs, and an eventual `OOMKilled` result.

Individual part writes are now allowed to complete. The deadline only prevents
new measured part writes from starting. Multipart completion, abort, and
cleanup are recorded separately from measured part-write failures.

### Latency and success metrics were too coarse

An average latency alone hid tail behavior, and combining multipart control
failures with part-write failures made the write success rate ambiguous.

The report now includes:

- HDR histogram mean, p50, p95, and p99 latency;
- successful bytes or operations divided by measured wall time;
- measured-operation success rate and categorized errors;
- separate multipart start, completion, abort, and coordination errors.

### The Linux musl allocator caused severe `mmap` contention

Early profiles showed a large share of CPU time in `mmap`, `munmap`, and kernel
memory-map locking. The static musl build was allocating and releasing many
network buffers through mappings, preventing the application from draining
socket receive queues quickly enough.

The binary now uses `tikv-jemallocator` as its global allocator. This sharply
reduced mapping syscalls and lock contention. In the earlier OCI environment,
the change increased peak read throughput to approximately 5,587 MiB/s at
concurrency 64, enough to saturate that node's network path.

### More concurrency was not always faster

Concurrency sweeps showed that throughput can decrease after the client or
backend reaches saturation. Before the allocator fix, lower concurrency values
such as 16 or 24 could outperform 64 or 128. Concurrency therefore remains a
benchmark parameter rather than a value that should always be maximized.

Use a sweep and keep request size, CPU limit, node, image, and bucket unchanged
between runs.

### CPU limits constrained Crusoe read throughput

A controlled Crusoe comparison used the same node, image, bucket, 10 MiB read
size, 64 read workers, and 60-second measurement window:

- With an 8-core limit, read throughput was 4,130 MiB/s, or approximately
  34.6 Gbps. The container used all 8 cores and was continuously throttled.
- With a 64-core limit, read throughput was 22,933 MiB/s, or approximately
  192.4 Gbps. The process used about 32–35 cores without throttling.

Increasing the CPU limit by 8x increased throughput by about 5.55x, not 8x.
The larger run reached the approximately 200 Gbps network ceiling, so adding
more CPU beyond roughly 35 actively used cores is not expected to improve this
workload on the same node.

## Approaches that did not explain the low throughput

- Reusing one OpenDAL reader per worker did not materially improve throughput.
- Raising Tokio worker count alone did not help when CPU was not throttled.
- Reading from multiple objects made the benchmark representative but did not
  by itself remove the client-side bottleneck.
- A native OCI Python SDK control was slower than the Rust/OpenDAL S3 path, so
  it did not show that OpenDAL's S3 implementation was uniquely slow.
- Moving to a dedicated node did not change the earlier result, which ruled out
  noisy neighbors as the primary cause.
- Increasing a range request from 2 MiB to 10 MiB did not by itself remove the
  bottleneck. Larger requests reduce request-level overhead but do not
  automatically improve throughput after the NIC is saturated.

## How to investigate future regressions

Do not infer the bottleneck from throughput alone. Collect these signals during
the same measurement window:

1. Read `cpu.stat` from the benchmark container's cgroup. Compare CPU usage with
   the configured limit and check throttling deltas.
2. Read the node interface byte counters once per second to calculate ingress
   and egress Gbps.
3. Use `ss -tinm` to count established connections and inspect receive queues,
   send queues, congestion windows, and retransmissions.
4. Capture a `perf` profile with frame pointers and inspect both userspace and
   kernel stacks.
5. Sweep one variable at a time. Keep the node, image, bucket, duration,
   request size, and all other concurrency settings fixed.

The practical stopping condition is clear: if CPU is not throttled and node
traffic is already near 200 Gbps, the current Crusoe network path—not additional
Tokio workers or CPU quota—is the throughput limit.
