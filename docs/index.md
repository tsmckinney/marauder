# Marauder Documentation

Marauder is organized around small Windows process-inspection layers:

- `process`: owned process handles, process snapshots, memory, modules, ranges,
  and pattern scanning.
- `pattern`: pure byte-pattern matching, including IDA-style wildcard strings.
- `injector`: normal Windows loader based DLL injection.
- `macros`: DLL entry-point helpers.
- `examples`: runnable demonstrations that keep high-risk concepts isolated or
  non-operational.

## Guides

- [Process and memory](process-memory.md)
- [Examples](examples.md)
- [Injection and safety boundary](injection.md)
- [Blackbone inspiration notes](blackbone-inspiration.md)

## Core Ideas

Marauder borrows Blackbone's layered shape without copying its full feature set.
The goal is to make Windows process concepts visible and understandable:

- Open a process with explicit access rights.
- Query process identity from snapshots.
- Inspect modules by name, path, address, or main executable module.
- Allocate memory with RAII cleanup.
- Read and write bytes, typed plain-old-data values, arrays, and bounded
  strings.
- Query and temporarily change page protection with scoped restore.
- Scan readable regions or module ranges with pure byte patterns.
- Keep unsupported stealth-oriented loader techniques out of the operational
  API.

## Verification

The current educational examples are intended to run locally on Windows:

```bash
cargo run -p marauder --example process_info
cargo run -p marauder --example process_scan
cargo run -p marauder --example module_scan
cargo run -p marauder --example toy_manual_map
```

The workspace should pass:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy -p marauder --all-targets
```
