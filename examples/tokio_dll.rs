//! This is a sample DLL using the tokio runtime.

// By default, setting the main function as asynchronous will use the tokio
// runtime. Neither I nor the original author have any plans at the moment
// regarding support for any others, as I prefer to prioritize community
// efforts to add more runtimes.
#[marauder::dll_main]
async fn main() {
    println!("Hi from tokio, module_handle: {:?}", module_handle);
}
