#![windows_subsystem = "windows"]

use mindmap::app::MindMapApp;
use eframe::egui;

fn main() {
    let result = run();

    if let Err(e) = result {
        let log_path = std::env::current_exe()
            .map(|p| p.with_file_name("mindmap_error.log"))
            .unwrap_or_else(|_| std::path::PathBuf::from("mindmap_error.log"));
        let _ = std::fs::write(&log_path, format!("{}\n", e));

        #[cfg(windows)]
        {
            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;
            use std::iter::once;

            let title: Vec<u16> = OsStr::new("SuperMiniMind")
                .encode_wide().chain(once(0)).collect();
            let msg_str = format!(
                "SuperMiniMind 启动失败!\n\n错误: {}\n\n日志: {}\n\n请更新显卡驱动后重试。",
                e, log_path.display()
            );
            let msg: Vec<u16> = OsStr::new(&msg_str)
                .encode_wide().chain(once(0)).collect();

            unsafe {
                unsafe extern "C" {
                    fn MessageBoxW(hwnd: *mut std::ffi::c_void, lp_text: *const u16, lp_caption: *const u16, u_type: u32) -> i32;
                }
                MessageBoxW(std::ptr::null_mut(), msg.as_ptr(), title.as_ptr(), 0x10);
            }
        }

        #[cfg(not(windows))]
        { eprintln!("MindMap error: {}", e); }

        std::process::exit(1);
    }
}

/// 启动应用：先试 Glow（OpenGL），失败则回退 WGPU（DX12 / WARP 软件渲染）
fn run() -> eframe::Result<()> {
    // 第一次尝试：Glow 渲染器（体积更小、性能更好）
    let glow_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        try_run(eframe::Renderer::Glow)
    }));

    match glow_result {
        Ok(Ok(())) => Ok(()),
        _ => {
            // Glow 失败（无 OpenGL 驱动）—— 回退 WGPU
            // WGPU 在 Windows 上优先用 DX12，无 GPU 时用 WARP 软件渲染
            try_run(eframe::Renderer::Wgpu)
        }
    }
}

fn try_run(renderer: eframe::Renderer) -> eframe::Result<()> {
    // 读取命令行参数：双击文件时 Windows 传入文件路径
    let args: Vec<String> = std::env::args().collect();
    let initial_file = args.get(1)
        .filter(|s| std::path::Path::new(s).is_file())
        .map(std::path::PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_position([0.0, 0.0])
            .with_maximized(true)
            .with_title("SuperMiniMind")
            .with_icon(generate_icon()),
        renderer,
        ..Default::default()
    };

    eframe::run_native(
        "SuperMiniMind",
        options,
        Box::new(move |cc| {
            let font_ids = setup_fonts(&cc.egui_ctx);
            Ok(Box::new(MindMapApp::with_fonts(font_ids, initial_file)))
        }),
    )
}

/// 程序化生成窗口图标：透明背景 + 红色 "X" 字母
fn generate_icon() -> egui::IconData {
    const S: u32 = 64;
    let mut rgba = vec![0u8; (S * S * 4) as usize]; // 透明背景（RGBA 全 0）

    // 红色 X：两条对角线，粗细 6px
    let red: [u8; 4] = [220, 53, 69, 255];
    let half = 3i32;
    let smax = S as i32 - 1;

    for i in 0..S as i32 {
        // 左上→右下
        for t in -half..=half {
            let x = (i + t).clamp(0, smax) as u32;
            let y = i as u32;
            let idx = ((y * S + x) * 4) as usize;
            rgba[idx..idx + 4].copy_from_slice(&red);
        }
        // 右上→左下
        for t in -half..=half {
            let x = (i + t).clamp(0, smax) as u32;
            let y = i as u32;
            let idx = ((y * S + (smax - x as i32) as u32) * 4) as usize;
            rgba[idx..idx + 4].copy_from_slice(&red);
        }
    }

    egui::IconData { rgba, width: S, height: S }
}

/// 加载系统中文字体 — 返回已加载的字体 ID 列表
fn setup_fonts(ctx: &egui::Context) -> Vec<u8> {
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded_ids = vec![0u8, 1u8]; // Proportional 和 Monospace 始终可用

    let font_files: &[(u8, &str, &str)] = &[
        (2, "msyh", "C:\\Windows\\Fonts\\msyh.ttc"),
        (3, "simhei", "C:\\Windows\\Fonts\\simhei.ttf"),
    ];

    for &(id, key, path) in font_files {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(key.to_owned(), egui::FontData::from_owned(data).into());
            fonts.families
                .entry(egui::FontFamily::Name((*key).into()))
                .or_default()
                .push(key.to_owned());
            loaded_ids.push(id);
        }
    }

    // 将第一个可用的中文字体加入 Proportional 和 Monospace（作为 CJK 回退）
    for &(_, key, _) in font_files {
        if fonts.font_data.contains_key(key) {
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.push(key.to_owned());
            }
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                family.push(key.to_owned());
            }
            break;
        }
    }

    ctx.set_fonts(fonts);
    loaded_ids
}
