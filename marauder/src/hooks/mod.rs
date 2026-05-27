use crate::{
    error::{Error, Result},
    hooks,
};

#[cfg(any(feature = "d2d", feature = "d3d9", feature = "d3d10", feature = "d3d11", feature = "d3d12"))]
pub mod directx;

#[cfg(feature = "opengl")]
pub mod opengl;

#[cfg(feature = "vulkan")]
pub mod vulkan;

pub enum RenderType {
    OPENGL = 0,
    VULKAN,
    D2D,
    D3D9,
    D3D10,
    D3D11,
    D3D12,
}

pub type MethodTable = Vec<*const usize>;

pub struct GraphicsHook {
    method_table: MethodTable,
}

impl GraphicsHook {
    /// Acquires the method table by creating a dummy device
    pub fn new(render_type: RenderType) -> Result<Self> {
        let method_table: MethodTable = match render_type {
            RenderType::OPENGL => {
                #[cfg(not(feature = "opengl"))]
                return Err(Error::RenderType);
                #[cfg(feature = "opengl")]
                hooks::opengl::get_method_table()?
            },
            RenderType::VULKAN => {
                #[cfg(not(feature = "vulkan"))]
                return Err(Error::RenderType);
                #[cfg(feature = "vulkan")]
                hooks::vulkan::get_method_table()?
            },
            RenderType::D2D | RenderType::D3D9 | RenderType::D3D10 | RenderType::D3D11 | RenderType::D3D12 => {
                #[cfg(not(any(feature = "d2d", feature = "d3d9", feature = "d3d10", feature = "d3d11", feature = "d3d12")))]
                return Err(Error::RenderType);
                #[cfg(any(feature = "d2d", feature = "d3d9", feature = "d3d10", feature = "d3d11", feature = "d3d12"))]
                hooks::directx::get_method_table(render_type)?
            },
        };
        self.method_table = method_table;

        todo!()
    }

    pub fn hook(index: u16) -> Result<()> {
        todo!()
    }

    pub fn unhook() -> Result<()> {
        todo!()
    }
}
