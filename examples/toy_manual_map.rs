//! A non-operational manual-map teaching model.
//!
//! This example does not open another process, allocate remote memory, parse a
//! real PE file, or execute injected code. It models the broad phases using
//! plain Rust structs so the data flow is visible without creating a usable
//! injector implementation.

use std::collections::BTreeMap;

const TARGET_BASE: usize = 0x5000_0000;
const PREFERRED_BASE: usize = 0x4000_0000;

fn main() {
    let module = ToyModule::sample();
    let mut process = ToyProcess::new();

    println!("toy target before mapping");
    process.dump_thread();

    let mapped = manual_map_toy_module(&mut process, &module);

    println!("\nmanual-map teaching trace");
    for event in &mapped.trace {
        println!("- {event}");
    }

    println!("\ntoy target after mapping");
    process.dump_region(mapped.base, mapped.size);
    process.dump_thread();
}

fn manual_map_toy_module(process: &mut ToyProcess, module: &ToyModule) -> MappedModule {
    let mut trace = Vec::new();
    trace.push(format!(
        "validated toy header: preferred_base=0x{:x}, image_size=0x{:x}",
        module.preferred_base, module.image_size
    ));

    let base = process.allocate(module.image_size);
    trace.push(format!("allocated fake target memory at 0x{base:x}"));

    for dependency in &module.dependencies {
        let loaded_base = process.load_dependency(dependency);
        trace.push(format!(
            "loaded dependency {} at fake base 0x{loaded_base:x}",
            dependency.name
        ));
    }

    for section in &module.sections {
        process.write(base + section.virtual_address, &section.bytes);
        trace.push(format!(
            "copied section {} to 0x{:x} ({} bytes)",
            section.name,
            base + section.virtual_address,
            section.bytes.len()
        ));
    }

    let delta = base.wrapping_sub(module.preferred_base);
    for relocation in &module.relocations {
        let address = base + relocation.offset;
        let original = process.read_usize(address);
        process.write_usize(address, original.wrapping_add(delta));
        trace.push(format!(
            "relocated pointer at 0x{address:x}: 0x{original:x} -> 0x{:x}",
            original.wrapping_add(delta)
        ));
    }

    for import in &module.imports {
        let resolved = process
            .exports
            .get(&(import.module, import.name))
            .copied()
            .expect("toy import should exist in fake export table");
        process.write_usize(base + import.write_offset, resolved);
        trace.push(format!(
            "resolved import {}!{} to 0x{resolved:x} and wrote IAT slot 0x{:x}",
            import.module,
            import.name,
            base + import.write_offset
        ));
    }

    for tls_callback in &module.tls_callbacks {
        trace.push(format!(
            "would invoke TLS callback at 0x{:x} before entrypoint",
            base + tls_callback.rva
        ));
    }

    let entrypoint = base + module.entrypoint_rva;
    process.thread.instruction_pointer = entrypoint;
    trace.push(format!(
        "pointed fake thread instruction pointer at entrypoint 0x{entrypoint:x}"
    ));

    MappedModule {
        base,
        size: module.image_size,
        trace,
    }
}

struct MappedModule {
    base: usize,
    size: usize,
    trace: Vec<String>,
}

struct ToyProcess {
    next_allocation: usize,
    memory: Vec<MemoryRegion>,
    loaded_modules: BTreeMap<&'static str, usize>,
    exports: BTreeMap<(&'static str, &'static str), usize>,
    thread: ToyThread,
}

impl ToyProcess {
    fn new() -> Self {
        Self {
            next_allocation: TARGET_BASE,
            memory: Vec::new(),
            loaded_modules: BTreeMap::new(),
            exports: BTreeMap::from([
                (("toyhost.dll", "host_log"), 0x7000_0100),
                (("toyhost.dll", "host_now"), 0x7000_0200),
                (("toytime.dll", "host_ticks"), 0x7100_0100),
            ]),
            thread: ToyThread {
                instruction_pointer: 0x1000,
                stack_pointer: 0x8000,
            },
        }
    }

    fn allocate(&mut self, size: usize) -> usize {
        let base = self.next_allocation;
        self.next_allocation += align_up(size, 0x1000);
        self.memory.push(MemoryRegion {
            base,
            bytes: vec![0; size],
        });
        base
    }

    fn load_dependency(&mut self, dependency: &ToyDependency) -> usize {
        *self
            .loaded_modules
            .entry(dependency.name)
            .or_insert(dependency.preferred_base)
    }

