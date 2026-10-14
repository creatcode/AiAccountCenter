//! 启动前 WebView2 检测。
//!
//! 在创建任何 WebView / 主窗口 / 托盘、启动任何后台任务之前，核对正式 Evergreen
//! Runtime 是否可用；缺失时弹原生中文对话框（不依赖 WebView），提供官方下载地址与
//! 「重试 / 取消」，避免白屏或闪退。独立 Web 服务不加载本模块。
//!
//! 检测方式：微软支持的 EdgeUpdate Clients 注册表（用户范围 + 机器 64/32 位视图）的
//! `pv` 版本值。不以 Edge 浏览器存在、单一目录或退出码作为就绪判据。

use std::ffi::c_void;
use windows_sys::Win32::System::Registry::{
    RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY,
    RRF_SUBKEY_WOW6464KEY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDRETRY, MB_ICONERROR, MB_RETRYCANCEL,
};

/// WebView2 Runtime 的 EdgeUpdate 产品 GUID（微软官方固定值）。
const WEBVIEW2_RUNTIME_GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// 官方 WebView2 Runtime 下载地址。
const DOWNLOAD_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// ⚠️ release-blocker：最低版本在 T11 按锁定依赖、所用 Web API 与干净机验收确定后
/// 再填写并启用比较；当前只判「是否安装正式 Runtime」，不设下限，避免误拒可用版本。
// const MIN_WEBVIEW2_VERSION: &str = "待定";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 读指定根键下 EdgeUpdate Clients 的 `pv`（版本字符串）。
fn read_runtime_version(hkey: isize, flags: u32, subkey: &str) -> Option<String> {
    let subkey = wide(subkey);
    let value = wide("pv");
    let mut ty = 0u32;
    let mut buf = [0u16; 128];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY：subkey/value 为 NUL 结尾 UTF-16；buf/len 指向有效内存。
    let r = unsafe {
        RegGetValueW(
            hkey,
            subkey.as_ptr(),
            value.as_ptr(),
            flags | RRF_RT_REG_SZ,
            &mut ty,
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
        )
    };
    if r != 0 {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1).min(buf.len());
    Some(
        String::from_utf16_lossy(&buf[..chars])
            .trim_end_matches('\0')
            .to_string(),
    )
}

/// 检测正式 Evergreen Runtime，返回版本；未安装返回 None。
pub fn detect_webview2() -> Option<String> {
    let user = format!("Software\\Microsoft\\EdgeUpdate\\Clients\\{WEBVIEW2_RUNTIME_GUID}");
    if let Some(v) = read_runtime_version(HKEY_CURRENT_USER, 0, &user) {
        return Some(v);
    }
    let machine = format!("SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{WEBVIEW2_RUNTIME_GUID}");
    if let Some(v) = read_runtime_version(HKEY_LOCAL_MACHINE, RRF_SUBKEY_WOW6464KEY, &machine) {
        return Some(v);
    }
    read_runtime_version(HKEY_LOCAL_MACHINE, RRF_SUBKEY_WOW6432KEY, &machine)
}

/// 阻塞式启动前检查：未检测到 Runtime 时弹原生对话框，重试直到可用或用户取消。
pub fn ensure_webview2_or_prompt() {
    loop {
        if let Some(v) = detect_webview2() {
            eprintln!("[WebView2] 已检测到正式 Runtime，版本 {v}");
            return;
        }
        let title = wide("AiAccountCenter 无法启动");
        let text = wide(&format!(
            "未检测到 WebView2 运行时（Microsoft Edge WebView2 Runtime）。\n\n\
             桌面版依赖该组件才能显示界面。请从下方官方地址下载安装后，点击「重试」重新检测：\n\n\
             {DOWNLOAD_URL}"
        ));
        // SAFETY：title/text 为 NUL 结尾 UTF-16；hwnd=0 表示无父窗口。
        let ret = unsafe { MessageBoxW(0, text.as_ptr(), title.as_ptr(), MB_ICONERROR | MB_RETRYCANCEL) };
        if ret != IDRETRY {
            std::process::exit(1);
        }
    }
}
