use std::{
    ffi::CString,
    ptr::{null, null_mut},
};

use windows::Win32::{
    Foundation::{HMODULE, HWND},
    Graphics::{
        Direct3D::{
            D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0
        },
        Direct3D9::{
            D3D_SDK_VERSION, D3DDEVTYPE_HAL, D3DFMT_UNKNOWN, D3DMULTISAMPLE_NONE,
            D3DPRESENT_PARAMETERS, D3DSWAPEFFECT_DISCARD, Direct3DCreate9, IDirect3DDevice9,
        },
        Direct3D10::{D3D10_DRIVER_TYPE_HARDWARE, D3D10_SDK_VERSION, D3D10CreateDeviceAndSwapChain, ID3D10Device},
        Direct3D11::{
            D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDeviceAndSwapChain, ID3D11Device,
            ID3D11DeviceContext,
        },
        Direct3D12::{
            D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_COMMAND_QUEUE_FLAG_NONE, D3D12CreateDevice,
            ID3D12CommandAllocator, ID3D12CommandQueue, ID3D12Device, ID3D12GraphicsCommandList, ID3D12PipelineState,
            D3D12_COMPUTE_PIPELINE_STATE_DESC,
        },
        Dxgi::{
            Common::{
                DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_MODE_SCALING_UNSPECIFIED, DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_RATIONAL, DXGI_SAMPLE_DESC
            },
            CreateDXGIFactory,
            DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH, DXGI_SWAP_EFFECT_DISCARD,
            DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE, IDXGIAdapter, IDXGIFactory, IDXGISwapChain,
        },
    },
    UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DestroyWindow, UnregisterClassW, WNDCLASSEXW, WS_EX_APPWINDOW,
        WS_OVERLAPPEDWINDOW,
    },
};
use windows_strings::PCWSTR;

use crate::{
    error::{Error, Result},
    hooks::{MethodTable, RenderType},
    windows::{
        utils::convert_windows_string,
        wrappers::{HandleInstance, get_module_handle, get_proc_address},
    },
};

#[cfg(feature = "d3d9")]
const D3D9_VTABLE_ELEMENTS: usize = 119;
#[cfg(feature = "d3d10")]
const D3D10_VTABLE_ELEMENTS: usize = 116;
#[cfg(feature = "d3d11")]
const D3D11_VTABLE_ELEMENTS: usize = 205;
#[cfg(feature = "d3d12")]
const D3D12_VTABLE_ELEMENTS: usize = 150;

