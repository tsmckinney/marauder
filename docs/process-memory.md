# Process And Memory

This guide covers the safe inspection APIs in `marauder::process`.

## Open A Process

Use explicit access rights:

```rust
use marauder::process::{Process, ProcessAccess};

let process = Process::current(ProcessAccess::read_write())?;
let info = process.info()?;

println!(
    "{} pid={} parent={} threads={}",
    info.name, info.id, info.parent_id, info.thread_count
);
```

You can also discover processes first and open one of the snapshot entries:

```rust
let matches = Process::find_all_by_name("target_process.exe")?;
let process = matches
    .first()
    .ok_or(marauder::error::Error::ProcessNotFound)?
    .open(ProcessAccess::query())?;
```

## Modules

Module inspection is scoped to an opened process:

```rust
let modules = process.modules().list()?;
let main = process.modules().main()?;
let kernel32 = process.modules().find("KERNEL32.DLL")?;
let owner = process.modules().find_by_address(main.base_address)?;

println!("{main}");
```

`ModuleInfo` exposes:

- `name`
- `path`
- `base_address`
- `size`
- `range()`
- `end_address()`
- `contains(address, size)`
- `intersection(region)`

Its `Display` implementation prints a compact diagnostic summary:

```text
process_info.exe 0x7ff700000000..0x7ff700049000 size=0x49000
```

## Memory Regions

Query a single address:

```rust
let region = process.memory().query(address)?;
println!("{} {}", region.range().end_address(), region.access());
```

Common filters are available:

```rust
let readable = process.memory().readable_regions()?;
let writable = process.memory().writable_regions()?;
let executable = process.memory().executable_regions()?;
```

`MemoryRegion::access()` prints compact protection summaries such as `rw-`,
`r-x`, or `no-access,guard`.

## Allocations

Allocations are owned RAII values and free themselves on drop:

```rust
let allocation = process.memory().allocate_read_write(0x1000)?;
println!("{allocation}");

assert!(allocation.contains(allocation.address_usize(), 16));
```

`ProcessAllocation` exposes:

- `address()`
- `address_usize()`
- `size()`
- `range()`
- `end_address()`
- `contains(address, size)`
- `free()`

## Read And Write

Raw bytes:

```rust
process.memory().write(allocation.address_usize(), b"marauder")?;
let bytes = process.memory().read(allocation.address_usize(), 8)?;
```

Bounded strings:

```rust
process
    .memory()
    .write_c_string(allocation.address_usize(), "marauder")?;

let value = process
    .memory()
    .read_string(allocation.address_usize(), 32)?;
```

Typed plain-old-data values and arrays:

```rust
process.memory().write_value(address, &0xfeed_beefu32)?;
let value = process.memory().read_value::<u32>(address)?;

process.memory().write_array(address, &[1_u32, 2, 3])?;
let values = process.memory().read_array::<u32>(address, 3)?;
```

For user-defined structs, only implement `PlainOldData` for types that are safe
to reinterpret as raw bytes: no references, no drop behavior, no invalid bit
patterns, and a stable primitive or `repr(C)` layout.

## Scoped Protection

Use `protect_scoped` when a temporary page-protection change should be restored:

```rust
use windows::Win32::System::Memory::PAGE_READONLY;

let guard = process.memory().protect_scoped(
    allocation.address_usize(),
    allocation.size(),
    PAGE_READONLY,
)?;

println!("{}", process.memory().query(allocation.address_usize())?.access());

guard.restore()?;
```

The guard also attempts restoration on drop.

## Pattern Scanning

Patterns are pure byte matchers:

```rust
use marauder::pattern::Pattern;

let exact = Pattern::exact(b"marker")?;
let ida = "48 8B ?? 90".parse::<Pattern>()?;
```

Process-aware scanning lives on `ProcessMemory`:

```rust
let matches = process.memory().scan_module(&main, &exact)?;

for match_ in matches {
    println!("0x{:x}", match_.address);
}
```

Available scanners:

- `scan_range(range, pattern)`
- `scan_region(region, pattern)`
- `scan_readable(pattern)`
- `scan_module(module, pattern)`
