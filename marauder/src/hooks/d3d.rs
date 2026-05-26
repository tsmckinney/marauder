use windows::{
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_11_0,
            },
            Direct3D9::{
                D3D_SDK_VERSION, D3DCREATE_SOFTWARE_VERTEXPROCESSING, D3DDEVTYPE_HAL, D3DFMT_UNKNOWN, D3DMULTISAMPLE_NONE,
                D3DPRESENT_PARAMETERS, D3DSWAPEFFECT_DISCARD, Direct3DCreate9, IDirect3DDevice9, IDirect3DDevice9_Vtbl,
            },
            Direct3D10::{
                D3D10_DRIVER_TYPE_HARDWARE, D3D10_SDK_VERSION, D3D10CreateDeviceAndSwapChain, ID3D10Device,
                ID3D10Device_Vtbl,
            },
            Direct3D11::{
                D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDeviceAndSwapChain, ID3D11Device, ID3D11Device_Vtbl,
                ID3D11DeviceContext, ID3D11DeviceContext_Vtbl,
            },
            Direct3D12::{
                D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_COMMAND_QUEUE_FLAG_NONE, D3D12CreateDevice,
                ID3D12CommandAllocator, ID3D12CommandAllocator_Vtbl, ID3D12CommandQueue, ID3D12CommandQueue_Vtbl,
                ID3D12Device, ID3D12Device_Vtbl, ID3D12GraphicsCommandList, ID3D12GraphicsCommandList_Vtbl,
            },
            Dxgi::{
                Common::{
                    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_MODE_SCALING_UNSPECIFIED,
                    DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
                },
                CreateDXGIFactory, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH, DXGI_SWAP_EFFECT_DISCARD,
                DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIFactory, IDXGISwapChain,
                IDXGISwapChain_Vtbl,
            },
        },
        UI::WindowsAndMessaging::{
            CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DestroyWindow, RegisterClassExW, UnregisterClassW, WNDCLASSEXW,
            WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW,
        },
    },
    core::{Interface, w},
};

use crate::{
    error::{Error, Result},
    hooks::{MethodTable, RenderType},
};

pub fn get_method_table(render_type: RenderType) -> Result<MethodTable> {
    match render_type {
        RenderType::D3D9 => with_dummy_window(get_d3d9_method_table),
        RenderType::D3D10 => with_dummy_window(get_d3d10_method_table),
        RenderType::D3D11 => with_dummy_window(get_d3d11_method_table),
        RenderType::D3D12 => with_dummy_window(get_d3d12_method_table),
        _ => unreachable!(),
    }
}