    fn write(&mut self, address: usize, bytes: &[u8]) {
        let region = self.region_mut(address, bytes.len());
        let offset = address - region.base;
        region.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    fn read_usize(&self, address: usize) -> usize {
        let region = self.region(address, std::mem::size_of::<usize>());
        let offset = address - region.base;
        let mut bytes = [0; std::mem::size_of::<usize>()];
        bytes.copy_from_slice(&region.bytes[offset..offset + std::mem::size_of::<usize>()]);
        usize::from_le_bytes(bytes)
    }

    fn write_usize(&mut self, address: usize, value: usize) { self.write(address, &value.to_le_bytes()); }

    fn dump_region(&self, base: usize, size: usize) {
        let region = self.region(base, size);
        println!("mapped region 0x{:x}..0x{:x}", region.base, region.base + region.bytes.len());
        for (index, chunk) in region.bytes.chunks(16).enumerate() {
            if chunk.iter().all(|byte| *byte == 0) {
                continue;
            }
            print!("  0x{:08x}: ", region.base + index * 16);
            for byte in chunk {
                print!("{byte:02x} ");
            }
            println!();
        }
    }

    fn dump_thread(&self) {
        println!(
            "thread ip=0x{:x}, sp=0x{:x}",
            self.thread.instruction_pointer, self.thread.stack_pointer
        );
    }

    fn region(&self, address: usize, size: usize) -> &MemoryRegion {
        self.memory
            .iter()
            .find(|region| region.contains(address, size))
            .expect("toy address must be inside an allocated region")
    }

    fn region_mut(&mut self, address: usize, size: usize) -> &mut MemoryRegion {
        self.memory
            .iter_mut()
            .find(|region| region.contains(address, size))
            .expect("toy address must be inside an allocated region")
    }
}

struct MemoryRegion {
    base: usize,
    bytes: Vec<u8>,
}

impl MemoryRegion {
    fn contains(&self, address: usize, size: usize) -> bool {
        address >= self.base && address + size <= self.base + self.bytes.len()
    }
}

struct ToyThread {
    instruction_pointer: usize,
    stack_pointer: usize,
}

struct ToyModule {
    preferred_base: usize,
    image_size: usize,
    entrypoint_rva: usize,
    sections: Vec<ToySection>,
    relocations: Vec<ToyRelocation>,
    dependencies: Vec<ToyDependency>,
    imports: Vec<ToyImport>,
    tls_callbacks: Vec<ToyTlsCallback>,
}

impl ToyModule {
    fn sample() -> Self {
        let mut data = vec![0; 0x40];
        data[0..std::mem::size_of::<usize>()].copy_from_slice(&(PREFERRED_BASE + 0x1010).to_le_bytes());

        Self {
            preferred_base: PREFERRED_BASE,
            image_size: 0x3000,
            entrypoint_rva: 0x1010,
            sections: vec![
                ToySection {
                    name: ".text",
                    virtual_address: 0x1000,
                    bytes: vec![0xcc, 0xcc, 0xcc, 0xc3],
                },
                ToySection {
                    name: ".data",
                    virtual_address: 0x2000,
                    bytes: data,
                },
            ],
            relocations: vec![ToyRelocation { offset: 0x2000 }],
            dependencies: vec![
                ToyDependency {
                    name: "toyhost.dll",
                    preferred_base: 0x7000_0000,
                },
                ToyDependency {
                    name: "toytime.dll",
                    preferred_base: 0x7100_0000,
                },
            ],
            imports: vec![
                ToyImport {
                    module: "toyhost.dll",
                    name: "host_log",
                    write_offset: 0x2010,
                },
                ToyImport {
                    module: "toyhost.dll",
                    name: "host_now",
                    write_offset: 0x2018,
                },
                ToyImport {
                    module: "toytime.dll",
                    name: "host_ticks",
                    write_offset: 0x2020,
                },
            ],
            tls_callbacks: vec![ToyTlsCallback { rva: 0x1020 }, ToyTlsCallback { rva: 0x1030 }],
        }
    }
}

struct ToySection {
    name: &'static str,
    virtual_address: usize,
    bytes: Vec<u8>,
}

struct ToyRelocation {
    offset: usize,
}

struct ToyDependency {
    name: &'static str,
    preferred_base: usize,
}

struct ToyImport {
    module: &'static str,
    name: &'static str,
    write_offset: usize,
}

struct ToyTlsCallback {
    rva: usize,
}

fn align_up(value: usize, alignment: usize) -> usize { (value + alignment - 1) & !(alignment - 1) }
