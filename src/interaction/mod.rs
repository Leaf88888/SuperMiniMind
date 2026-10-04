// 交互模块 — 当前交互逻辑内联在 app.rs 中
// 未来可将鼠标/键盘/拖拽逻辑抽取到此处

/// 编辑模式状态
#[derive(Clone, Debug, PartialEq)]
pub enum EditMode {
    /// 未编辑
    None,
    /// 正在编辑文本
    Editing(NodeId),
}

/// 鼠标操作类型
#[derive(Clone, Debug)]
pub enum MouseAction {
    Click(Pos2),
    DoubleClick(Pos2),
    RightClick(Pos2),
    Drag { from: Pos2, to: Pos2 },
    Scroll(f32),
}

use eframe::egui::Pos2;
use crate::core::node::NodeId;