fn with_dummy_window(method: impl FnOnce(HWND) -> Result<MethodTable>) -> Result<MethodTable> {
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
        lpszClassName: w!("marauder"),
        hIconSm: Default::default(),
    };

    unsafe {
        RegisterClassExW(&window_class);
    }

    let window = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            window_class.lpszClassName,
            w!(""),
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
    }
    .map_err(|_| Error::DummyDevice)?;

    let method_table = match render_type {
        RenderType::D3D9 => {
            let direct3d9 = unsafe { Direct3DCreate9(D3D_SDK_VERSION).unwrap() };
            let params = D3DPRESENT_PARAMETERS {
                BackBufferWidth: 0,
                BackBufferHeight: 0,
                BackBufferFormat: D3DFMT_UNKNOWN,
                BackBufferCount: 0,
                MultiSampleType: D3DMULTISAMPLE_NONE,
                MultiSampleQuality: 0,
                SwapEffect: D3DSWAPEFFECT_DISCARD,
                hDeviceWindow: window,
                Windowed: true.into(),
                EnableAutoDepthStencil: false.into(),
                AutoDepthStencilFormat: D3DFMT_UNKNOWN,
                Flags: 0,
                FullScreen_RefreshRateInHz: 0,
                PresentationInterval: 0,
            };
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
                Err(Error::DummyDevice)
            }

            // size is the size of the elements, not the bytes this is similar to calloc in
            // c++
            let method_table = unsafe {
                std::slice::from_raw_parts((device_interface as *const *const MethodTable).read(), D3D9_VTABLE_ELEMENTS)
            }
            .to_vec();
            if method_table.is_empty() {
                Err(Error::DummyDevice)
            }

            Ok(method_table)
        },
        RenderType::D3D10 => unsafe {
            let factory = CreateDXGIFactory::<IDXGIFactory>();
            if factory.is_err() {
                return Err(Error::DummyDevice);
            }
            let adapter: *const IDXGIAdapter = null();
            factory.unwrap().EnumAdapters(&adapter as u32);
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
            let swap_chain_desc = DXGI_SWAP_CHAIN_DESC {
                BufferDesc: buffer,
                SampleDesc: sample,
                BufferUsage: 32,
                BufferCount: 1,
                OutputWindow: window,
                Windowed: true.into(),
                SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
                Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
            };

            let swap_chain = null_mut();
            let device = null_mut();

            D3D10CreateDeviceAndSwapChain(
                adapter,
                D3D10_DRIVER_TYPE_HARDWARE,
                null(),
                0,
                D3D10_SDK_VERSION,
                &swap_chain_desc as *mut DXGI_SWAP_CHAIN_DESC,
                swap_chain,
                device,
            )
            .unwrap();

            // size is the size of the elements, not the bytes this is similar to calloc in
            // c++
            let method_table =
                unsafe { std::slice::from_raw_parts((device as *const *const MethodTable).read(), D3D10_VTABLE_ELEMENTS) }
                    .to_vec();
            if method_table.is_empty() {
                Err(Error::DummyDevice)
            }

            Ok(method_table)
        },
        RenderType::D3D11 => unsafe {
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
            let swap_chain_desc = DXGI_SWAP_CHAIN_DESC {
                BufferDesc: buffer_desc,
                SampleDesc: sample_desc,
                BufferUsage: 32,
                BufferCount: 1,
                OutputWindow: window,
                Windowed: true.into(),
                SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
                Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
            };
            let swap_chain = null_mut();
            let device = null_mut();
            let context = null_mut();

            D3D11CreateDeviceAndSwapChain(
                null_mut(),
                D3D_DRIVER_TYPE_HARDWARE,
                null_mut(),
                D3D11_CREATE_DEVICE_FLAG(0),
                feature_levels.as_ptr(),
                2,
                D3D11_SDK_VERSION,
                swap_chain_desc as *const DXGI_SWAP_CHAIN_DESC,
                swap_chain,
                device,
                feature_level,
                context,
            )
            .unwrap();
            // size is the size of the elements, not the bytes this is similar to calloc in
            // c++
            let method_table =
                unsafe { std::slice::from_raw_parts((device as *const *const MethodTable).read(), D3D11_VTABLE_ELEMENTS) }
                    .to_vec();
            if method_table.is_empty() {
                Err(Error::DummyDevice)
            }
        },
        RenderType::D3D12 => unsafe {
            let feature_level = D3D_FEATURE_LEVEL_11_0;
            let factory = CreateDXGIFactory::<IDXGIFactory>();
            let adapter = factory.unwrap().EnumAdapters();
            let device = D3D12CreateDevice::<ID3D12Device>(adapter, D3D_FEATURE_LEVEL_11_0);
            let queue_desc = D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                Priority: 0,
                Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
                NodeMask: 0,
            };
            let command_queue = device.unwrap().CreateCommandQueue::<ID3D12CommandQueue>(&queue_desc).unwrap();
            let command_allocator = device
                .unwrap()
                .CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .unwrap();
            let command_list = device
                .unwrap()
                .CreateCommandList::<ID3D12GraphicsCommandList>(
                    0,
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                    command_allocator,
                    null_mut(),
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
            let swap_chain_desc = DXGI_SWAP_CHAIN_DESC {
                BufferDesc: buffer_desc,
                SampleDesc: sample_desc,
                BufferUsage: 32,
                BufferCount: 2,
                OutputWindow: window,
                Windowed: true.into(),
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
            };
            let device = null_mut();
            let swap_chain = factory
                .unwrap()
                .CreateSwapChain(command_queue, &swap_chain_desc as *mut DXGI_SWAP_CHAIN_DESC)
                .unwrap();
            D3D12CreateDevice(null_mut(), feature_level, device);
            // size is the size of the elements, not the bytes this is similar to calloc in
            // c++
            let method_table =
                unsafe { std::slice::from_raw_parts((device as *const *const MethodTable).read(), D3D11_VTABLE_ELEMENTS) }
                    .to_vec();
            if method_table.is_empty() {
                Err(Error::DummyDevice)
            }
        },
        _ => unreachable!(),
    };

    destroy_class(&window_class, &window);

    method_table
}

