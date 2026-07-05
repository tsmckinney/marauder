# Injection And Safety Boundary

Marauder currently supports normal Windows loader based DLL injection. The
injector is intentionally narrow and rejects configuration options that would
turn it into a stealth loader.

## Supported

The supported path is:

- allocate a remote DLL path
- call `LoadLibraryA` or `LoadLibraryExA`
- execute through `CreateRemoteThread`
- wait for completion and report failure
- clean up temporary remote allocations

Example:

```rust
use marauder::injector::{Config, InjectionMethod, Injector};

let mut config = Config::default();
config.injection_method = InjectionMethod::LoadLibraryEx;
config.randomize_file_name = true;

let injector = Injector::new(config);
injector.inject(pid, r"C:\path\to\sample_dll.dll")?;
```

The example runner reads environment variables:

```bash
$env:dll_path = "C:\path\to\sample_dll.dll"
$env:process_name = "target_process.exe"
$env:load_library_ex = "1"
$env:randomize_file_name = "1"
cargo run -p marauder --example injector
```

## Rejected

These options are represented in the config surface so callers can see the
boundary, but they return `Error::UnsupportedInjectorFeature`:

- manual mapping
- thread hijacking
- thread cloaking
- PE header erasing or fake PE headers

Educational explanations of those concepts belong in toy models, not in the
operational injector.

## Educational Model

Run:

```bash
cargo run -p marauder --example toy_manual_map
```

That example models broad loader phases with ordinary Rust data structures:

- validate a fake module
- allocate fake target memory
- load fake dependencies
- copy fake sections
- apply fake relocations
- resolve fake imports
- trace fake TLS callbacks
- point a fake thread at a fake entrypoint

It intentionally does not open another process, parse a real PE file, allocate
remote memory, or execute injected code.
