use eframe::egui::{Color32, Pos2};
use serde::{Deserialize, Serialize};

/// 节点ID — arena-based，用索引代替指针
pub type NodeId = usize;

/// 彩虹色板 — 现代柔和风格，8 色循环
const RAINBOW: [Color32; 8] = [
    Color32::from_rgb(235, 87, 87),    // 柔红
    Color32::from_rgb(242, 153, 74),   // 暖橙
    Color32::from_rgb(241, 196, 15),   // 金黄
    Color32::from_rgb(39, 174, 96),    // 翠绿
    Color32::from_rgb(45, 179, 165),   // 青绿
    Color32::from_rgb(72, 156, 214),   // 海蓝
    Color32::from_rgb(155, 81, 224),   // 雅紫
    Color32::from_rgb(236, 64, 122),   // 玫粉
];

/// 根据分支索引获取彩虹色
pub fn branch_color(index: usize) -> Color32 {
    RAINBOW[index % RAINBOW.len()]
}

/// 将颜色向白色混合（变浅）
/// amount=0 原色，amount=1 纯白
pub fn lighten_color(color: Color32, amount: f32) -> Color32 {
    let r = color.r() as f32;
    let g = color.g() as f32;
    let b = color.b() as f32;
    Color32::from_rgb(
        (r + (255.0 - r) * amount) as u8,
        (g + (255.0 - g) * amount) as u8,
        (b + (255.0 - b) * amount) as u8,
    )
}

/// 节点样式
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeStyle {
    pub bg_color: Color32,
    pub text_color: Color32,
    pub font_size: f32,
    pub bold: bool,
    pub border: bool,
    #[serde(default)]
    pub font_id: u8,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            bg_color: Color32::from_rgb(240, 240, 245),
            text_color: Color32::from_rgb(30, 30, 35),
            font_size: 14.0,
            bold: false,
            border: false,
            font_id: 0,
        }
    }
}

/// 根据深度和分支索引计算节点样式（现代柔和风格）
pub fn style_for_node(depth: usize, branch_idx: Option<usize>) -> NodeStyle {
    match depth {
        0 => NodeStyle {
            // 中心节点：深色背景，大字粗体
            bg_color: Color32::from_rgb(32, 42, 51),
            text_color: Color32::WHITE,
            font_size: 20.0,
            bold: true,
            border: false,
            font_id: 0,
        },
        1 => {
            // 一级分支：彩虹色
            let color = branch_color(branch_idx.unwrap_or(0));
            NodeStyle {
                bg_color: color,
                text_color: Color32::WHITE,
                font_size: 16.0,
                bold: true,
                border: false,
                font_id: 0,
            }
        }
        2 => {
            // 二级：彩虹色浅色版
            let color = branch_color(branch_idx.unwrap_or(0));
            NodeStyle {
                bg_color: lighten_color(color, 0.68),
                text_color: Color32::from_rgb(40, 40, 50),
                font_size: 14.0,
                bold: false,
                border: false,
                font_id: 0,
            }
        }
        _ => {
            // 更深层：极浅色
            let color = branch_color(branch_idx.unwrap_or(0));
            NodeStyle {
                bg_color: lighten_color(color, 0.82),
                text_color: Color32::from_rgb(45, 45, 55),
                font_size: 13.0,
                bold: false,
                border: false,
                font_id: 0,
            }
        }
    }
}

/// 为节点计算默认样式（根据深度）— 保留兼容旧接口
pub fn default_style_for_depth(depth: usize) -> NodeStyle {
    style_for_node(depth, None)
}

/// 脑图节点
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MindNode {
    pub id: NodeId,
    pub text: String,
    pub children: Vec<NodeId>,
    pub parent: Option<NodeId>,
    pub style: NodeStyle,
    /// 自定义位置（None = 自动布局）
    pub custom_pos: Option<Pos2>,
    pub collapsed: bool,
}

impl MindNode {
    /// 创建新节点
    pub fn new(id: NodeId, text: impl Into<String>, parent: Option<NodeId>) -> Self {
        let depth = match parent {
            Some(_) => 1,
            None => 0,
        };
        Self {
            id,
            text: text.into(),
            children: Vec::new(),
            parent,
            style: default_style_for_depth(depth),
            custom_pos: None,
            collapsed: false,
        }
    }

    /// 是否为根节点
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    /// 是否为叶子节点
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// 获取所有可见子节点（考虑折叠）
    pub fn visible_children(&self) -> bool {
        !self.collapsed
    }
}
