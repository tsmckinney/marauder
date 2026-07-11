use std::{
    fmt,
    mem::{MaybeUninit, size_of, size_of_val},
};

use windows::Win32::System::{
    Diagnostics::ToolHelp::{TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS},
    Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
    },
    Threading::{PROCESS_ALL_ACCESS, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE},
};

use crate::{
    error::{Error, SnapshotKind, SnapshotStage},
    pattern::Pattern,
    windows::{
        utils::convert_windows_string,
        wrappers::{
            DWORD_PTR, Handle, LPVOID, MemoryBasicInformation, ModuleEntry32, PageProtectionFlags, PageType,
            ProcessAccessRights, ProcessEntry32, VirtualAllocationType, close_handle, create_tool_help32_snapshot,
            get_process_id, module32_first, module32_next, open_process, process32_first, process32_next,
            read_process_memory, virtual_alloc_ex, virtual_free_ex, virtual_protect_ex, virtual_query_ex,
            write_process_memory,
        },
    },
};

/// Owned handle for a Windows process.
///
/// This keeps process lifetime management in one place so higher-level modules
/// can be built around process, memory, and module objects instead of passing
/// raw handles through every API boundary.
pub struct Process {
    handle: Handle,
}

/// Common process access masks.
pub struct ProcessAccess;

impl ProcessAccess {
    #[must_use]
    pub const fn all() -> ProcessAccessRights {
        PROCESS_ALL_ACCESS
    }

    #[must_use]
    pub const fn query() -> ProcessAccessRights {
        PROCESS_QUERY_INFORMATION
    }

    #[must_use]
    pub fn read() -> ProcessAccessRights {
        PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
    }

    #[must_use]
    pub fn write() -> ProcessAccessRights {
        PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_WRITE
    }

    #[must_use]
    pub fn read_write() -> ProcessAccessRights {
        PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE
    }
}

/// Marker trait for values that can be safely copied to and from raw process
/// memory as bytes.
///
/// # Safety
/// Implement this only for types that are plain data: no references, no drop
/// implementation, no invalid bit patterns, and a stable `repr(C)` or
/// primitive layout when shared across a process boundary.
pub unsafe trait PlainOldData: Copy + 'static {}

macro_rules! impl_plain_old_data {
    ($($ty:ty),+ $(,)?) => {
        $(unsafe impl PlainOldData for $ty {})+
    };
}

impl_plain_old_data!(u8, u16, u32, u64, u128, usize);
impl_plain_old_data!(i8, i16, i32, i64, i128, isize);
impl_plain_old_data!(f32, f64);

unsafe impl<T: PlainOldData, const N: usize> PlainOldData for [T; N] {}

impl Process {
    #[must_use]
    pub fn current_id() -> u32 {
        std::process::id()
    }

    /// Opens the current process with explicit access rights.
    ///
    /// # Errors
    /// Returns an error if Windows refuses to open the current process.
    pub fn current(access: ProcessAccessRights) -> Result<Self, Error> {
        Self::open(Self::current_id(), access)
    }

    /// Opens a process with explicit access rights.
    ///
    /// # Errors
    /// Returns an error if Windows refuses to open the process.
    pub fn open(process_id: u32, access: ProcessAccessRights) -> Result<Self, Error> {
        let handle = open_process(access, false, process_id)?;
        Ok(Self { handle })
    }

    /// Opens the first process with a case-insensitive matching executable
    /// name.
    ///
    /// # Errors
    /// Returns an error if Windows cannot enumerate or open the process.
    pub fn open_by_name(process_name: &str, access: ProcessAccessRights) -> Result<Self, Error> {
        let process = Self::find_by_name(process_name)?.ok_or(Error::ProcessNotFound)?;
        process.open(access)
    }

    /// Opens a process with `PROCESS_ALL_ACCESS`.
    ///
    /// # Errors
    /// Returns an error if Windows refuses to open the process.
    pub fn open_all_access(process_id: u32) -> Result<Self, Error> {
        Self::open(process_id, ProcessAccess::all())
    }

    /// Lists processes visible through a `ToolHelp` snapshot.
    ///
    /// # Errors
    /// Returns an error if Windows cannot create or walk the process snapshot.
    pub fn list() -> Result<Vec<ProcessInfo>, Error> {
        let snapshot = Snapshot::new(TH32CS_SNAPPROCESS, 0, SnapshotKind::Process)?;
        let mut entry = ProcessEntry32 {
            dwSize: size_of::<ProcessEntry32>() as u32,
            ..ProcessEntry32::default()
        };
        let mut processes = Vec::new();

        process32_first(snapshot.handle, &mut entry)
            .map_err(|source| snapshot_error(SnapshotKind::Process, 0, SnapshotStage::FirstEntry, source))?;
        loop {
            processes.push(ProcessInfo::from_entry(&entry)?);

            if process32_next(snapshot.handle, &mut entry).is_err() {
                break;
            }
        }

        Ok(processes)
    }

    /// Finds the first process with a case-insensitive matching executable
    /// name.
    ///
    /// # Errors
    /// Returns an error if Windows cannot enumerate processes.
    pub fn find_by_name(process_name: &str) -> Result<Option<ProcessInfo>, Error> {
        Ok(Self::find_all_by_name(process_name)?.into_iter().next())
    }

    /// Finds all processes with a case-insensitive matching executable name.
    ///
    /// # Errors
    /// Returns an error if Windows cannot enumerate processes.
    pub fn find_all_by_name(process_name: &str) -> Result<Vec<ProcessInfo>, Error> {
        Ok(Self::list()?
            .into_iter()
            .filter(|process| process.name.eq_ignore_ascii_case(process_name))
            .collect())
    }

