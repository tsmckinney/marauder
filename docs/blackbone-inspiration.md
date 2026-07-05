# Blackbone Inspiration Notes

Blackbone is one of marauder's design references. Its public project shape is
useful because it treats Windows process work as a set of layered abstractions:
process attachment, memory operations, module inspection, remote execution,
mapping, driver-backed helpers, samples, and tests.

Marauder should borrow the parts of that shape that make a Rust library easier
to understand and safer to use:

- Keep process, memory, module, hook, and injection concerns in separate modules.
- Prefer small typed wrappers over raw Windows calls at public API boundaries.
- Return structured errors instead of booleans or panics.
- Make examples runnable and focused on one task each.
- Document unsupported techniques at the enum/config boundary.
- Use toy models for high-risk loader concepts so the learning path remains
  visible without producing a stealth injector.

## Safe Architecture Targets

### Process

The long-term process API should own the process handle, process id, and basic
identity metadata. Opening and closing handles should happen through RAII, with
access rights kept explicit at construction.

Possible shape:

```rust
let current = Process::current(ProcessAccess::read_write())?;
let info = current.info()?;
let opened_from_snapshot = info.open(ProcessAccess::query())?;
let process = Process::open(pid, ProcessAccess::read_write())?;
let modules = process.modules().list()?;
let main = process.modules().main()?;
let kernel32 = process.modules().find("KERNEL32.DLL")?;
let memory = process.memory();
```

### Memory

Memory operations should move away from ad hoc free functions in user-facing
code. A `ProcessMemory` facade can make common reads, writes, allocations, and
protection changes easier to audit.

Possible shape:

```rust
let allocation = process.memory().allocate_read_write(size)?;
let allocation_range = allocation.range();
process.memory().write(allocation.address_usize(), bytes)?;
let bytes = process.memory().read(address, len)?;
process.memory().write_c_string(address, "marker")?;
let string = process.memory().read_string(address, max_len)?;
process.memory().write_value(address, &1234_u32)?;
let value = process.memory().read_value::<u32>(address)?;
process.memory().write_array(address, &[1_u32, 2, 3])?;
let values = process.memory().read_array::<u32>(address, 3)?;
let region = process.memory().query(address)?;
let guard = process.memory().protect_scoped(address, len, protection)?;
let readable = process.memory().readable_regions()?;
let matches = process.memory().scan_module(&module, &pattern)?;
```

`ProcessMemory` can also enumerate regions with `regions()`, which is useful
for diagnostics and future scanner-style examples. Region values expose helpers
such as `is_committed`, `is_readable`, `is_writable`, `is_executable`, `range`,
and `access` so diagnostics can show compact `rw-`/`r-x`-style summaries while
still preserving raw Windows flags.

### Modules

Module inspection should be separate from loading. A module API can enumerate
loaded modules, find a module by name, path, or contained address, and expose
base/size/path metadata. This matches how users naturally debug target-process
state without coupling that debugging view to injection. Module ranges should
expose helpers such as `end_address`, `contains`, and `intersection` so
examples do not hand-roll address math. Module values should also provide a
compact display summary for diagnostics.

Process and module snapshot failures should preserve the underlying Windows
error while adding context for the snapshot kind, process id, and failed stage.

### Patterns

Pattern matching should stay as a pure byte-slice utility. Process examples can
combine it with readable memory regions, but the matcher itself should not own
process access or scanning policy. `ProcessMemory` owns scanning policy by
combining `Pattern` with readable regions, explicit ranges, or module
intersections.

Possible shape:

```rust
let pattern = Pattern::exact(b"marker")?;
let ida_pattern = "48 8B ?? 90".parse::<Pattern>()?;
let offset = pattern.find_in(bytes);
```

### Injection

The supported injector remains normal Windows loader based:

- `LoadLibraryA`
- `LoadLibraryExA`
- `CreateRemoteThread`
- explicit cleanup and error reporting

Unsupported options such as manual mapping, thread hijacking, and PE cloaking
should stay rejected by config validation. Educational examples can model their
data flow with fake memory, fake dependencies, fake TLS callbacks, and fake
threads.

### Examples

Examples should be small and named after the thing they teach:

- `target_process`: a harmless process for normal loader injection examples.
- `injector`: supported `LoadLibrary`/`LoadLibraryEx` injection.
- `process_info`: current-process memory and module inspection.
- `process_scan`: current-process pattern scan over a readable region using
  `Pattern`.
- `module_scan`: current-process pattern scan constrained to the executable
  module range.
- `toy_manual_map`: a non-operational model of mapping steps.

## Current Follow-Up List

- Add focused diagnostics for future edge cases as real usage exposes them.

## References

- Blackbone repository: <https://github.com/DarthTon/Blackbone>
- Blackbone README feature list, including process memory, module, injection,
  manual mapping, and driver features: <https://github.com/DarthTon/Blackbone>
