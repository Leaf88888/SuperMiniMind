use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use crate::core::mindmap::MindMap;
use crate::core::node::NodeId;

// === Win32 文件对话框（替代 rfd，零外部依赖）===

#[repr(C)]
#[derive(Default)]
struct OpenFilenameW {
    l_struct_size: u32,
    hwnd_owner: *mut std::ffi::c_void,
    instance: *mut std::ffi::c_void,
    filter: *const u16,
    custom_filter: *mut u16,
    max_cust_filter: u32,
    filter_index: u32,
    file_name: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_file_title: u32,
    initial_dir: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    def_ext: *const u16,
    cust_data: isize,
    hook: *mut std::ffi::c_void,
    template_name: *const u16,
    pv_reserved: *mut std::ffi::c_void,
    dw_reserved: u32,
    flags_ex: u32,
}

const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
const OFN_EXPLORER: u32 = 0x0008_0000;

#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(lpofn: *mut OpenFilenameW) -> i32;
    fn GetSaveFileNameW(lpofn: *mut OpenFilenameW) -> i32;
}

fn wide_z(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// 文件对话框 — 打开（支持 .mmind 和 .xmind）
pub fn open_dialog() -> Option<PathBuf> {
    // 过滤器格式："显示名\0*.ext\0...\0\0"
    let filter_str = "MindMap (*.mmind;*.xmind;*.json)\0*.mmind;*.xmind;*.json\0All Files (*.*)\0*.*\0";
    let mut filter: Vec<u16> = std::ffi::OsStr::new(filter_str)
        .encode_wide()
        .collect();
    filter.push(0); // 双 null 终止

    let mut file_buf = vec![0u16; 260];

    let mut ofn = OpenFilenameW {
        l_struct_size: std::mem::size_of::<OpenFilenameW>() as u32,
        filter: filter.as_ptr(),
        file_name: file_buf.as_mut_ptr(),
        max_file: file_buf.len() as u32,
        flags: OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST | OFN_EXPLORER,
        ..Default::default()
    };

    let result = unsafe { GetOpenFileNameW(&mut ofn) };
    if result == 0 {
        None
    } else {
        Some(PathBuf::from(from_wide(&file_buf)))
    }
}

/// 文件对话框 — 保存（default_name 为默认文件名，通常为中心节点文本）
pub fn save_dialog(default_name: &str) -> Option<PathBuf> {
    let filter_str = "XMind (*.xmind)\0*.xmind\0MindMap (*.mmind)\0*.mmind\0All Files (*.*)\0*.*\0";
    let mut filter: Vec<u16> = std::ffi::OsStr::new(filter_str)
        .encode_wide()
        .collect();
    filter.push(0);

    let mut file_buf = vec![0u16; 260];
    // 预填默认文件名（中心节点文本 + .xmind）
    let safe_name = if default_name.is_empty() {
        "untitled.xmind".to_string()
    } else {
        format!("{}.xmind", default_name)
    };
    let default_name_wide = wide_z(&safe_name);
    let copy_len = default_name_wide.len().min(file_buf.len() - 1);
    file_buf[..copy_len].copy_from_slice(&default_name_wide[..copy_len]);

    let def_ext = wide_z("xmind");

    let mut ofn = OpenFilenameW {
        l_struct_size: std::mem::size_of::<OpenFilenameW>() as u32,
        filter: filter.as_ptr(),
        file_name: file_buf.as_mut_ptr(),
        max_file: file_buf.len() as u32,
        flags: OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_EXPLORER,
        def_ext: def_ext.as_ptr(),
        ..Default::default()
    };

    let result = unsafe { GetSaveFileNameW(&mut ofn) };
    if result == 0 {
        None
    } else {
        Some(PathBuf::from(from_wide(&file_buf)))
    }
}

// === 文件读写 ===

/// 保存脑图到文件（按扩展名自动选择格式）
pub fn save_mindmap(map: &MindMap, path: &PathBuf) -> Result<(), String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "xmind" => save_xmind(map, path),
        _ => {
            let json = serde_json::to_string_pretty(map)
                .map_err(|e| format!("序列化失败: {}", e))?;
            std::fs::write(path, json)
                .map_err(|e| format!("写入文件失败: {}", e))?;
            Ok(())
        }
    }
}