    /// Returns the process id for the owned handle.
    ///
    /// # Errors
    /// Returns an error if Windows cannot query the process id.
    pub fn id(&self) -> Result<u32, Error> {
        get_process_id(self.handle)
    }

    /// Returns snapshot metadata for the opened process.
    ///
    /// # Errors
    /// Returns an error if Windows cannot query the process id or enumerate
    /// processes.
    pub fn info(&self) -> Result<ProcessInfo, Error> {
        let process_id = self.id()?;
        Self::list()?
            .into_iter()
            .find(|process| process.id == process_id)
            .ok_or(Error::ProcessNotFound)
    }

    #[must_use]
    pub const fn memory(&self) -> ProcessMemory<'_> {
        ProcessMemory { process: self }
    }

    #[must_use]
    pub const fn modules(&self) -> ProcessModules<'_> {
        ProcessModules { process: self }
    }

    #[must_use]
    pub const fn handle(&self) -> Handle {
        self.handle
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = close_handle(self.handle);
    }
}

/// Memory operations scoped to a process handle.
pub struct ProcessMemory<'process> {
    process: &'process Process,
}

impl ProcessMemory<'_> {
    /// Queries memory information for the region containing `address`.
    ///
    /// # Errors
    /// Returns an error if Windows cannot query the address.
    pub fn query(&self, address: DWORD_PTR) -> Result<MemoryRegion, Error> {
        let mut info = MemoryBasicInformation::default();
        virtual_query_ex(
            self.process.handle,
            address as _,
            &raw mut info,
            size_of::<MemoryBasicInformation>(),
        )?;
        Ok(MemoryRegion::from_basic_info(&info))
    }

    /// Lists memory regions visible from address zero upward.
    ///
    /// # Errors
    /// Returns an error if the first `VirtualQueryEx` call fails. Later
    /// failures are treated as the end of the address space.
    pub fn regions(&self) -> Result<Vec<MemoryRegion>, Error> {
        let mut address = 0usize;
        let mut regions = Vec::new();

        loop {
            match self.query(address) {
                Ok(region) => {
                    let next_address = region.base_address.saturating_add(region.size);
                    if next_address <= address {
                        break;
                    }

                    regions.push(region);
                    address = next_address;
                },
                Err(error) if regions.is_empty() => return Err(error),
                Err(_) => break,
            }
        }

        Ok(regions)
    }

    /// Lists committed regions.
    ///
    /// # Errors
    /// Returns an error if region enumeration fails at the first address.
    pub fn committed_regions(&self) -> Result<Vec<MemoryRegion>, Error> {
        Ok(self.regions()?.into_iter().filter(MemoryRegion::is_committed).collect())
    }

    /// Lists committed regions that are readable and not guarded.
    ///
    /// # Errors
    /// Returns an error if region enumeration fails at the first address.
    pub fn readable_regions(&self) -> Result<Vec<MemoryRegion>, Error> {
        Ok(self.regions()?.into_iter().filter(MemoryRegion::is_readable).collect())
    }

    /// Lists committed regions that are writable and not guarded.
    ///
    /// # Errors
    /// Returns an error if region enumeration fails at the first address.
    pub fn writable_regions(&self) -> Result<Vec<MemoryRegion>, Error> {
        Ok(self.regions()?.into_iter().filter(MemoryRegion::is_writable).collect())
    }

    /// Lists committed regions that are executable and not guarded.
    ///
    /// # Errors
    /// Returns an error if region enumeration fails at the first address.
    pub fn executable_regions(&self) -> Result<Vec<MemoryRegion>, Error> {
        Ok(self.regions()?.into_iter().filter(MemoryRegion::is_executable).collect())
    }

    /// Reads `size` bytes from the process.
    ///
    /// # Errors
    /// Returns an error if Windows cannot read the full requested range.
    pub fn read(&self, address: DWORD_PTR, size: usize) -> Result<Vec<u8>, Error> {
        let mut bytes = vec![0; size];
        let mut bytes_read = 0;
        read_process_memory(
            self.process.handle,
            address as _,
            bytes.as_mut_ptr().cast(),
            size,
            &raw mut bytes_read,
        )?;

        if bytes_read != size {
            return Err(Error::PartialMemoryAccess {
                expected: size,
                actual: bytes_read,
            });
        }

        Ok(bytes)
    }

    /// Reads a bounded UTF-8 string from the process.
    ///
    /// The read stops at the first NUL byte or `max_len`, whichever comes
    /// first.
    ///
    /// # Errors
    /// Returns an error if Windows cannot read the full requested range or if
    /// the bytes before the first NUL are not valid UTF-8.
    pub fn read_string(&self, address: DWORD_PTR, max_len: usize) -> Result<String, Error> {
        let bytes = self.read_string_bytes(address, max_len)?;
        Ok(std::str::from_utf8(&bytes)?.to_owned())
    }

    /// Reads a bounded string from the process, replacing invalid UTF-8.
    ///
    /// The read stops at the first NUL byte or `max_len`, whichever comes
    /// first.
    ///
    /// # Errors
    /// Returns an error if Windows cannot read the full requested range.
    pub fn read_string_lossy(&self, address: DWORD_PTR, max_len: usize) -> Result<String, Error> {
        Ok(String::from_utf8_lossy(&self.read_string_bytes(address, max_len)?).into_owned())
    }

    fn read_string_bytes(&self, address: DWORD_PTR, max_len: usize) -> Result<Vec<u8>, Error> {
        let mut bytes = self.read(address, max_len)?;
        if let Some(nul) = bytes.iter().position(|byte| *byte == 0) {
            bytes.truncate(nul);
        }
        Ok(bytes)
    }

    /// Scans a specific memory range for a pattern.
    ///
    /// # Errors
    /// Returns an error if Windows cannot read the full requested range.
    pub fn scan_range(&self, range: MemoryRange, pattern: &Pattern) -> Result<Vec<PatternMatch>, Error> {
        let bytes = self.read(range.base_address, range.size)?;
        Ok(pattern
            .find_all_in(&bytes)
            .into_iter()
            .map(|offset| PatternMatch {
                address: range.base_address + offset,
                offset,
                range,
            })
            .collect())
    }

    /// Scans a readable memory region for a pattern.
    ///
    /// # Errors
    /// Returns an error if Windows cannot read the full region.
    pub fn scan_region(&self, region: &MemoryRegion, pattern: &Pattern) -> Result<Vec<PatternMatch>, Error> {
        self.scan_range(MemoryRange::from_region(region), pattern)
    }

    /// Scans all readable regions for a pattern.
    ///
    /// # Errors
    /// Returns an error if region enumeration or a region read fails.
    pub fn scan_readable(&self, pattern: &Pattern) -> Result<Vec<PatternMatch>, Error> {
        let mut matches = Vec::new();
        for region in self.readable_regions()? {
            matches.extend(self.scan_region(&region, pattern)?);
        }
        Ok(matches)
    }

    /// Scans readable ranges that intersect a module.
    ///
    /// # Errors
    /// Returns an error if region enumeration or a range read fails.
    pub fn scan_module(&self, module: &ModuleInfo, pattern: &Pattern) -> Result<Vec<PatternMatch>, Error> {
        let mut matches = Vec::new();
        for region in self.readable_regions()? {
            if let Some(range) = module.intersection(&region) {
                matches.extend(self.scan_range(range, pattern)?);
            }
        }
        Ok(matches)
    }

    /// Reads a `Copy` value from the process.
    ///
    /// # Errors
    /// Returns an error if Windows cannot read enough bytes for `T`.
    pub fn read_value<T: Copy>(&self, address: DWORD_PTR) -> Result<T, Error> {
        let mut value = MaybeUninit::<T>::uninit();
        let mut bytes_read = 0;
        read_process_memory(
            self.process.handle,
            address as _,
            value.as_mut_ptr().cast(),
            size_of::<T>(),
            &raw mut bytes_read,
        )?;

        if bytes_read != size_of::<T>() {
            return Err(Error::PartialMemoryAccess {
                expected: size_of::<T>(),
                actual: bytes_read,
            });
        }

        Ok(unsafe { value.assume_init() })
    }

    /// Reads a plain-old-data value from the process.
    ///
    /// Use this when the target type explicitly supports raw byte
    /// reinterpretation. For user-defined structs, prefer `#[repr(C)]` and an
    /// explicit `unsafe impl PlainOldData`.
    ///
    /// # Errors
    /// Returns an error if Windows cannot read enough bytes for `T`.
    pub fn read_pod<T: PlainOldData>(&self, address: DWORD_PTR) -> Result<T, Error> {
        self.read_value(address)
    }

    /// Reads `count` plain-old-data values from the process.
    ///
    /// # Errors
    /// Returns an error if Windows cannot read the full requested range.
    pub fn read_array<T: PlainOldData>(&self, address: DWORD_PTR, count: usize) -> Result<Vec<T>, Error> {
        let byte_len = size_of::<T>().checked_mul(count).ok_or(Error::MemorySizeOverflow)?;
        let mut values = vec![unsafe { MaybeUninit::<T>::zeroed().assume_init() }; count];
        let mut bytes_read = 0;

        read_process_memory(
            self.process.handle,
            address as _,
            values.as_mut_ptr().cast(),
            byte_len,
            &raw mut bytes_read,
        )?;

        if bytes_read != byte_len {
            return Err(Error::PartialMemoryAccess {
                expected: byte_len,
                actual: bytes_read,
            });
        }

        Ok(values)
    }

    /// Writes bytes into the process.
    ///
    /// # Errors
    /// Returns an error if Windows cannot write the full requested range.
    pub fn write(&self, address: DWORD_PTR, bytes: &[u8]) -> Result<(), Error> {
        let mut bytes_written = 0;
        write_process_memory(
            self.process.handle,
            address as _,
            bytes.as_ptr().cast(),
            bytes.len(),
            Some(&raw mut bytes_written),
        )?;

        if bytes_written != bytes.len() {
            return Err(Error::PartialMemoryAccess {
                expected: bytes.len(),
                actual: bytes_written,
            });
        }

        Ok(())
    }

    /// Writes UTF-8 string bytes into the process without appending a NUL byte.
    ///
    /// # Errors
    /// Returns an error if Windows cannot write the full requested range.
    pub fn write_string(&self, address: DWORD_PTR, value: &str) -> Result<(), Error> {
        self.write(address, value.as_bytes())
    }

    /// Writes UTF-8 string bytes followed by a NUL byte into the process.
    ///
    /// # Errors
    /// Returns an error if `value` contains an interior NUL byte or if Windows
    /// cannot write the full requested range.
    pub fn write_c_string(&self, address: DWORD_PTR, value: &str) -> Result<(), Error> {
        if value.as_bytes().contains(&0) {
            return Err(Error::InteriorNul);
        }

        let mut bytes = Vec::with_capacity(value.len() + 1);
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
        self.write(address, &bytes)
    }

    /// Writes a `Copy` value into the process.
    ///
    /// # Errors
    /// Returns an error if Windows cannot write enough bytes for `T`.
    pub fn write_value<T: Copy>(&self, address: DWORD_PTR, value: &T) -> Result<(), Error> {
        let mut bytes_written = 0;
        write_process_memory(
            self.process.handle,
            address as _,
            std::ptr::from_ref(value).cast(),
            size_of::<T>(),
            Some(&raw mut bytes_written),
        )?;

        if bytes_written != size_of::<T>() {
            return Err(Error::PartialMemoryAccess {
                expected: size_of::<T>(),
                actual: bytes_written,
            });
        }

        Ok(())
    }

    /// Writes a plain-old-data value into the process.
    ///
    /// # Errors
    /// Returns an error if Windows cannot write enough bytes for `T`.
    pub fn write_pod<T: PlainOldData>(&self, address: DWORD_PTR, value: &T) -> Result<(), Error> {
        self.write_value(address, value)
    }

    /// Writes plain-old-data values into the process.
    ///
    /// # Errors
    /// Returns an error if Windows cannot write the full requested range.
    pub fn write_array<T: PlainOldData>(&self, address: DWORD_PTR, values: &[T]) -> Result<(), Error> {
        self.write(address, plain_old_data_slice_as_bytes(values))
    }

    /// Allocates memory in the process with read/write protection.
    ///
    /// # Errors
    /// Returns an error if Windows cannot allocate the memory.
    pub fn allocate_read_write(&self, size: usize) -> Result<ProcessAllocation, Error> {
        self.allocate(size, PAGE_READWRITE)
    }

    /// Allocates memory in the process.
    ///
    /// # Errors
    /// Returns an error if Windows cannot allocate the memory.
    pub fn allocate(&self, size: usize, protection: PageProtectionFlags) -> Result<ProcessAllocation, Error> {
        let address = virtual_alloc_ex(self.process.handle, None, size, MEM_RESERVE | MEM_COMMIT, protection)?;
        Ok(ProcessAllocation {
            process: self.process.handle,
            address,
            size,
        })
    }

    /// Changes protection for a memory range and returns the previous flags.
    ///
    /// # Errors
    /// Returns an error if Windows cannot change protection for the range.
    pub fn protect(
        &self,
        address: DWORD_PTR,
        size: usize,
        protection: PageProtectionFlags,
    ) -> Result<PageProtectionFlags, Error> {
        let mut old_protection = PageProtectionFlags::default();
        virtual_protect_ex(self.process.handle, address as _, size, protection, &raw mut old_protection)?;
        Ok(old_protection)
    }

    /// Changes protection for a memory range until the returned guard is
    /// dropped.
    ///
    /// # Errors
    /// Returns an error if Windows cannot change protection for the range.
    pub fn protect_scoped(
        &self,
        address: DWORD_PTR,
        size: usize,
        protection: PageProtectionFlags,
    ) -> Result<ProcessProtectionGuard, Error> {
        let old_protection = self.protect(address, size, protection)?;
        Ok(ProcessProtectionGuard {
            process: self.process.handle,
            address,
            size,
            old_protection,
            restored: false,
        })
    }
}

