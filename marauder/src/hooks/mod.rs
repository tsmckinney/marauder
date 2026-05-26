use std::ffi::c_void;

use windows::Win32::System::{
    Diagnostics::Debug::FlushInstructionCache,
    Memory::{PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect},
    Threading::GetCurrentProcess,
};

use crate::{
    error::{Error, Result},
    hooks,
};

#[cfg(any(feature = "d3d9", feature = "d3d10", feature = "d3d11", feature = "d3d12"))]
pub mod d3d;

#[cfg(feature = "opengl")]
pub mod opengl;

#[cfg(feature = "vulkan")]
pub mod vulkan;

pub enum RenderType {
    OPENGL = 0,
    VULKAN,
    D3D9,
    D3D10,
    D3D11,
    D3D12,
}

pub type MethodTable = Vec<*mut usize>;

#[derive(Clone, Copy)]
enum PatchMode {
    VTableSlot,
    Inline,
}

enum InstalledHook {
    VTableSlot {
        index: usize,
        slot: *mut usize,
        original: usize,
        old_protect: PAGE_PROTECTION_FLAGS,
    },
    Inline {
        index: usize,
        target: *mut u8,
        original: [u8; INLINE_PATCH_SIZE],
        old_protect: PAGE_PROTECTION_FLAGS,
    },
}

pub struct GraphicsHook {
    method_table: MethodTable,
    hooks: Vec<InstalledHook>,
    patch_mode: PatchMode,
}

#[cfg(target_pointer_width = "64")]
const INLINE_PATCH_SIZE: usize = 12;
#[cfg(target_pointer_width = "32")]
const INLINE_PATCH_SIZE: usize = 5;

impl GraphicsHook {
    /// Acquires the method table for a graphics API.
    ///
    /// Direct3D render types use writable vtable slots. OpenGL and Vulkan use
    /// exported function addresses and are hooked with a small inline jump.
    pub fn new(render_type: RenderType) -> Result<Self> {
        let patch_mode = match render_type {
            RenderType::D3D9 | RenderType::D3D10 | RenderType::D3D11 | RenderType::D3D12 => PatchMode::VTableSlot,
            RenderType::OPENGL | RenderType::VULKAN => PatchMode::Inline,
        };
        let method_table: MethodTable = match render_type {
            RenderType::OPENGL => {
                #[cfg(not(feature = "opengl"))]
                return Err(Error::RenderType);
                hooks::opengl::get_method_table()?
            },
            RenderType::VULKAN => {
                #[cfg(not(feature = "vulkan"))]
                return Err(Error::RenderType);
                hooks::vulkan::get_method_table()?
            },
            RenderType::D3D9 | RenderType::D3D10 | RenderType::D3D11 | RenderType::D3D12 => {
                #[cfg(not(any(feature = "d3d9", feature = "d3d10", feature = "d3d11", feature = "d3d12")))]
                return Err(Error::RenderType);
                hooks::d3d::get_method_table(render_type)?
            },
        };
        self.method_table = method_table;

        Ok(Self {
            method_table,
            hooks: Vec::new(),
            patch_mode,
        })
    }

    /// Returns the current function pointer for a located method.
    ///
    /// # Errors
    /// Returns `Error::HookIndex` if `index` is outside the located method
    /// table.
    pub fn method(&self, index: usize) -> Result<*mut c_void> {
        let slot = self.method_table.get(index).ok_or(Error::HookIndex)?;
        Ok(unsafe { **slot as *mut c_void })
    }

    /// Installs `detour` for a located method and returns the original pointer.
    ///
    /// Direct3D hooks swap vtable slots, so the returned pointer remains
    /// callable. OpenGL/Vulkan hooks patch the function prologue; the
    /// returned pointer is the original entry address, not a trampoline.
    ///
    /// # Errors
    /// Returns `Error::HookIndex` for an invalid index,
    /// `Error::HookAlreadyInstalled` if the same index is already hooked,
    /// or `Error::HookProtection` if memory protection cannot be changed.
    pub fn hook(&mut self, index: usize, detour: *mut c_void) -> Result<*mut c_void> {
        if self.hooks.iter().any(|hook| hook.index() == index) {
            return Err(Error::HookAlreadyInstalled);
        }

        let slot = *self.method_table.get(index).ok_or(Error::HookIndex)?;
        match self.patch_mode {
            PatchMode::VTableSlot => self.hook_vtable_slot(index, slot, detour),
            PatchMode::Inline => self.hook_inline(index, slot, detour),
        }
    }

    fn hook_vtable_slot(&mut self, index: usize, slot: *mut usize, detour: *mut c_void) -> Result<*mut c_void> {
        let mut old_protect = PAGE_PROTECTION_FLAGS(0);

        unsafe {
            VirtualProtect(
                slot.cast_const().cast(),
                std::mem::size_of::<usize>(),
                PAGE_EXECUTE_READWRITE,
                &mut old_protect,
            )
            .map_err(|_| Error::HookProtection)?;

            let original = *slot;
            *slot = detour as usize;

            let mut ignored = PAGE_PROTECTION_FLAGS(0);
            let _ = VirtualProtect(
                slot.cast_const().cast(),
                std::mem::size_of::<usize>(),
                old_protect,
                &mut ignored,
            );

            self.hooks.push(InstalledHook::VTableSlot {
                index,
                slot,
                original,
                old_protect,
            });

            Ok(original as *mut c_void)
        }
    }

