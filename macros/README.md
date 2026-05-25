# marauder_macros

This is currently to make it easier to create DLLs, it will eventually be expanded to support other things.

## Install
```toml
[dependencies]
marauder_macros = "0.1.0"
```

## Examples

### Easy DLL creation with minimal boilerplate
```rust
#[marauder_macros::dll_main]
fn main() {
    // This is a fully functional DLL ready for injection!
    println!("I am a DLL inside the process, my module handle is: {}!", module_handle);
}
```