/// Owned allocation in a process.
pub struct ProcessAllocation {
    process: Handle,
    address: LPVOID,
    size: usize,
}

impl ProcessAllocation {
    #[must_use]
    pub const fn address(&self) -> LPVOID {
        self.address
    }

    #[must_use]
    pub fn address_usize(&self) -> DWORD_PTR {
        self.address as DWORD_PTR
    }

    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    #[must_use]
    pub fn range(&self) -> MemoryRange {
        MemoryRange {
            base_address: self.address_usize(),
            size: self.size,
        }
    }

    #[must_use]
    pub fn end_address(&self) -> DWORD_PTR {
        self.range().end_address()
    }

    #[must_use]
    pub fn contains(&self, address: DWORD_PTR, size: usize) -> bool {
        self.range().contains(address, size)
    }

    /// Releases this allocation before drop.
    ///
    /// # Errors
    /// Returns an error if Windows cannot free the allocation.
    pub fn free(self) -> Result<(), Error> {
        let process = self.process;
        let address = self.address;
        std::mem::forget(self);
        virtual_free_ex(process, address, 0, MEM_RELEASE)
    }
}

impl fmt::Display for ProcessAllocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "0x{:x}..0x{:x} size=0x{:x}",
            self.address_usize(),
            self.end_address(),
            self.size
        )
    }
}

