#![cfg(target_os = "windows")]

use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;
use synth_ui::{DEFAULT_SOURCE, UiModel, WebViewHost};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, RegisterClassW, SW_SHOW, SetTimer,
    ShowWindow, TranslateMessage, WM_DESTROY, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

const CLOSE_TIMER: usize = 1;

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER if wparam == CLOSE_TIMER => {
            unsafe { DestroyWindow(window) };
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let class_name = wide("CodeSynthUiSmokeWindow");
    let title = wide("Code Synthesizer UI smoke test");
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        return Err("GetModuleHandleW failed".into());
    }
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..unsafe { MaybeUninit::zeroed().assume_init() }
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err("RegisterClassW failed".into());
    }
    let window = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1050,
            680,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    if window.is_null() {
        return Err("CreateWindowExW failed".into());
    }
    unsafe {
        ShowWindow(window, SW_SHOW);
        UpdateWindow(window);
    }
    let model = UiModel::new(DEFAULT_SOURCE);
    let _webview = unsafe { WebViewHost::attach(window.cast::<c_void>(), 1034, 641, model) }
        .map_err(|error| format!("WebView attach failed: {error}"))?;
    unsafe { SetTimer(window, CLOSE_TIMER, 5_000, None) };

    let mut message = MaybeUninit::<MSG>::zeroed();
    while unsafe { GetMessageW(message.as_mut_ptr(), ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(message.as_ptr());
            DispatchMessageW(message.as_ptr());
        }
    }
    Ok(())
}
