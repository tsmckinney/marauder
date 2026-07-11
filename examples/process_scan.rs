use marauder::{
    pattern::Pattern,
    process::{Process, ProcessAccess},
};

const MARKER: &[u8] = b"marauder-scan-marker";

fn main() {
    let process = Process::current(ProcessAccess::read_write()).expect("open current process");
    let allocation = process.memory().allocate_read_write(0x1000).expect("allocate scan page");
    let marker_address = allocation.address_usize() + 0x180;
    let pattern = Pattern::exact(MARKER).expect("build exact pattern");

    process.memory().write(marker_address, MARKER).expect("write marker");

    let region = process
        .memory()
        .readable_regions()
        .expect("enumerate readable regions")
        .into_iter()
        .find(|region| region.contains(marker_address, MARKER.len()))
        .expect("find marker region");
    let found = process
        .memory()
        .scan_region(&region, &pattern)
        .expect("scan marker region")
        .into_iter()
        .find(|match_| match_.address == marker_address)
        .expect("find marker pattern");

    println!(
        "found marker at 0x{:x} in readable region 0x{:x}..0x{:x}",
        found.address,
        found.range.base_address,
        found.range.end_address()
    );
}