pub fn get_method_table(render_type: RenderType) -> Result<MethodTable> {
    let window_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: None,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: Default::default(),
        hIcon: Default::default(),
        hCursor: Default::default(),
        hbrBackground: Default::default(),
        lpszMenuName: Default::default(),
        lpszClassName: PCWSTR("marauder".as_ptr() as *const u16),
        hIconSm: Default::default(),
    };

    let window = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            window_class.lpszClassName,
            PCWSTR("".as_ptr() as *const u16),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(window_class.hInstance),
            None,
        )
    };

    let method_table: Result<Vec<Vec<*const usize>>> = match render_type {
        RenderType::D3D9 => {
            let direct3d9 = unsafe { Direct3DCreate9(D3D_SDK_VERSION).unwrap() };
            let params: D3DPRESENT_PARAMETERS;
            if let Ok(wnd) = window {
                params = D3DPRESENT_PARAMETERS {
                    BackBufferWidth: 0,
                    BackBufferHeight: 0,
                    BackBufferFormat: D3DFMT_UNKNOWN,
                    BackBufferCount: 0,
                    MultiSampleType: D3DMULTISAMPLE_NONE,
                    MultiSampleQuality: 0,
                    SwapEffect: D3DSWAPEFFECT_DISCARD,
                    hDeviceWindow: wnd,
                    Windowed: true.into(),
                    EnableAutoDepthStencil: false.into(),
                    AutoDepthStencilFormat: D3DFMT_UNKNOWN,
                    Flags: 0,
                    FullScreen_RefreshRateInHz: 0,
                    PresentationInterval: 0,
                };
            } else {
                return Err(Error::DummyDevice)
            }
            let device_interface: *mut IDirect3DDevice9 = std::ptr::null_mut();
            let dummy_device = unsafe {
                direct3d9.CreateDevice(
                    0u32,
                    D3DDEVTYPE_HAL,
                    params.hDeviceWindow,
                    32u32 | 256u32,
                    std::mem::transmute(params),
                    std::mem::transmute(device_interface),
                )
            };

            if dummy_device.is_err() {
                return Err(Error::DummyDevice)
            }

            // size is the size of the elements, not the bytes this is similar to calloc in
            // c++
            let method_table = unsafe {
                std::slice::from_raw_parts((device_interface as *const *const MethodTable).read(), D3D9_VTABLE_ELEMENTS)
            };
            if method_table.is_empty() {
                return Err(Error::DummyDevice)
            }

            Ok(method_table)
        },
        RenderType::D3D10 => unsafe {
            let factory = CreateDXGIFactory::<IDXGIFactory>();
            if factory.is_err() {
                return Err(Error::DummyDevice);
            }
            let adapter: IDXGIAdapter;
            factory.unwrap().EnumAdapters(&raw mut adapter as u32);
            let refresh_rate = DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            };

            let buffer_desc = DXGI_MODE_DESC {
                Width: 100,
                Height: 100,
                RefreshRate: refresh_rate,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
            };
            let sample_desc = DXGI_SAMPLE_DESC { Count: 1, Quality: 0 };
            let swap_chain_desc: DXGI_SWAP_CHAIN_DESC;
            if let Ok(wnd) = window {
                swap_chain_desc = DXGI_SWAP_CHAIN_DESC {
                    BufferDesc: buffer_desc,
                    SampleDesc: sample_desc,
                    BufferUsage: DXGI_USAGE(32),
                    BufferCount: 1,
                    OutputWindow: wnd,
                    Windowed: true.into(),
                    SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
                    Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
                };
            }

            let swap_chain: Option<*mut Option<IDXGISwapChain>>;
            let device: Option<*mut Option<ID3D10Device>>;

            D3D10CreateDeviceAndSwapChain(
                &adapter,
                D3D10_DRIVER_TYPE_HARDWARE,
                HMODULE(window_class.hInstance.0),
                0,
                D3D10_SDK_VERSION,
                Some(&swap_chain_desc as *const DXGI_SWAP_CHAIN_DESC),
                swap_chain,
                device,
            )
            .unwrap();

            // size is the size of the elements, not the bytes this is similar to calloc in
            // c++
            let method_table =
                unsafe { std::slice::from_raw_parts((device.unwrap() as *const *const MethodTable).read(), D3D10_VTABLE_ELEMENTS) }
                    .to_vec();
            if method_table.is_empty() {
                return Err(Error::DummyDevice)
            }

            Ok(method_table)
        },
        RenderType::D3D11 => unsafe {
            let factory = CreateDXGIFactory::<IDXGIFactory>();
            if factory.is_err() {
                return Err(Error::DummyDevice);
            }
            let adapter: IDXGIAdapter;
            factory.unwrap().EnumAdapters(&raw mut adapter as u32);
            let feature_levels = vec![D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0];
            let refresh_rate = DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            };
            let buffer_desc = DXGI_MODE_DESC {
                Width: 100,
                Height: 100,
                RefreshRate: refresh_rate,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
            };
            let sample_desc = DXGI_SAMPLE_DESC { Count: 1, Quality: 0 };
            let swap_chain_desc: DXGI_SWAP_CHAIN_DESC;
            if let Ok(wnd) = window {
                swap_chain_desc = DXGI_SWAP_CHAIN_DESC {
                    BufferDesc: buffer_desc,
                    SampleDesc: sample_desc,
                    BufferUsage: DXGI_USAGE(32),
                    BufferCount: 1,
                    OutputWindow: wnd,
                    Windowed: true.into(),
                    SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
                    Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
                };
            }
            let swap_chain: Option<*mut Option<IDXGISwapChain>>;
            let device: Option<*mut Option<ID3D11Device>>;
            let context: Option<*mut Option<ID3D11DeviceContext>>;

            let level: *mut D3D_FEATURE_LEVEL = feature_levels.first_mut().unwrap();

            D3D11CreateDeviceAndSwapChain(
                &adapter,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE(null_mut()),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(feature_levels.as_slice()),
                D3D11_SDK_VERSION,
                Some(&swap_chain_desc as *const DXGI_SWAP_CHAIN_DESC),
                swap_chain,
                device,
                Some(level),
                context,
            )
            .unwrap();
            // size is the size of the elements, not the bytes this is similar to calloc in
            // c++
            let method_table =
                unsafe { std::slice::from_raw_parts((device.unwrap() as *const *const MethodTable).read(), D3D11_VTABLE_ELEMENTS) }
                    .to_vec();
            if method_table.is_empty() {
                return Err(Error::DummyDevice)
            }
            Ok(method_table)
        },
        RenderType::D3D12 => unsafe {
            let feature_level = D3D_FEATURE_LEVEL_11_0;
            let factory = CreateDXGIFactory::<IDXGIFactory>();
            let adapter_num: u32;
            let adapter: IDXGIAdapter = factory.unwrap().EnumAdapters(adapter_num).map_or_else(|_| {
                return Err(Error::DummyDevice);
            }, Ok).unwrap();
            let mut device: ID3D12Device;
            let queue_desc = D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                Priority: 0,
                Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
                NodeMask: 0,
            };
            let command_queue = device.CreateCommandQueue::<ID3D12CommandQueue>(&queue_desc).unwrap();
            let command_allocator = device
                .CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .unwrap();
            let pipeline_state = device.CreateComputePipelineState::<ID3D12PipelineState>(&(D3D12_COMPUTE_PIPELINE_STATE_DESC::default()));
            let command_list = device
                .CreateCommandList(
                    0,
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                    &command_allocator,
                    &pipeline_state.unwrap(),
                )
                .unwrap();
            let refresh_rate = DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            };
            let buffer_desc = DXGI_MODE_DESC {
                Width: 100,
                Height: 100,
                RefreshRate: refresh_rate,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
            };
            let sample_desc = DXGI_SAMPLE_DESC { Count: 1, Quality: 0 };
            let swap_chain_desc: DXGI_SWAP_CHAIN_DESC = DXGI_SWAP_CHAIN_DESC {
                BufferDesc: buffer_desc,
                SampleDesc: sample_desc,
                BufferUsage: DXGI_USAGE(32),
                BufferCount: 2,
                OutputWindow: window.unwrap(),
                Windowed: true.into(),
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
            };
            let swap_chain: IDXGISwapChain;
            factory
                .unwrap()
                .CreateSwapChain(&command_queue, &swap_chain_desc, &mut Some(swap_chain))
                .unwrap();
            let _ = D3D12CreateDevice(&adapter, feature_level, &mut Some(device));
            // size is the size of the elements, not the bytes this is similar to calloc in
            // c++
            let method_table =
                unsafe { std::slice::from_raw_parts((device as *const *const MethodTable).read(), D3D12_VTABLE_ELEMENTS) }
                    .to_vec();
            if method_table.is_empty() {
                return Err(Error::DummyDevice)
            }
            Ok(method_table)
        },
        _ => unreachable!(),
    };

    destroy_class(&window_class, &window.unwrap());

    method_table
}

fn destroy_class(class: &WNDCLASSEXW, window: &HWND) {
    unsafe {
        DestroyWindow(window);
        UnregisterClassW(class.lpszClassName, class.hInstance);
    };
}
