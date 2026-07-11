use std::ffi::CString;

use windows::{
    core::PCSTR,
    Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress},
};

use crate::{
    error::{Error, Result},
    hooks::MethodTable,
};

const OPENGL_METHODS: &[&str] = &[
    "glAccum",
    "glAlphaFunc",
    "glBegin",
    "glBindTexture",
    "glBlendFunc",
    "glCallList",
    "glClear",
    "glClearColor",
    "glClearDepth",
    "glClearStencil",
    "glColorPointer",
    "glCullFace",
    "glDeleteTextures",
    "glDepthFunc",
    "glDepthMask",
    "glDisable",
    "glDrawArrays",
    "glDrawElements",
    "glEnable",
    "glEnd",
    "glFinish",
    "glFlush",
    "glGetError",
    "glGetString",
    "glLoadIdentity",
    "glMatrixMode",
    "glOrtho",
    "glPopMatrix",
    "glPushMatrix",
    "glReadPixels",
    "glScissor",
    "glTexCoordPointer",
    "glTexImage2D",
    "glTexParameterf",
    "glTexParameteri",
    "glTexSubImage2D",
    "glVertexPointer",
    "glViewport",
    "wglSwapBuffers",
];

pub fn get_method_table() -> Result<MethodTable> {
    let module_name = CString::new("opengl32.dll")?;
    let module = unsafe { GetModuleHandleA(PCSTR(module_name.as_ptr().cast())) }.map_err(|_| Error::RenderType)?;

    Ok(OPENGL_METHODS
        .iter()
        .map(|name| {
            let name = CString::new(*name).expect("OpenGL method names cannot contain NUL bytes");
            let address = unsafe { GetProcAddress(module.into(), PCSTR(name.as_ptr().cast())) }
                .map_or(0, |function| function as usize);
            Box::into_raw(Box::new(address))
        })
        .collect())
}
