use marauder::{
    injector::{Config, Injector},
    windows::utils::get_process_id,
};

fn main() {
    let dll_path = std::env::var("dll_path").expect("You must provide a DLL path.");
    let process_name = std::env::var("process_name").expect("You must provide a process name.");

    let path = std::path::Path::new(&dll_path);
    if !path.exists() {
        panic!("Could not find a DLL at {}. Please check if such a DLL exists.", dll_path);
    }
    let config = Config::default();
    let injector = Injector::new(config);

    let pid = get_process_id(&process_name).unwrap();
    injector.inject(pid, &dll_path).unwrap();
    println!("Successfully injected DLL from {dll_path} into process \"{process_name}\"!")
}