impl Drop for ProcessAllocation {
    fn drop(&mut self) {
        let _ = virtual_free_ex(self.process, self.address, 0, MEM_RELEASE);
    }
}

/// Scoped memory protection change for a process range.
pub struct ProcessProtectionGuard {
    process: Handle,
    address: DWORD_PTR,
    size: usize,
    old_protection: PageProtectionFlags,
    restored: bool,
}

impl ProcessProtectionGuard {
    #[must_use]
    pub const fn address(&self) -> DWORD_PTR {
        self.address
    }

    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    #[must_use]
    pub const fn old_protection(&self) -> PageProtectionFlags {
        self.old_protection
    }

    /// Restores the previous protection before drop.
    ///
    /// # Errors
    /// Returns an error if Windows cannot restore protection for the range.
    pub fn restore(mut self) -> Result<(), Error> {
        self.restore_inner()?;
        Ok(())
    }

    fn restore_inner(&mut self) -> Result<(), Error> {
        if self.restored {
            return Ok(());
        }

        let mut ignored = PageProtectionFlags::default();
        virtual_protect_ex(
            self.process,
            self.address as _,
            self.size,
            self.old_protection,
            &raw mut ignored,
        )?;
        self.restored = true;
        Ok(())
    }
}

impl Drop for ProcessProtectionGuard {
    fn drop(&mut self) {
        let _ = self.restore_inner();
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct MemoryRegion {
    pub base_address: DWORD_PTR,
    pub allocation_base: DWORD_PTR,
    pub allocation_protect: PageProtectionFlags,
    pub size: usize,
    pub state: VirtualAllocationType,
    pub protect: PageProtectionFlags,
    pub kind: PageType,
}

impl MemoryRegion {
    fn from_basic_info(info: &MemoryBasicInformation) -> Self {
        Self {
            base_address: info.BaseAddress as DWORD_PTR,
            allocation_base: info.AllocationBase as DWORD_PTR,
            allocation_protect: info.AllocationProtect,
            size: info.RegionSize,
            state: info.State,
            protect: info.Protect,
            kind: info.Type,
        }
    }

    #[must_use]
    pub fn is_committed(&self) -> bool {
        self.state == MEM_COMMIT
    }

    #[must_use]
    pub const fn is_guarded(&self) -> bool {
        self.has_protection(PAGE_GUARD)
    }

    #[must_use]
    pub const fn is_no_access(&self) -> bool {
        self.has_protection(PAGE_NOACCESS)
    }

    #[must_use]
    pub fn is_readable(&self) -> bool {
        self.is_committed()
            && !self.is_guarded()
            && !self.is_no_access()
            && self.has_any_protection(&[
                PAGE_READONLY,
                PAGE_READWRITE,
                PAGE_WRITECOPY,
                PAGE_EXECUTE_READ,
                PAGE_EXECUTE_READWRITE,
                PAGE_EXECUTE_WRITECOPY,
            ])
    }

    #[must_use]
    pub fn is_writable(&self) -> bool {
        self.is_committed()
            && !self.is_guarded()
            && !self.is_no_access()
            && self.has_any_protection(&[PAGE_READWRITE, PAGE_WRITECOPY, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY])
    }

    #[must_use]
    pub fn is_executable(&self) -> bool {
        self.is_committed()
            && !self.is_guarded()
            && !self.is_no_access()
            && self.has_any_protection(&[
                PAGE_EXECUTE,
                PAGE_EXECUTE_READ,
                PAGE_EXECUTE_READWRITE,
                PAGE_EXECUTE_WRITECOPY,
            ])
    }

    #[must_use]
    pub fn access(&self) -> MemoryAccess {
        MemoryAccess::from_region(self)
    }

    #[must_use]
    pub const fn range(&self) -> MemoryRange {
        MemoryRange::from_region(self)
    }

    #[must_use]
    pub const fn end_address(&self) -> DWORD_PTR {
        self.base_address.saturating_add(self.size)
    }

    #[must_use]
    pub const fn contains(&self, address: DWORD_PTR, size: usize) -> bool {
        address >= self.base_address && address.saturating_add(size) <= self.end_address()
    }

    fn has_any_protection(&self, flags: &[PageProtectionFlags]) -> bool {
        flags.iter().any(|flag| self.has_protection(*flag))
    }

    const fn has_protection(&self, flag: PageProtectionFlags) -> bool {
        self.protect.0 & flag.0 == flag.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct MemoryAccess {
    bits: u8,
}

impl MemoryAccess {
    const READ: u8 = 0b0_0001;
    const WRITE: u8 = 0b0_0010;
    const EXECUTE: u8 = 0b0_0100;
    const GUARD: u8 = 0b0_1000;
    const NO_ACCESS: u8 = 0b1_0000;

    fn from_region(region: &MemoryRegion) -> Self {
        let mut bits = 0;
        if region.is_readable() {
            bits |= Self::READ;
        }
        if region.is_writable() {
            bits |= Self::WRITE;
        }
        if region.is_executable() {
            bits |= Self::EXECUTE;
        }
        if region.is_guarded() {
            bits |= Self::GUARD;
        }
        if region.is_no_access() {
            bits |= Self::NO_ACCESS;
        }
        Self { bits }
    }

    #[must_use]
    pub const fn is_readable(self) -> bool {
        self.bits & Self::READ == Self::READ
    }

    #[must_use]
    pub const fn is_writable(self) -> bool {
        self.bits & Self::WRITE == Self::WRITE
    }

    #[must_use]
    pub const fn is_executable(self) -> bool {
        self.bits & Self::EXECUTE == Self::EXECUTE
    }

    #[must_use]
    pub const fn is_guarded(self) -> bool {
        self.bits & Self::GUARD == Self::GUARD
    }

    #[must_use]
    pub const fn is_no_access(self) -> bool {
        self.bits & Self::NO_ACCESS == Self::NO_ACCESS
    }
}

impl fmt::Display for MemoryAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_no_access() {
            f.write_str("no-access")?;
        } else {
            f.write_str(if self.is_readable() { "r" } else { "-" })?;
            f.write_str(if self.is_writable() { "w" } else { "-" })?;
            f.write_str(if self.is_executable() { "x" } else { "-" })?;
        }

        if self.is_guarded() {
            f.write_str(",guard")?;
        }

        Ok(())
    }
}

/// Module inspection scoped to a process.
pub struct ProcessModules<'process> {
    process: &'process Process,
}

impl ProcessModules<'_> {
    /// Lists modules visible through a `ToolHelp` snapshot.
    ///
    /// # Errors
    /// Returns an error if Windows cannot create or walk the snapshot.
    pub fn list(&self) -> Result<Vec<ModuleInfo>, Error> {
        let process_id = self.process.id()?;
        let snapshot = Snapshot::new(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, process_id, SnapshotKind::Module)?;
        let mut entry = ModuleEntry32 {
            dwSize: size_of::<ModuleEntry32>() as u32,
            ..ModuleEntry32::default()
        };
        let mut modules = Vec::new();

        module32_first(snapshot.handle, &mut entry)
            .map_err(|source| snapshot_error(SnapshotKind::Module, process_id, SnapshotStage::FirstEntry, source))?;
        loop {
            modules.push(ModuleInfo::from_entry(&entry)?);

            if module32_next(snapshot.handle, &mut entry).is_err() {
                break;
            }
        }

        Ok(modules)
    }