/// 保存为 .xmind 格式（ZIP + content.json + metadata.json + manifest.json）
fn save_xmind(map: &MindMap, path: &PathBuf) -> Result<(), String> {
    use std::io::Write;

    let topic = build_xmind_topic(map, map.root);
    let content = serde_json::json!([{
        "id": format!("sheet-{}", map.root),
        "class": "sheet",
        "title": "",
        "rootTopic": topic,
    }]);
    let json_str = serde_json::to_string_pretty(&content)
        .map_err(|e| format!("序列化失败: {}", e))?;

    // metadata.json
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let metadata = serde_json::json!({
        "creator": {
            "name": "SuperMiniMind",
            "version": "1.0.0"
        },
        "created": now,
        "modified": now
    });
    let metadata_str = serde_json::to_string_pretty(&metadata)
        .map_err(|e| format!("序列化 metadata 失败: {}", e))?;

    // manifest.json
    let manifest = serde_json::json!({
        "file-entries": {
            "content.json": {},
            "metadata.json": {}
        }
    });
    let manifest_str = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("序列化 manifest 失败: {}", e))?;

    let file = std::fs::File::create(path)
        .map_err(|e| format!("创建文件失败: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("content.json", options)
        .map_err(|e| format!("写入 ZIP 失败: {}", e))?;
    zip.write_all(json_str.as_bytes())
        .map_err(|e| format!("写入 content.json 失败: {}", e))?;

    zip.start_file("metadata.json", options)
        .map_err(|e| format!("写入 ZIP 失败: {}", e))?;
    zip.write_all(metadata_str.as_bytes())
        .map_err(|e| format!("写入 metadata.json 失败: {}", e))?;

    zip.start_file("manifest.json", options)
        .map_err(|e| format!("写入 ZIP 失败: {}", e))?;
    zip.write_all(manifest_str.as_bytes())
        .map_err(|e| format!("写入 manifest.json 失败: {}", e))?;

    zip.finish()
        .map_err(|e| format!("完成 ZIP 失败: {}", e))?;
    Ok(())
}

/// 递归构建 XMind topic JSON（含 id 和 class 字段）
fn build_xmind_topic(map: &MindMap, id: NodeId) -> serde_json::Value {
    let node = match map.get(id) {
        Some(n) => n,
        None => return serde_json::Value::Null,
    };
    let children: Vec<serde_json::Value> = node.children.iter()
        .map(|&cid| build_xmind_topic(map, cid))
        .collect();
    let mut topic = serde_json::json!({
        "id": format!("topic-{}", id),
        "class": "topic",
        "title": node.text,
    });
    if !children.is_empty() {
        topic["children"] = serde_json::json!({ "attached": children });
    }
    topic
}

/// 从文件加载脑图（自动识别 .mmind 和 .xmind 格式）
pub fn load_mindmap(path: &PathBuf) -> Result<MindMap, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "xmind" => load_xmind(path),
        _ => {
            let json = std::fs::read_to_string(path)
                .map_err(|e| format!("读取文件失败: {}", e))?;
            let map: MindMap = serde_json::from_str(&json)
                .map_err(|e| format!("反序列化失败: {}", e))?;
            Ok(map)
        }
    }
}

// === XMind 解析 ===

/// 从 .xmind 文件加载脑图
/// .xmind 本质是 ZIP 压缩包，内部 content.json 包含脑图结构
fn load_xmind(path: &PathBuf) -> Result<MindMap, String> {
    use std::io::Read;

    let file = std::fs::File::open(path)
        .map_err(|e| format!("打开文件失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("读取 ZIP 失败: {}", e))?;

    // 查找 content.json
    let mut content_json: Option<String> = None;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 ZIP 条目失败: {}", e))?;
        let name = entry.name().to_lowercase();
        if name.ends_with("content.json") {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| format!("读取 content.json 失败: {}", e))?;
            content_json = Some(content);
            break;
        }
    }

    let json_str =
        content_json.ok_or("XMind 文件中未找到 content.json，可能是不兼容的旧版本格式")?;

    parse_xmind_content(&json_str)
}

/// 解析 XMind content.json，转换为 MindMap
fn parse_xmind_content(json_str: &str) -> Result<MindMap, String> {
    let sheets: Vec<serde_json::Value> = serde_json::from_str(json_str)
        .map_err(|e| format!("解析 content.json 失败: {}", e))?;

    let first_sheet = sheets
        .first()
        .ok_or("XMind 文件中没有工作表")?;

    let root_topic = first_sheet
        .get("rootTopic")
        .ok_or("找不到根主题")?;

    let title = root_topic
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("中心主题");

    let mut map = MindMap::new(title);
    let root_id = map.root;

    if let Some(children) = root_topic.get("children") {
        if let Some(attached) = children.get("attached") {
            if let Some(arr) = attached.as_array() {
                for child in arr {
                    add_topic_to_map(&mut map, root_id, child);
                }
            }
        }
    }

    Ok(map)
}

/// 递归将 XMind topic 转换为 MindMap 节点
fn add_topic_to_map(map: &mut MindMap, parent_id: NodeId, topic: &serde_json::Value) {
    let title = topic
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("新节点");

    let child_id = map.add_child(parent_id, title);

    if let Some(children) = topic.get("children") {
        if let Some(attached) = children.get("attached") {
            if let Some(arr) = attached.as_array() {
                for child in arr {
                    add_topic_to_map(map, child_id, child);
                }
            }
        }
    }
}