    fn hook_inline(&mut self, index: usize, slot: *mut usize, detour: *mut c_void) -> Result<*mut c_void> {
        let target = unsafe { *slot as *mut u8 };
        if target.is_null() || detour.is_null() {
            return Err(Error::HookIndex);
        }

        let mut old_protect = PAGE_PROTECTION_FLAGS(0);
        let mut original = [0; INLINE_PATCH_SIZE];

        unsafe {
            VirtualProtect(
                target.cast_const().cast(),
                INLINE_PATCH_SIZE,
                PAGE_EXECUTE_READWRITE,
                &mut old_protect,
            )
            .map_err(|_| Error::HookProtection)?;

            std::ptr::copy_nonoverlapping(target, original.as_mut_ptr(), INLINE_PATCH_SIZE);
            write_inline_jump(target, detour)?;
            let _ = FlushInstructionCache(GetCurrentProcess(), Some(target.cast_const().cast()), INLINE_PATCH_SIZE);

            let mut ignored = PAGE_PROTECTION_FLAGS(0);
            let _ = VirtualProtect(target.cast_const().cast(), INLINE_PATCH_SIZE, old_protect, &mut ignored);

            self.hooks.push(InstalledHook::Inline {
                index,
                target,
                original,
                old_protect,
            });

            Ok(target.cast())
        }
    }

    /// Restores every installed hook.
    ///
    /// # Errors
    /// Returns `Error::HookProtection` if memory protection cannot be changed.
    pub fn unhook(&mut self) -> Result<()> {
        for hook in self.hooks.drain(..).rev() {
            hook.restore()?;
        }

        Ok(())
    }
}

impl Drop for GraphicsHook {
    fn drop(&mut self) {
        let _ = self.unhook();

        if matches!(self.patch_mode, PatchMode::Inline) {
            for slot in self.method_table.drain(..) {
                unsafe {
                    drop(Box::from_raw(slot));
                }
            }
        }
    }
}

impl InstalledHook {
    fn index(&self) -> usize {
        match self {
            Self::VTableSlot { index, .. } | Self::Inline { index, .. } => *index,
        }
    }

    fn restore(self) -> Result<()> {
        match self {
            Self::VTableSlot {
                slot,
                original,
                old_protect,
                ..
            } => restore_vtable_slot(slot, original, old_protect),
            Self::Inline {
                target,
                original,
                old_protect,
                ..
            } => restore_inline_patch(target, &original, old_protect),
        }
    }
}

fn restore_vtable_slot(slot: *mut usize, original: usize, old_protect: PAGE_PROTECTION_FLAGS) -> Result<()> {
    let mut ignored = PAGE_PROTECTION_FLAGS(0);

    unsafe {
        VirtualProtect(
            slot.cast_const().cast(),
            std::mem::size_of::<usize>(),
            PAGE_EXECUTE_READWRITE,
            &mut ignored,
        )
        .map_err(|_| Error::HookProtection)?;

        *slot = original;

        VirtualProtect(
            slot.cast_const().cast(),
            std::mem::size_of::<usize>(),
            old_protect,
            &mut ignored,
        )
        .map_err(|_| Error::HookProtection)
    }
}

fn restore_inline_patch(
    target: *mut u8,
    original: &[u8; INLINE_PATCH_SIZE],
    old_protect: PAGE_PROTECTION_FLAGS,
) -> Result<()> {
    let mut ignored = PAGE_PROTECTION_FLAGS(0);

    unsafe {
        VirtualProtect(
            target.cast_const().cast(),
            INLINE_PATCH_SIZE,
            PAGE_EXECUTE_READWRITE,
            &mut ignored,
        )
        .map_err(|_| Error::HookProtection)?;

        std::ptr::copy_nonoverlapping(original.as_ptr(), target, INLINE_PATCH_SIZE);
        let _ = FlushInstructionCache(GetCurrentProcess(), Some(target.cast_const().cast()), INLINE_PATCH_SIZE);

        VirtualProtect(target.cast_const().cast(), INLINE_PATCH_SIZE, old_protect, &mut ignored)
            .map_err(|_| Error::HookProtection)
    }
}

#[cfg(target_pointer_width = "64")]
unsafe fn write_inline_jump(target: *mut u8, detour: *mut c_void) -> Result<()> {
    *target = 0x48;
    *target.add(1) = 0xB8;
    std::ptr::write_unaligned(target.add(2).cast::<usize>(), detour as usize);
    *target.add(10) = 0xFF;
    *target.add(11) = 0xE0;
    Ok(())
}

#[cfg(target_pointer_width = "32")]
unsafe fn write_inline_jump(target: *mut u8, detour: *mut c_void) -> Result<()> {
    let displacement = (detour as isize)
        .checked_sub(target as isize)
        .and_then(|value| value.checked_sub(INLINE_PATCH_SIZE as isize))
        .ok_or(Error::HookProtection)?;
    let displacement = i32::try_from(displacement).map_err(|_| Error::HookProtection)?;

    *target = 0xE9;
    std::ptr::write_unaligned(target.add(1).cast::<i32>(), displacement);
    Ok(())
}
