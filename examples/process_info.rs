use marauder::process::{Process, ProcessAccess};

fn main() {
    let current_path = std::env::current_exe().expect("current executable path");
    let current_name = current_path
        .file_name()
        .expect("current executable file name")
        .to_string_lossy()
        .into_owned();
    let process = Process::current(ProcessAccess::read_write()).expect("open current process");
    let process_info = process.info().expect("query current process info");
    let matching_processes = Process::find_all_by_name(&process_info.name).expect("find matching current processes");
    println!(
        "opened process: {} pid={} parent={} threads={} priority={} current={} same-name-matches={}",
        process_info.name,
        process_info.id,
        process_info.parent_id,
        process_info.thread_count,
        process_info.base_priority,
        process_info.is_current(),
        matching_processes.len()
    );

    if let Some(match_) = matching_processes.first() {
        let opened_match = match_.open(ProcessAccess::query()).expect("open process info match");
        println!(
            "first same-name match opened from ProcessInfo: pid={}",
            opened_match.id().expect("query matched process id")
        );
    }

    println!("process id: {}", process.id().expect("query process id"));

    let allocation = process.memory().allocate_read_write(16).expect("allocate memory");
    let region = process
        .memory()
        .query(allocation.address_usize())
        .expect("query allocated region");
    println!(
        "allocation: {} region-protect=0x{:x} access={}",
        allocation,
        region.protect.0,
        region.access()
    );

    let guard = process
        .memory()
        .protect_scoped(
            allocation.address_usize(),
            allocation.size(),
            windows::Win32::System::Memory::PAGE_READONLY,
        )
        .expect("temporarily protect allocation");
    let read_only_region = process
        .memory()
        .query(allocation.address_usize())
        .expect("query read-only allocation");
    println!("temporary protection access={}", read_only_region.access());
    guard.restore().expect("restore allocation protection");

    process
        .memory()
        .write_c_string(allocation.address_usize(), "marauder")
        .expect("write memory string");
    let string = process
        .memory()
        .read_string(allocation.address_usize(), 16)
        .expect("read memory string");
    println!("string round trip: {string}");

    process
        .memory()
        .write_value(allocation.address_usize(), &0xfeed_beefu32)
        .expect("write typed value");
    let value = process
        .memory()
        .read_value::<u32>(allocation.address_usize())
        .expect("read typed value");
    println!("typed round trip: 0x{value:x}");

    let array_values = [0x11_u32, 0x22, 0x33, 0x44];
    process
        .memory()
        .write_array(allocation.address_usize(), &array_values)
        .expect("write typed array");
    let read_back = process
        .memory()
        .read_array::<u32>(allocation.address_usize(), array_values.len())
        .expect("read typed array");
    println!("array round trip: {read_back:x?}");

    println!("modules:");
    let modules = process.modules().list().expect("list modules");
    let main_module = process.modules().main().expect("find main module");
    println!("main module: {main_module}");
    if let Some(module) = process.modules().find(&current_name).expect("find current module") {
        println!("current module by name: {}", module.name);
    }
    if let Some(module) = process
        .modules()
        .find_by_path(current_path.to_string_lossy().as_ref())
        .expect("find module by path")
    {
        println!("current module by path: {}", module.name);
    }
    if let Some(module) = process
        .modules()
        .find_by_address(main as usize)
        .expect("find module by address")
    {
        println!("main is inside module: {}", module.name);
    }

    for module in modules.into_iter().take(8) {
        println!("- {module}");
    }
}