    /// Returns the process executable module.
    ///
    /// `ToolHelp` module snapshots report the executable image as the first
    /// module. This keeps examples from rediscovering it through filesystem
    /// path comparisons.
    ///
    /// # Errors
    /// Returns an error if Windows cannot enumerate modules.
    pub fn main(&self) -> Result<ModuleInfo, Error> {
        self.list()?.into_iter().next().ok_or(Error::ProcessNotFound)
    }

    /// Finds the first module with a case-insensitive matching name.
    ///
    /// # Errors
    /// Returns an error if Windows cannot enumerate modules.
    pub fn find(&self, name: &str) -> Result<Option<ModuleInfo>, Error> {
        Ok(self.list()?.into_iter().find(|module| module.name.eq_ignore_ascii_case(name)))
    }

    /// Finds the first module with a case-insensitive matching path.
    ///
    /// # Errors
    /// Returns an error if Windows cannot enumerate modules.
    pub fn find_by_path(&self, path: &str) -> Result<Option<ModuleInfo>, Error> {
        Ok(self.list()?.into_iter().find(|module| module.path.eq_ignore_ascii_case(path)))
    }

    /// Finds the module containing an address.
    ///
    /// # Errors
    /// Returns an error if Windows cannot enumerate modules.
    pub fn find_by_address(&self, address: DWORD_PTR) -> Result<Option<ModuleInfo>, Error> {
        Ok(self.list()?.into_iter().find(|module| module.contains(address, 1)))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProcessInfo {
    pub id: u32,
    pub parent_id: u32,
    pub thread_count: u32,
    pub base_priority: i32,
    pub name: String,
}

impl ProcessInfo {
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.id == Process::current_id()
    }

    /// Opens this process snapshot entry with explicit access rights.
    ///
    /// # Errors
    /// Returns an error if Windows refuses to open the process.
    pub fn open(&self, access: ProcessAccessRights) -> Result<Process, Error> {
        Process::open(self.id, access)
    }

    /// Opens this process snapshot entry with `PROCESS_ALL_ACCESS`.
    ///
    /// # Errors
    /// Returns an error if Windows refuses to open the process.
    pub fn open_all_access(&self) -> Result<Process, Error> {
        self.open(ProcessAccess::all())
    }

    fn from_entry(entry: &ProcessEntry32) -> Result<Self, Error> {
        Ok(Self {
            id: entry.th32ProcessID,
            parent_id: entry.th32ParentProcessID,
            thread_count: entry.cntThreads,
            base_priority: entry.pcPriClassBase,
            name: convert_windows_string(entry.szExeFile)?.to_owned(),
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ModuleInfo {
    pub name: String,
    pub path: String,
    pub base_address: DWORD_PTR,
    pub size: usize,
}

impl ModuleInfo {
    #[must_use]
    pub const fn range(&self) -> MemoryRange {
        MemoryRange {
            base_address: self.base_address,
            size: self.size,
        }
    }

    #[must_use]
    pub const fn end_address(&self) -> DWORD_PTR {
        self.base_address.saturating_add(self.size)
    }

    #[must_use]
    pub const fn contains(&self, address: DWORD_PTR, size: usize) -> bool {
        address >= self.base_address && address.saturating_add(size) <= self.end_address()
    }

    #[must_use]
    pub fn intersection(&self, region: &MemoryRegion) -> Option<MemoryRange> {
        let start = region.base_address.max(self.base_address);
        let end = region.end_address().min(self.end_address());
        if start < end {
            Some(MemoryRange {
                base_address: start,
                size: end - start,
            })
        } else {
            None
        }
    }

    fn from_entry(entry: &ModuleEntry32) -> Result<Self, Error> {
        Ok(Self {
            name: convert_windows_string(entry.szModule)?.to_owned(),
            path: convert_windows_string(entry.szExePath)?.to_owned(),
            base_address: entry.modBaseAddr as DWORD_PTR,
            size: entry.modBaseSize as usize,
        })
    }
}

impl fmt::Display for ModuleInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} 0x{:x}..0x{:x} size=0x{:x}",
            self.name,
            self.base_address,
            self.end_address(),
            self.size
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct MemoryRange {
    pub base_address: DWORD_PTR,
    pub size: usize,
}

impl MemoryRange {
    #[must_use]
    pub const fn from_region(region: &MemoryRegion) -> Self {
        Self {
            base_address: region.base_address,
            size: region.size,
        }
    }

    #[must_use]
    pub const fn end_address(&self) -> DWORD_PTR {
        self.base_address.saturating_add(self.size)
    }

    #[must_use]
    pub const fn contains(&self, address: DWORD_PTR, size: usize) -> bool {
        address >= self.base_address && address.saturating_add(size) <= self.end_address()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct PatternMatch {
    pub address: DWORD_PTR,
    pub offset: usize,
    pub range: MemoryRange,
}

struct Snapshot {
    handle: Handle,
}

impl Snapshot {
    fn new(
        flags: windows::Win32::System::Diagnostics::ToolHelp::CREATE_TOOLHELP_SNAPSHOT_FLAGS,
        process_id: u32,
        kind: SnapshotKind,
    ) -> Result<Self, Error> {
        Ok(Self {
            handle: create_tool_help32_snapshot(flags, process_id)
                .map_err(|source| snapshot_error(kind, process_id, SnapshotStage::Create, source))?,
        })
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = close_handle(self.handle);
    }
}

fn snapshot_error(kind: SnapshotKind, process_id: u32, stage: SnapshotStage, source: Error) -> Error {
    Error::Snapshot {
        kind,
        process_id,
        stage,
        source: Box::new(source),
    }
}

const fn plain_old_data_slice_as_bytes<T: PlainOldData>(values: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), size_of_val(values)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_reports_its_id() {
        let process = Process::current(ProcessAccess::query()).expect("open current process");

        assert_eq!(Process::current_id(), std::process::id());
        assert_eq!(process.id().expect("query process id"), Process::current_id());
    }

    #[test]
    fn current_process_reports_snapshot_info() {
        let process = Process::current(ProcessAccess::query()).expect("open current process");
        let info = process.info().expect("query process info");

        assert_eq!(info.id, Process::current_id());
        assert!(info.is_current());
        assert!(!info.name.is_empty());
    }

    #[test]
    fn can_find_and_open_current_process_by_name() {
        let current_name = std::env::current_exe()
            .expect("current executable path")
            .file_name()
            .expect("current executable file name")
            .to_string_lossy()
            .into_owned();
        let process_info = Process::find_by_name(&current_name)
            .expect("find current process")
            .expect("current process should be listed");
        let process_from_info = process_info.open(ProcessAccess::query()).expect("open current process info");
        let process = Process::open_by_name(&current_name, ProcessAccess::query()).expect("open current process by name");

        assert_eq!(process_info.id, Process::current_id());
        assert_eq!(
            process_from_info.id().expect("query process info opened id"),
            Process::current_id()
        );
        assert_eq!(process.id().expect("query opened process id"), Process::current_id());
    }

    #[test]
    fn can_find_all_matching_processes_by_name() {
        let current_name = std::env::current_exe()
            .expect("current executable path")
            .file_name()
            .expect("current executable file name")
            .to_string_lossy()
            .into_owned();
        let matches = Process::find_all_by_name(&current_name).expect("find matching processes");

        assert!(matches.iter().any(|process| process.id == Process::current_id()));
        assert!(matches.iter().all(|process| process.name.eq_ignore_ascii_case(&current_name)));
    }

    #[test]
    fn current_process_memory_round_trip() {
        let process = Process::current(ProcessAccess::read_write()).expect("open current process");
        let allocation = process.memory().allocate_read_write(16).expect("allocate memory");

        process
            .memory()
            .write(allocation.address_usize(), b"marauder")
            .expect("write memory");
        let bytes = process.memory().read(allocation.address_usize(), 8).expect("read memory");

        assert_eq!(bytes, b"marauder");
    }

    #[test]
    fn current_process_bounded_string_reads_stop_at_nul_or_limit() {
        let process = Process::current(ProcessAccess::read_write()).expect("open current process");
        let allocation = process.memory().allocate_read_write(32).expect("allocate memory");

        process
            .memory()
            .write_c_string(allocation.address_usize(), "marauder")
            .expect("write C string");

        assert_eq!(
            process
                .memory()
                .read_string(allocation.address_usize(), 16)
                .expect("read string"),
            "marauder"
        );
        assert_eq!(
            process
                .memory()
                .read_string(allocation.address_usize(), 4)
                .expect("read limited string"),
            "mara"
        );

        process
            .memory()
            .write_string(allocation.address_usize(), "rust")
            .expect("write raw string");
        assert_eq!(
            process
                .memory()
                .read_string(allocation.address_usize(), 4)
                .expect("read raw string"),
            "rust"
        );
        assert!(matches!(
            process.memory().write_c_string(allocation.address_usize(), "bad\0value"),
            Err(Error::InteriorNul)
        ));

        process
            .memory()
            .write(allocation.address_usize(), &[0xff, b'o', b'k', 0])
            .expect("write invalid string");
        assert!(process.memory().read_string(allocation.address_usize(), 4).is_err());
        assert_eq!(
            process
                .memory()
                .read_string_lossy(allocation.address_usize(), 4)
                .expect("read lossy string"),
            "\u{fffd}ok"
        );
    }

    #[test]
    fn current_process_scan_region_finds_pattern() {
        let process = Process::current(ProcessAccess::read_write()).expect("open current process");
        let allocation = process.memory().allocate_read_write(0x1000).expect("allocate memory");
        let marker = b"marauder-process-scan";
        let marker_address = allocation.address_usize() + 0x80;
        let pattern = Pattern::exact(marker).expect("marker pattern");

        process.memory().write(marker_address, marker).expect("write marker");
        let region = process.memory().query(marker_address).expect("query marker region");
        let matches = process.memory().scan_region(&region, &pattern).expect("scan marker region");

        assert!(matches.iter().any(|match_| match_.address == marker_address));
        assert!(
            matches
                .iter()
                .all(|match_| match_.range.contains(match_.address, pattern.len()))
        );
    }

    #[test]
    fn current_process_typed_memory_round_trip() {
        let process = Process::current(ProcessAccess::read_write()).expect("open current process");
        let allocation = process
            .memory()
            .allocate_read_write(size_of::<u32>())
            .expect("allocate memory");

        process
            .memory()
            .write_value(allocation.address_usize(), &0xfeed_beefu32)
            .expect("write value");
        let value = process
            .memory()
            .read_value::<u32>(allocation.address_usize())
            .expect("read value");

        assert_eq!(value, 0xfeed_beef);
    }

    #[test]
    fn current_process_plain_old_data_array_round_trip() {
        let process = Process::current(ProcessAccess::read_write()).expect("open current process");
        let values = [0x10_u32, 0x20, 0x30, 0x40];
        let allocation = process
            .memory()
            .allocate_read_write(size_of_val(&values))
            .expect("allocate memory");

        process
            .memory()
            .write_array(allocation.address_usize(), &values)
            .expect("write POD array");
        let read_back = process
            .memory()
            .read_array::<u32>(allocation.address_usize(), values.len())
            .expect("read POD array");

        assert_eq!(read_back, values);
    }

    #[test]
    fn current_process_queries_allocated_region() {
        let process = Process::current(ProcessAccess::read_write()).expect("open current process");
        let allocation = process.memory().allocate_read_write(16).expect("allocate memory");
        let region = process
            .memory()
            .query(allocation.address_usize())
            .expect("query allocation region");

        assert!(region.base_address <= allocation.address_usize());
        assert!(region.base_address + region.size >= allocation.address_usize() + allocation.size());
        assert!(region.is_committed());
        assert!(region.is_readable());
        assert!(region.is_writable());
        assert!(region.contains(allocation.address_usize(), allocation.size()));
        assert!(region.access().is_readable());
        assert!(region.access().is_writable());
        assert!(!region.access().is_executable());
        assert_eq!(region.access().to_string(), "rw-");
        assert!(region.range().contains(allocation.address_usize(), allocation.size()));
        assert_eq!(allocation.range().base_address, allocation.address_usize());
        assert_eq!(allocation.range().size, allocation.size());
        assert_eq!(allocation.end_address(), allocation.address_usize() + allocation.size());
        assert!(allocation.contains(allocation.address_usize(), allocation.size()));
        assert!(
            allocation
                .to_string()
                .contains(&format!("0x{:x}", allocation.address_usize()))
        );
    }

    #[test]
    fn current_process_scoped_protection_restores_previous_flags() {
        let process = Process::current(ProcessAccess::read_write()).expect("open current process");
        let allocation = process.memory().allocate_read_write(0x1000).expect("allocate memory");
        let address = allocation.address_usize();

        let guard = process
            .memory()
            .protect_scoped(address, allocation.size(), PAGE_READONLY)
            .expect("protect allocation as read-only");
        let read_only = process.memory().query(address).expect("query read-only region");

        assert_eq!(guard.address(), address);
        assert_eq!(guard.size(), allocation.size());
        assert!(read_only.is_readable());
        assert!(!read_only.is_writable());

        guard.restore().expect("restore previous protection");
        let restored = process.memory().query(address).expect("query restored region");

        assert!(restored.is_writable());
    }

    #[test]
    fn current_process_has_modules() {
        let process = Process::current(ProcessAccess::query()).expect("open current process");
        let modules = process.modules().list().expect("list modules");

        assert!(!modules.is_empty());
        assert!(modules.iter().any(|module| module.size > 0));
    }

    #[test]
    fn current_process_reports_main_module() {
        let process = Process::current(ProcessAccess::query()).expect("open current process");
        let main = process.modules().main().expect("query main module");
        let current_name = std::env::current_exe()
            .expect("current executable path")
            .file_name()
            .expect("current executable file name")
            .to_string_lossy()
            .into_owned();

        assert!(main.name.eq_ignore_ascii_case(&current_name));
        assert!(main.contains(current_process_reports_main_module as usize, 1));
    }

    #[test]
    fn current_process_can_find_module_by_address() {
        let process = Process::current(ProcessAccess::query()).expect("open current process");
        let address = current_process_can_find_module_by_address as usize;
        let module = process
            .modules()
            .find_by_address(address)
            .expect("find module by address")
            .expect("test function should belong to a module");

        assert!(module.contains(address, 1));
    }

    #[test]
    fn current_process_can_find_module_by_path() {
        let process = Process::current(ProcessAccess::query()).expect("open current process");
        let current_path = std::env::current_exe()
            .expect("current executable path")
            .to_string_lossy()
            .into_owned();
        let module = process
            .modules()
            .find_by_path(&current_path)
            .expect("find module by path")
            .expect("current executable should be a module");

        assert!(module.path.eq_ignore_ascii_case(&current_path));
    }

    #[test]
    fn current_process_module_display_includes_range() {
        let process = Process::current(ProcessAccess::query()).expect("open current process");
        let module = process.modules().main().expect("query main module");
        let display = module.to_string();

        assert_eq!(module.range().base_address, module.base_address);
        assert_eq!(module.range().size, module.size);
        assert!(display.contains(&module.name));
        assert!(display.contains(&format!("0x{:x}", module.base_address)));
        assert!(display.contains(&format!("0x{:x}", module.end_address())));
    }

    #[test]
    fn current_process_module_ranges_intersect_memory_regions() {
        let process = Process::current(ProcessAccess::query()).expect("open current process");
        let module = process
            .modules()
            .list()
            .expect("list modules")
            .into_iter()
            .find(|module| module.size > 0)
            .expect("process should have a sized module");
        let region = process.memory().query(module.base_address).expect("query module base region");
        let intersection = module.intersection(&region).expect("module should intersect its base region");

        assert!(module.end_address() > module.base_address);
        assert!(module.contains(module.base_address, 1));
        assert!(intersection.contains(module.base_address, 1));
    }
}
