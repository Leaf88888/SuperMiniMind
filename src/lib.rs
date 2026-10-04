pub mod app;
pub mod core;
pub mod render;
pub mod persistence;
pub mod interaction;

pub fn log_step(msg: &str) {
    let log_path = std::env::current_exe()
        .map(|p| p.with_file_name("mindmap_debug.log"))
        .unwrap_or_else(|_| std::path::PathBuf::from("mindmap_debug.log"));
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{}\n", msg).as_bytes()));
}