fn get_d3d9_method_table(window: HWND) -> Result<MethodTable> {
    let direct3d9 = unsafe { Direct3DCreate9(D3D_SDK_VERSION) }.ok_or(Error::DummyDevice)?;
    let mut params = D3DPRESENT_PARAMETERS {
        BackBufferWidth: 0,
        BackBufferHeight: 0,
        BackBufferFormat: D3DFMT_UNKNOWN,
        BackBufferCount: 0,
        MultiSampleType: D3DMULTISAMPLE_NONE,
        MultiSampleQuality: 0,
        SwapEffect: D3DSWAPEFFECT_DISCARD,
        hDeviceWindow: window,
        Windowed: true.into(),
        EnableAutoDepthStencil: false.into(),
        AutoDepthStencilFormat: D3DFMT_UNKNOWN,
        Flags: 0,
        FullScreen_RefreshRateInHz: 0,
        PresentationInterval: 0,
    };
    let mut device: Option<IDirect3DDevice9> = None;

    unsafe {
        direct3d9
            .CreateDevice(
                0,
                D3DDEVTYPE_HAL,
                window,
                D3DCREATE_SOFTWARE_VERTEXPROCESSING as u32,
                &mut params,
                &mut device,
            )
            .map_err(|_| Error::DummyDevice)?;
    }

    let device = device.ok_or(Error::DummyDevice)?;
    Ok(vtable_slots::<IDirect3DDevice9, IDirect3DDevice9_Vtbl>(&device))
}

fn get_d3d10_method_table(window: HWND) -> Result<MethodTable> {
    let factory = unsafe { CreateDXGIFactory::<IDXGIFactory>() }.map_err(|_| Error::DummyDevice)?;
    let adapter = unsafe { factory.EnumAdapters(0) }.map_err(|_| Error::DummyDevice)?;
    let swap_chain_desc = swap_chain_desc(window, DXGI_SWAP_EFFECT_DISCARD, 1);
    let mut swap_chain: Option<IDXGISwapChain> = None;
    let mut device: Option<ID3D10Device> = None;

    unsafe {
        D3D10CreateDeviceAndSwapChain(
            &adapter,
            D3D10_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            0,
            D3D10_SDK_VERSION,
            Some(&swap_chain_desc),
            Some(&mut swap_chain),
            Some(&mut device),
        )
        .map_err(|_| Error::DummyDevice)?;
    }

    let swap_chain = swap_chain.ok_or(Error::DummyDevice)?;
    let device = device.ok_or(Error::DummyDevice)?;

    let mut methods = vtable_slots::<IDXGISwapChain, IDXGISwapChain_Vtbl>(&swap_chain);
    methods.extend(vtable_slots::<ID3D10Device, ID3D10Device_Vtbl>(&device));
    Ok(methods)
}

fn get_d3d11_method_table(window: HWND) -> Result<MethodTable> {
    let factory = unsafe { CreateDXGIFactory::<IDXGIFactory>() }.map_err(|_| Error::DummyDevice)?;
    let adapter = unsafe { factory.EnumAdapters(0) }.map_err(|_| Error::DummyDevice)?;
    let swap_chain_desc = swap_chain_desc(window, DXGI_SWAP_EFFECT_DISCARD, 1);
    let feature_levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
    let mut swap_chain: Option<IDXGISwapChain> = None;
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut feature_level = D3D_FEATURE_LEVEL(0);

    unsafe {
        D3D11CreateDeviceAndSwapChain(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&swap_chain_desc),
            Some(&mut swap_chain),
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )
        .map_err(|_| Error::DummyDevice)?;
    }

    let swap_chain = swap_chain.ok_or(Error::DummyDevice)?;
    let device = device.ok_or(Error::DummyDevice)?;
    let context = context.ok_or(Error::DummyDevice)?;

    let mut methods = vtable_slots::<IDXGISwapChain, IDXGISwapChain_Vtbl>(&swap_chain);
    methods.extend(vtable_slots::<ID3D11Device, ID3D11Device_Vtbl>(&device));
    methods.extend(vtable_slots::<ID3D11DeviceContext, ID3D11DeviceContext_Vtbl>(&context));
    Ok(methods)
}

