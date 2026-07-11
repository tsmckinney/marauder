use std::ffi::CString;

use windows::{
    Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress},
    core::PCSTR,
};

use crate::{
    error::{Error, Result},
    hooks::MethodTable,
};

const VULKAN_METHODS: &[&str] = &[
    "vkCreateInstance",
    "vkDestroyInstance",
    "vkEnumeratePhysicalDevices",
    "vkGetPhysicalDeviceFeatures",
    "vkGetPhysicalDeviceProperties",
    "vkGetInstanceProcAddr",
    "vkGetDeviceProcAddr",
    "vkCreateDevice",
    "vkDestroyDevice",
    "vkGetDeviceQueue",
    "vkQueueSubmit",
    "vkQueueWaitIdle",
    "vkDeviceWaitIdle",
    "vkAllocateMemory",
    "vkFreeMemory",
    "vkMapMemory",
    "vkUnmapMemory",
    "vkCreateImage",
    "vkDestroyImage",
    "vkCreateImageView",
    "vkDestroyImageView",
    "vkCreateShaderModule",
    "vkDestroyShaderModule",
    "vkCreateGraphicsPipelines",
    "vkCreateComputePipelines",
    "vkDestroyPipeline",
    "vkCreateCommandPool",
    "vkDestroyCommandPool",
    "vkAllocateCommandBuffers",
    "vkFreeCommandBuffers",
    "vkBeginCommandBuffer",
    "vkEndCommandBuffer",
    "vkCmdBindPipeline",
    "vkCmdDraw",
    "vkCmdDrawIndexed",
    "vkCmdDispatch",
    "vkCmdBeginRenderPass",
    "vkCmdEndRenderPass",
    "vkQueuePresentKHR",
    "vkCreateSwapchainKHR",
    "vkDestroySwapchainKHR",
    "vkAcquireNextImageKHR",
];

pub fn get_method_table() -> Result<MethodTable> {
    let module_name = CString::new("vulkan-1.dll")?;
    let module = unsafe { GetModuleHandleA(PCSTR(module_name.as_ptr().cast())) }.map_err(|_| Error::RenderType)?;

    Ok(VULKAN_METHODS
        .iter()
        .map(|name| {
            let name = CString::new(*name).expect("Vulkan method names cannot contain NUL bytes");
            let address = unsafe { GetProcAddress(module.into(), PCSTR(name.as_ptr().cast())) }
                .map_or(0, |function| function as usize);
            Box::into_raw(Box::new(address))
        })
        .collect())
}
