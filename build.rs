fn main() {
    // 仅 Windows 嵌入图标和版本信息
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        res.set("FileDescription", "SuperMiniMind - 轻量级思维导图");
        res.set("ProductName", "SuperMiniMind");
        res.set("LegalCopyright", "MIT");
        if let Err(e) = res.compile() {
            eprintln!("warning: winres compile failed: {}", e);
        }
    }
}
