use proc_macro::TokenStream;
use quote::quote;
use syn::ItemFn;

#[proc_macro_attribute]
pub fn dll_main(args: TokenStream, input: TokenStream) -> TokenStream {
    let mut input = syn::parse_macro_input!(input as syn::ItemFn);

    if input.sig.ident != "main" {
        return syn::Error::new_spanned(&input.sig.ident, "dll_main must be named main")
            .to_compile_error()
            .into();
    }

    if !input.sig.inputs.is_empty() {
        return syn::Error::new_spanned(&input.sig.inputs, "dll_main can't accept any arguments")
            .to_compile_error()
            .into();
    }

    let is_async = input.sig.asyncness.take().is_some();

    unsafe { create_main(input, args, is_async).unwrap_or_else(|e| e.to_compile_error().into()) }
}

unsafe fn create_main(mut input: ItemFn, _args: TokenStream, is_async: bool) -> Result<TokenStream, syn::Error> {
    let original_body = &input.block;
    let brace_token = input.block.brace_token;
    if !is_async {
        input.block = syn::parse2(quote! {
            {
                #[link(name = "kernel32")]
                extern "system" {
                    fn DisableThreadLibraryCalls(hlibmodule: *mut std::ffi::c_void) -> i32;
                }

                unsafe {
                    DisableThreadLibraryCalls(module_handle);
                }

                match dw_reason {
                    1u32 => {
                        let module_handle_raw = module_handle as isize;
                        let lp_reserved_raw = lp_reserved as isize;
                        std::thread::spawn(move || {
                            let module_handle = module_handle_raw as *mut std::ffi::c_void;
                            let lp_reserved = lp_reserved_raw as *mut std::ffi::c_void;
                            #original_body
                        });
                    },
                    _ => {
                        return false
                    },
                };

                true
            }
        })
        .expect("Couldn't parse");
    } else {
        input.block = syn::parse2(quote! {
            {
                #[link(name = "kernel32")]
                extern "system" {
                    fn DisableThreadLibraryCalls(hlibmodule: *mut std::ffi::c_void) -> i32;
                }

                unsafe {
                    DisableThreadLibraryCalls(module_handle);
                }

                match dw_reason {
                    1u32 => {
                        let mut rt = tokio::runtime::Runtime::new().unwrap();
                        rt.block_on(async move {
                            #original_body
                        });
                    },
                    _ => {
                        return false
                    },
                };

                true
            }
        })
        .expect("Couldn't parse");
    }
    input.block.brace_token = brace_token;

    // Only reason I decided to use quote! to parse this is because working with
    // input.sig.input is way more confusing than it should be
    // TODO: We probably want to make the type of the params of this function from
    //  our marauder library's types
    input.sig =
        syn::parse2(quote! {extern "system" fn DllMain(module_handle: *mut std::ffi::c_void, dw_reason: std::os::raw::c_ulong, lp_reserved: *mut std::ffi::c_void) -> bool})
            .unwrap();

    // If we really cared I think we could just append a Attribute to input.attr for
    // no_mangle
    let result = quote! {
        #[no_mangle]
        #[allow(unused_braces)]
        #input
    };

    Ok(result.into())
}
