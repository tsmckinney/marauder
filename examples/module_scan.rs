use marauder::{
    pattern::Pattern,
    process::{Process, ProcessAccess},
};

#[used]
static MODULE_MARKER: [u8; 28] = *b"marauder-owned-module-marker";

fn main() {
    let process = Process::current(ProcessAccess::read()).expect("open current process for reading");
    let module = process.modules().main().expect("find current executable module");
    let marker = std::hint::black_box(&MODULE_MARKER);
    let pattern = Pattern::exact(marker).expect("build marker pattern");
    let matches = process
        .memory()
        .scan_module(&module, &pattern)
        .expect("scan current executable module");

    assert!(
        matches.iter().any(|match_| match_.address == marker.as_ptr() as usize),
        "expected to find marker static in current executable module"
    );

    println!("found {} marker match(es) in {}", matches.len(), module);

    for match_ in matches {
        println!("  0x{:x}", match_.address);
    }
}
