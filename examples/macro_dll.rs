//! This is a sample DLL using our macro for DllMain generation.

// Basically, your main function is injected inside of a extern "system" DllMain
// function inside of a new thread. This is nearly identical to our sample_dll
// example, apart from it requiring less boilerplate code.
#[marauder::dll_main]
fn main() {
    // module_handle is exposed from our macro, so is dw_reason, and also
    // lp_reserved
    println!("Hi from the macro dll, module_handle: {:?}", module_handle);
}

// The code above will expand to roughly
// ```
// #[no_mangle]
// pub extern "system" fn DllMain(
//     module_handle: HMODULE,
//     dw_reason: DWORD,
//     lp_reserved: LPVOID,
// ) -> bool {
//     match dw_reason {
//         1u32 => {
//             std::thread::spawn(|| hack_thread());
//         },
//         _ => return false,
//     };
//
//     true
// }
// ```
