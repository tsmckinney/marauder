# Examples

All examples are under the repository-level `examples/` directory and are wired
through `marauder/Cargo.toml`.

## Current-Process Inspection

```bash
cargo run -p marauder --example process_info
```

Shows:

- `Process::current`
- `Process::info`
- process discovery and `ProcessInfo::open`
- owned allocations
- memory region diagnostics
- scoped page protection
- bounded string reads/writes
- typed value and array reads/writes
- module lookup by main module, name, path, and address

This is the best first example when learning the inspection API.

## Readable Region Scan

```bash
cargo run -p marauder --example process_scan
```

Allocates a page in the current process, writes a marker, finds the readable
region containing that marker, and scans it with `Pattern`.

## Module-Bounded Scan

```bash
cargo run -p marauder --example module_scan
```

Places a marker in the current executable module and scans only readable ranges
that intersect the main module.

## Toy Manual Map

```bash
cargo run -p marauder --example toy_manual_map
```

This is a non-operational teaching model. It uses fake process memory, fake
modules, fake dependencies, fake imports, fake TLS callbacks, and a fake thread
register set. It does not parse a real PE file, open another process, allocate
remote memory, or execute injected code.

Use this example to understand the broad data flow without producing a working
manual mapper.

## Loader Injection Example

```bash
$env:dll_path = "C:\path\to\sample_dll.dll"
$env:process_name = "target_process.exe"
cargo run -p marauder --example injector
```

Optional environment flags:

```bash
$env:load_library_ex = "1"
$env:randomize_file_name = "1"
```

This example uses the supported Windows loader path. See
[Injection And Safety](injection.md).

## DLL Examples

Build or inspect:

- `sample_dll`
- `macro_dll`
- `tokio_dll`

The macro examples demonstrate `#[marauder::dll_main]` for synchronous and
async entry points.
