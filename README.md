# marauder

`marauder` is a Windows-only Rust library for learning and building process
inspection tooling, DLL entry points, and normal Windows loader based DLL
injection. Its current direction is inspired by
[DarthTon/Blackbone](https://github.com/DarthTon/Blackbone): layered process,
memory, module, pattern, and injector APIs with small examples for each concept.

The project keeps higher-risk loader concepts in non-operational teaching
models. Supported injection uses the normal Windows loader path; manual mapping,
thread hijacking, PE cloaking, and similar options are rejected by the runtime
configuration.

## Status

- [x] DLL creation macros
- [x] Normal loader based injection
- [x] Process, memory, module, and pattern inspection helpers
- [x] Current-process educational examples
- [ ] D3D hooks

## Install

```toml
[dependencies]
marauder = "0.1.0"

# or, if you only want the DLL creation macros:
marauder-macros = "0.1.0"
```

## Documentation

- [Documentation index](docs/index.md)
- [Process and memory guide](docs/process-memory.md)
- [Examples guide](docs/examples.md)
- [Injection and safety boundary](docs/injection.md)
- [Blackbone inspiration notes](docs/blackbone-inspiration.md)

## Quick Start

Run the safe current-process inspection example:

```bash
cargo run -p marauder --example process_info
```

That example opens its own process, prints process and module metadata,
allocates memory, queries region protection, temporarily changes page
protection with an RAII guard, and performs typed/string memory round trips.

Scan memory in the current process:

```bash
cargo run -p marauder --example process_scan
cargo run -p marauder --example module_scan
```

Learn manual-map concepts without building a real manual mapper:

```bash
cargo run -p marauder --example toy_manual_map
```

## DLL Entry Points

```rust
#[marauder::dll_main]
fn main() {
    println!("loaded from DllMain: {module_handle:?}");
}

#[marauder::dll_main]
async fn async_main() {
    println!("async DllMain runs on the Tokio runtime");
}
```

## Supported Injection Path

The injector supports `LoadLibraryA` / `LoadLibraryExA` through
`CreateRemoteThread`:

```bash
$env:dll_path = "C:\path\to\sample_dll.dll"
$env:process_name = "target_process.exe"
cargo run -p marauder --example injector
```

Optional flags:

```bash
$env:load_library_ex = "1"
$env:randomize_file_name = "1"
```

See [docs/injection.md](docs/injection.md) for what is supported and what is
intentionally rejected.
