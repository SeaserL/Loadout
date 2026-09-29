# Performance Baseline — v0.1.0

- **Measured**: 2026-09-29, release profile (`lto = "thin"`, `codegen-units = 1`), Windows 10 Pro 19045, loopback mock upstreams.
- **Command**: `cargo bench -p loadout-benchmarks -- --warm-up-time 1 --measurement-time 3`（criterion 中位数）。
- Harness §2.3 / ADR-003 §10 budgets: route selection P95 < 2 ms, local router overhead P95 < 5 ms, gateway added TTFT P95 < 10 ms.

## Gateway overhead

| Benchmark | Time | Notes |
|---|---|---|
| `ttft/direct_upstream` | 91 µs | client → mock upstream, first SSE event |
| `ttft/via_gateway_passthrough` | 524 µs | same through the gateway: **+0.43 ms** |
| `ttft/via_gateway_converted_messages` | 557 µs | Anthropic client → Chat upstream: **+0.47 ms** |
| `gateway_skeleton/get_models_keepalive` | 52 µs | auth + Host checks + listing, keep-alive |
| `client_auth/valid_bearer` | 112 ns | SHA-256 + map lookup |

## Routing (hot path)

| Benchmark | Time |
|---|---|
| `route_selection/exact_100_targets` | 0.23 µs |
| `route_selection/provider_model_100_targets` | 0.49 µs |
| `route_selection/virtual_100_targets_with_session` | 7.0 µs |
| `snapshot_swap/load` | 6.3 ns |
| `snapshot_swap/load_during_swaps` (writer republishing continuously) | 6.5 ns |

## Control plane

| Benchmark | Time |
|---|---|
| `snapshot_swap/build_100_targets_3_virtuals` | 40 µs |
| `snapshot_swap/store` | 4.0 µs |
| `sqlite_usage_write/writer_5000_events` | 13.6 ms (≈ 2.7 µs / event, off the request path) |
| `ipc_roundtrip/echo` | 31 µs |

## Streaming and conversion

| Benchmark | Time |
|---|---|
| `stream_proxy/passthrough_200_events` | 166 µs (0.8 µs / event) |
| `stream_proxy/convert_chat_to_messages_200_events` | 412 µs (2.1 µs / event) |
| `canonical_conversion/chat_to_messages` | 67 µs (large request with tools) |
| `canonical_conversion/passthrough_rewrite` | 29 µs |

## End to end (debug build, real daemon)

`stress_e2e`: 128 requests at concurrency 32 (mixed Chat / Messages, streaming and not) against a mock upstream that waits 50 ms: all succeed and are recorded; p50 61 ms, p95 62 ms.

## Footprint (release)

| Item | Value |
|---|---|
| `loadoutd.exe` | 14.0 MB |
| `loadout.exe` | 2.5 MB |
| `loadout-app.exe` | 8.7 MB |
| Package zip | 9.8 MB |
| Daemon start → IPC ready (`loadout daemon start`) | 0.44 s |
| Daemon idle memory | 25 MB working set, 7.5 MB private |
| Desktop app idle memory (WebView2 host process only) | 29 MB working set |

All budgets are met with more than an order of magnitude to spare. Re-run the suite after any router or gateway change (Harness §20) and update this file when a number moves noticeably.
