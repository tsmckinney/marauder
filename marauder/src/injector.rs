use std::{
    ffi::CString,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use windows::Win32::System::{
    LibraryLoader::LOAD_WITH_ALTERED_SEARCH_PATH,
    Memory::{MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_READWRITE},
    Threading::INFINITE,
};

use crate::{
    error::Error,
    process::Process,
    windows::wrappers::{
        close_handle, create_remote_thread, get_exit_code_thread, get_module_handle, get_proc_address, read_process_memory,
        virtual_alloc_ex, virtual_free_ex, wait_for_single_object, write_process_memory, LPThreadStartRoutine, LPVOID,
    },
};

/// Several methods of loading our library into the target process
pub enum InjectionMethod {
    /// This is the typical method when safety is not really a concern
    LoadLibrary,
    /// `LoadLibraryEx` is just a extended version of `LoadLibrary` which isn't
    /// always detected by anti-cheats
    LoadLibraryEx,
    /// Unsupported. Manual mapping bypasses the normal Windows loader.
    ManualMap,
}

/// These are methods in which the injector will execute the code from the DLL
/// that is injected
pub enum CodeExecutionMethod {
    /// Creates a new thread on the target process which will have `DllMain`
    /// called
    CreateRemoteThread,
    /// Unsupported. Hijacking an existing thread changes unrelated process
    /// execution state.
    ThreadHijack,
}

/// Choices of what to do with PE headers after injection.
pub enum PECloaking {
    /// We will do nothing with PE headers
    Keep,
    /// PE headers will be erased
    Erase,
    /// PE headers will be scrambled with fake ones
    Fake,
}

pub struct Config {
    pub injection_method: InjectionMethod,
    pub execution_method: CodeExecutionMethod,
    pub cloak_thread: bool,
    pub randomize_file_name: bool,
    pub pe_cloaking: PECloaking,
}

impl Config {
    #[must_use]
    pub const fn load_library_ex() -> Self {
        Self {
            injection_method: InjectionMethod::LoadLibraryEx,
            execution_method: CodeExecutionMethod::CreateRemoteThread,
            cloak_thread: false,
            randomize_file_name: false,
            pe_cloaking: PECloaking::Keep,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            injection_method: InjectionMethod::LoadLibrary,
            execution_method: CodeExecutionMethod::CreateRemoteThread,
            cloak_thread: false,
            randomize_file_name: false,
            pe_cloaking: PECloaking::Keep,
        }
    }
}

pub struct Injector {
    config: Config,
}

#[repr(C)]
struct LoadLibraryExParams {
    load_library_ex: usize,
    path: usize,
    flags: u32,
    _padding: u32,
    result: usize,
}

impl Injector {
    #[must_use]
    pub const fn new(config: Config) -> Self {
        Self { config }
    }

    /// # Errors
    pub fn inject(&self, process_id: u32, dll_path: &str) -> Result<(), Error> {
        self.validate_config()?;

        let dll_path = self.prepare_dll_path(dll_path)?;
        let dll_path = dll_path.to_str().ok_or(Error::DllPath)?;
        let dll_path = CString::new(dll_path)?;
        let process = Process::open_all_access(process_id)?;
        let remote_path = RemoteAllocation::write(process.handle(), dll_path.as_bytes_with_nul(), PAGE_READWRITE)?;

        let result = match self.config.injection_method {
            InjectionMethod::LoadLibrary | InjectionMethod::LoadLibraryEx => {
                self.inject_with_load_library_ex(process.handle(), remote_path.address)
            },
            InjectionMethod::ManualMap => Err(Error::UnsupportedInjectorFeature("manual map")),
        };

        drop(remote_path);
        result
    }

    const fn validate_config(&self) -> Result<(), Error> {
        if matches!(self.config.injection_method, InjectionMethod::ManualMap) {
            return Err(Error::UnsupportedInjectorFeature("manual map"));
        }

        if self.config.cloak_thread {
            return Err(Error::UnsupportedInjectorFeature("cloak_thread"));
        }

        if !matches!(self.config.pe_cloaking, PECloaking::Keep) {
            return Err(Error::UnsupportedInjectorFeature("pe_cloaking"));
        }

        if matches!(self.config.execution_method, CodeExecutionMethod::ThreadHijack) {
            return Err(Error::UnsupportedInjectorFeature("thread hijack"));
        }

        Ok(())
    }

    fn prepare_dll_path(&self, dll_path: &str) -> Result<PathBuf, Error> {
        let dll_path = Path::new(dll_path);
        if !dll_path.exists() {
            return Err(Error::DllPath);
        }

        if self.config.randomize_file_name {
            randomized_dll_copy(dll_path)
        } else {
            dll_path.canonicalize().map_err(Into::into)
        }
    }

    fn inject_with_load_library_ex(
        &self,
        process_handle: crate::windows::wrappers::Handle,
        remote_path: LPVOID,
    ) -> Result<(), Error> {
        #[cfg(target_pointer_width = "64")]
        {
            let load_library_flags = match self.config.injection_method {
                InjectionMethod::LoadLibrary => 0,
                InjectionMethod::LoadLibraryEx => LOAD_WITH_ALTERED_SEARCH_PATH.0,
                InjectionMethod::ManualMap => unreachable!(),
            };
            inject_load_library_ex_x64(process_handle, remote_path, load_library_flags)
        }

        #[cfg(not(target_pointer_width = "64"))]
        {
            if matches!(self.config.injection_method, InjectionMethod::LoadLibraryEx) {
                return Err(Error::UnsupportedInjectorFeature("LoadLibraryEx on non-x64"));
            }

            inject_load_library(process_handle, remote_path)
        }
    }
}

struct RemoteAllocation {
    process: crate::windows::wrappers::Handle,
    address: LPVOID,
}

impl RemoteAllocation {
    fn new(
        process: crate::windows::wrappers::Handle,
        size: usize,
        protection: windows::Win32::System::Memory::PAGE_PROTECTION_FLAGS,
    ) -> Result<Self, Error> {
        let address = virtual_alloc_ex(process, None, size, MEM_RESERVE | MEM_COMMIT, protection)?;
        Ok(Self { process, address })
    }

    fn write(
        process: crate::windows::wrappers::Handle,
        bytes: &[u8],
        protection: windows::Win32::System::Memory::PAGE_PROTECTION_FLAGS,
    ) -> Result<Self, Error> {
        let allocation = Self::new(process, bytes.len(), protection)?;
        write_process_memory(process, allocation.address, bytes.as_ptr().cast(), bytes.len(), None)?;
        Ok(allocation)
    }
}

impl Drop for RemoteAllocation {
    fn drop(&mut self) { let _ = virtual_free_ex(self.process, self.address, 0, MEM_RELEASE); }
}

#[cfg(target_pointer_width = "64")]
fn inject_load_library_ex_x64(
    process_handle: crate::windows::wrappers::Handle,
    remote_path: LPVOID,
    flags: u32,
) -> Result<(), Error> {
    let load_library_ex_address = get_proc_address(get_module_handle("Kernel32.dll")?, "LoadLibraryExA")?;
    let params = LoadLibraryExParams {
        load_library_ex: load_library_ex_address,
        path: remote_path as usize,
        flags,
        _padding: 0,
        result: 0,
    };
    let remote_params = RemoteAllocation::write(
        process_handle,
        unsafe { std::slice::from_raw_parts((&raw const params).cast(), std::mem::size_of::<LoadLibraryExParams>()) },
        PAGE_READWRITE,
    )?;
    let remote_stub = RemoteAllocation::write(process_handle, LOAD_LIBRARY_EX_X64_STUB, PAGE_EXECUTE_READWRITE)?;

    let _exit_code = run_remote_thread(process_handle, remote_stub.address, Some(remote_params.address))?;

    let mut remote_result = LoadLibraryExParams {
        load_library_ex: 0,
        path: 0,
        flags: 0,
        _padding: 0,
        result: 0,
    };
    let mut bytes_read = 0;
    read_process_memory(
        process_handle,
        remote_params.address,
        (&raw mut remote_result).cast(),
        std::mem::size_of::<LoadLibraryExParams>(),
        &raw mut bytes_read,
    )?;

    if remote_result.result == 0 {
        Err(Error::InjectionFailed)
    } else {
        Ok(())
    }
}

#[cfg(not(target_pointer_width = "64"))]
fn inject_load_library(process_handle: crate::windows::wrappers::Handle, remote_path: LPVOID) -> Result<(), Error> {
    let load_library_address = get_proc_address(get_module_handle("Kernel32.dll")?, "LoadLibraryA")?;
    let exit_code = run_remote_thread(process_handle, load_library_address as LPVOID, Some(remote_path))?;
    if exit_code == 0 {
        Err(Error::InjectionFailed)
    } else {
        Ok(())
    }
}

fn run_remote_thread(
    process_handle: crate::windows::wrappers::Handle,
    start_address: LPVOID,
    parameter: Option<LPVOID>,
) -> Result<u32, Error> {
    let thread_handle = unsafe {
        let start_routine = std::mem::transmute::<LPVOID, LPThreadStartRoutine>(start_address);
        create_remote_thread(process_handle, None, 0, start_routine, parameter, 0, None)?
    };

    let wait_result = wait_for_single_object(thread_handle, INFINITE);
    let exit_code_result = wait_result.and_then(|_| get_exit_code_thread(thread_handle));
    let close_result = close_handle(thread_handle);

    match (exit_code_result, close_result) {
        (Ok(exit_code), Ok(())) => Ok(exit_code),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

fn randomized_dll_copy(path: &Path) -> Result<PathBuf, Error> {
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or("dll");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| std::io::Error::last_os_error())?
        .as_nanos();
    let file_name = format!("marauder-{now:x}.{extension}");
    let destination = std::env::temp_dir().join(file_name);
    std::fs::copy(path, &destination)?;
    destination.canonicalize().map_err(Into::into)
}

#[cfg(target_pointer_width = "64")]
const LOAD_LIBRARY_EX_X64_STUB: &[u8] = &[
    0x53, // push rbx
    0x48, 0x83, 0xEC, 0x20, // sub rsp, 0x20
    0x48, 0x89, 0xCB, // mov rbx, rcx
    0x48, 0x8B, 0x03, // mov rax, [rbx]
    0x44, 0x8B, 0x43, 0x10, // mov r8d, [rbx+0x10]
    0x48, 0x31, 0xD2, // xor rdx, rdx
    0x48, 0x8B, 0x4B, 0x08, // mov rcx, [rbx+0x08]
    0xFF, 0xD0, // call rax
    0x48, 0x89, 0x43, 0x18, // mov [rbx+0x18], rax
    0x48, 0x83, 0xC4, 0x20, // add rsp, 0x20
    0x5B, // pop rbx
    0xC3, // ret
];

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn default_config_is_supported() {
        let injector = Injector::new(Config::default());

        assert!(injector.validate_config().is_ok());
    }

    #[test]
    fn load_library_ex_config_is_supported() {
        let injector = Injector::new(Config::load_library_ex());

        assert!(injector.validate_config().is_ok());
    }

    #[test]
    fn unsupported_config_options_return_errors() {
        let injector = Injector::new(Config {
            injection_method: InjectionMethod::ManualMap,
            ..Config::default()
        });
        assert!(matches!(
            injector.validate_config(),
            Err(Error::UnsupportedInjectorFeature("manual map"))
        ));

        let injector = Injector::new(Config {
            execution_method: CodeExecutionMethod::ThreadHijack,
            ..Config::default()
        });
        assert!(matches!(
            injector.validate_config(),
            Err(Error::UnsupportedInjectorFeature("thread hijack"))
        ));

        let injector = Injector::new(Config {
            cloak_thread: true,
            ..Config::default()
        });
        assert!(matches!(
            injector.validate_config(),
            Err(Error::UnsupportedInjectorFeature("cloak_thread"))
        ));

        let injector = Injector::new(Config {
            pe_cloaking: PECloaking::Erase,
            ..Config::default()
        });
        assert!(matches!(
            injector.validate_config(),
            Err(Error::UnsupportedInjectorFeature("pe_cloaking"))
        ));
    }

    #[test]
    fn prepare_dll_path_requires_existing_file() {
        let injector = Injector::new(Config::default());

        assert!(matches!(
            injector.prepare_dll_path("definitely-not-a-real-dll-path.dll"),
            Err(Error::DllPath)
        ));
    }

    #[test]
    fn randomized_dll_copy_preserves_contents_with_new_name() {
        let source = test_file_path("source.dll");
        let mut file = std::fs::File::create(&source).expect("create test dll");
        file.write_all(b"not a real dll").expect("write test dll");

        let injector = Injector::new(Config {
            randomize_file_name: true,
            ..Config::default()
        });
        let randomized = injector
            .prepare_dll_path(source.to_str().expect("test path is valid unicode"))
            .expect("randomize dll path");

        assert_ne!(source.canonicalize().expect("canonical source"), randomized);
        assert_eq!(
            std::fs::read(&source).expect("read source"),
            std::fs::read(&randomized).expect("read randomized")
        );

        std::fs::remove_file(source).expect("remove source");
        std::fs::remove_file(randomized).expect("remove randomized");
    }

    fn test_file_path(file_name: &str) -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("marauder-test-{id}-{file_name}"))
    }
}