fn get_d3d12_method_table(window: HWND) -> Result<MethodTable> {
    let factory = unsafe { CreateDXGIFactory::<IDXGIFactory>() }.map_err(|_| Error::DummyDevice)?;
    let adapter: IDXGIAdapter = unsafe { factory.EnumAdapters(0) }.map_err(|_| Error::DummyDevice)?;
    let mut device: Option<ID3D12Device> = None;

    unsafe {
        D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device).map_err(|_| Error::DummyDevice)?;
    }

    let device = device.ok_or(Error::DummyDevice)?;
    let queue_desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        Priority: 0,
        Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
        NodeMask: 0,
    };
    let command_queue: ID3D12CommandQueue =
        unsafe { device.CreateCommandQueue(&queue_desc) }.map_err(|_| Error::DummyDevice)?;
    let command_allocator: ID3D12CommandAllocator =
        unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.map_err(|_| Error::DummyDevice)?;
    let command_list: ID3D12GraphicsCommandList = unsafe {
        device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &command_allocator,
            None::<&windows::Win32::Graphics::Direct3D12::ID3D12PipelineState>,
        )
    }
    .map_err(|_| Error::DummyDevice)?;

    let swap_chain_desc = swap_chain_desc(window, DXGI_SWAP_EFFECT_FLIP_DISCARD, 2);
    let mut swap_chain: Option<IDXGISwapChain> = None;
    unsafe {
        factory
            .CreateSwapChain(&command_queue, &swap_chain_desc, &mut swap_chain)
            .ok()
            .map_err(|_| Error::DummyDevice)?;
    }
    let swap_chain = swap_chain.ok_or(Error::DummyDevice)?;

    let mut methods = vtable_slots::<ID3D12Device, ID3D12Device_Vtbl>(&device);
    methods.extend(vtable_slots::<ID3D12CommandQueue, ID3D12CommandQueue_Vtbl>(&command_queue));
    methods.extend(vtable_slots::<ID3D12CommandAllocator, ID3D12CommandAllocator_Vtbl>(
        &command_allocator,
    ));
    methods.extend(vtable_slots::<ID3D12GraphicsCommandList, ID3D12GraphicsCommandList_Vtbl>(
        &command_list,
    ));
    methods.extend(vtable_slots::<IDXGISwapChain, IDXGISwapChain_Vtbl>(&swap_chain));
    Ok(methods)
}

fn swap_chain_desc(
    window: HWND,
    swap_effect: windows::Win32::Graphics::Dxgi::DXGI_SWAP_EFFECT,
    buffer_count: u32,
) -> DXGI_SWAP_CHAIN_DESC {
    DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: 100,
            Height: 100,
            RefreshRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
            Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
        },
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: buffer_count,
        OutputWindow: window,
        Windowed: true.into(),
        SwapEffect: swap_effect,
        Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
    }
}

fn vtable_slots<T, Vtbl>(interface: &T) -> MethodTable
where
    T: Interface,
{
    let slot_count = std::mem::size_of::<Vtbl>() / std::mem::size_of::<usize>();
    let vtable = unsafe { *(interface.as_raw() as *mut *mut usize) };

    (0..slot_count).map(|index| unsafe { vtable.add(index) }).collect()
}

fn destroy_class(class: &WNDCLASSEXW, window: HWND) {
    unsafe {
        let _ = DestroyWindow(window);
        let _ = UnregisterClassW(class.lpszClassName, Some(class.hInstance));
    };
}
