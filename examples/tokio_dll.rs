//! This is a sample of a DLL using the tokio runtime

// By default putting dll_main as async will use the tokio runtime, I have no
// plans atm of supporting any others as its very simple to build a runtime
#[marauder::dll_main]
#[tokio::main]
async fn main() {
    println!("Hi from tokio, instance_handle: {:?}", instance_handle);
}
