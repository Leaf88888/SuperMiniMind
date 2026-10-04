use std::collections::HashMap;

use eframe::egui::Pos2;

use super::mindmap::MindMap;
use super::node::NodeId;

/// 布局方向
#[derive(Clone, Debug)]
pub enum LayoutDirection {
    /// 左→右展开（经典思维导图）
    LeftToRight,
    /// 上→下展开（组织架构图）
    TopToBottom,
}

/// 布局配置
#[derive(Clone, Debug)]
pub struct LayoutConfig {
    pub direction: LayoutDirection,
    /// 同级节点间距
    pub node_spacing: f32,
    /// 层级间距
    pub level_spacing: f32,
    /// 节点内边距
    pub padding_x: f32,
    pub padding_y: f32,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            direction: LayoutDirection::LeftToRight,
            node_spacing: 10.0,
            level_spacing: 200.0,
            padding_x: 16.0,
            padding_y: 8.0,
        }
    }
}

/// 节点布局尺寸信息
#[derive(Clone, Debug, Default)]
pub struct NodeLayout {
    pub pos: Pos2,
    pub size: (f32, f32),
    /// 子树占据的垂直范围
    pub subtree_top: f32,
    pub subtree_bottom: f32,
}

/// 计算所有节点的布局
pub fn layout_tree(
    map: &mut MindMap,
    config: &LayoutConfig,
    text_sizes: &HashMap<NodeId, (f32, f32)>,
) -> HashMap<NodeId, NodeLayout> {
    let mut layouts: HashMap<NodeId, NodeLayout> = HashMap::new();

    match config.direction {
        LayoutDirection::LeftToRight => {
            layout_left_to_right(map, config, text_sizes, &mut layouts);
        }
        LayoutDirection::TopToBottom => {
            layout_top_to_bottom(map, config, text_sizes, &mut layouts);
        }
    }

    // 回填位置到 map.positions
    for (id, layout) in &layouts {
        map.positions.insert(*id, layout.pos);
    }

    layouts
}

/// 左→右布局算法
fn layout_left_to_right(
    map: &MindMap,
    config: &LayoutConfig,
    text_sizes: &HashMap<NodeId, (f32, f32)>,
    layouts: &mut HashMap<NodeId, NodeLayout>,
) {
    // 从根节点开始递归布局
    let root = map.root;
    let start_x = 50.0;
    let start_y = 0.0; // 会被调整

    // 第一遍：计算每个子树的高度
    let total_height = compute_subtree_height(map, root, config, text_sizes, layouts);

    // 第二遍：分配位置
    let center_y = start_y + total_height / 2.0;
    assign_positions(
        map,
        root,
        start_x,
        center_y,
        config,
        text_sizes,
        layouts,
        0,
    );
}

/// 递归计算子树高度，并填充 NodeLayout 的 size 和 subtree 范围
fn compute_subtree_height(
    map: &MindMap,
    id: NodeId,
    config: &LayoutConfig,
    text_sizes: &HashMap<NodeId, (f32, f32)>,
    layouts: &mut HashMap<NodeId, NodeLayout>,
) -> f32 {
    let (tw, th) = text_sizes.get(&id).copied().unwrap_or((80.0, 24.0));
    let node_w = tw + config.padding_x * 2.0;
    let node_h = th + config.padding_y * 2.0;

    let children = map.visible_children(id);

    if children.is_empty() {
        let layout = NodeLayout {
            pos: Pos2::ZERO,
            size: (node_w, node_h),
            subtree_top: 0.0,
            subtree_bottom: node_h,
        };
        layouts.insert(id, layout);
        return node_h;
    }

    let mut total_h = 0.0;
    for &child_id in &children {
        let child_h = compute_subtree_height(map, child_id, config, text_sizes, layouts);
        total_h += child_h + config.node_spacing;
    }
    total_h -= config.node_spacing; // 最后一个不需要间距

    let layout = NodeLayout {
        pos: Pos2::ZERO,
        size: (node_w, node_h),
        subtree_top: 0.0,
        subtree_bottom: total_h.max(node_h),
    };
    layouts.insert(id, layout);

    total_h.max(node_h)
}

/// 递归分配位置 — 使用已缓存的子树高度，避免重复计算 O(n²)→O(n)
#[allow(clippy::too_many_arguments)]
fn assign_positions(
    map: &MindMap,
    id: NodeId,
    x: f32,
    center_y: f32,
    config: &LayoutConfig,
    text_sizes: &HashMap<NodeId, (f32, f32)>,
    layouts: &mut HashMap<NodeId, NodeLayout>,
    _depth: usize,
) {
    let (tw, th) = text_sizes.get(&id).copied().unwrap_or((80.0, 24.0));
    let node_w = tw + config.padding_x * 2.0;
    let node_h = th + config.padding_y * 2.0;

    let children = map.visible_children(id);

    if children.is_empty() {
        let layout = NodeLayout {
            pos: Pos2::new(x, center_y - node_h / 2.0),
            size: (node_w, node_h),
            subtree_top: center_y - node_h / 2.0,
            subtree_bottom: center_y + node_h / 2.0,
        };
        layouts.insert(id, layout);
        return;
    }

    // 读取已缓存的子树高度（第一遍 compute_subtree_height 已计算）
    let mut child_heights = Vec::with_capacity(children.len());
    let mut total_h = 0.0;
    for &child_id in &children {
        let h = layouts.get(&child_id).map(|l| l.subtree_bottom).unwrap_or(node_h);
        child_heights.push(h);
        total_h += h + config.node_spacing;
    }
    total_h -= config.node_spacing;

    let mut current_y = center_y - total_h / 2.0;

    for (i, &child_id) in children.iter().enumerate() {
        let child_h = child_heights[i];
        let child_center = current_y + child_h / 2.0;
        let child_x = x + node_w + config.level_spacing;

        assign_positions(
            map,
            child_id,
            child_x,
            child_center,
            config,
            text_sizes,
            layouts,
            _depth + 1,
        );

        current_y += child_h + config.node_spacing;
    }

    let layout = NodeLayout {
        pos: Pos2::new(x, center_y - node_h / 2.0),
        size: (node_w, node_h),
        subtree_top: center_y - total_h / 2.0,
        subtree_bottom: center_y + total_h / 2.0,
    };
    layouts.insert(id, layout);
}

/// 上→下布局算法
fn layout_top_to_bottom(
    map: &MindMap,
    config: &LayoutConfig,
    text_sizes: &HashMap<NodeId, (f32, f32)>,
    layouts: &mut HashMap<NodeId, NodeLayout>,
) {
    // 与 left_to_right 类似，但方向为垂直
    // 简化：复用水平逻辑，交换 x/y
    let root = map.root;
    let start_y = 50.0;
    let start_x = 0.0;

    let total_width = compute_subtree_width(map, root, config, text_sizes, layouts);
    let center_x = start_x + total_width / 2.0;
    assign_positions_vertical(
        map,
        root,
        start_y,
        center_x,
        config,
        text_sizes,
        layouts,
        0,
    );
}

fn compute_subtree_width(
    map: &MindMap,
    id: NodeId,
    config: &LayoutConfig,
    text_sizes: &HashMap<NodeId, (f32, f32)>,
    layouts: &mut HashMap<NodeId, NodeLayout>,
) -> f32 {
    let (tw, th) = text_sizes.get(&id).copied().unwrap_or((80.0, 24.0));
    let node_w = tw + config.padding_x * 2.0;
    let _node_h = th + config.padding_y * 2.0;

    let children = map.visible_children(id);

    if children.is_empty() {
        let layout = NodeLayout {
            pos: Pos2::ZERO,
            size: (node_w, th + config.padding_y * 2.0),
            subtree_top: 0.0,
            subtree_bottom: 0.0,
        };
        layouts.insert(id, layout);
        return node_w;
    }

    let mut total_w = 0.0;
    for &child_id in &children {
        let w = compute_subtree_width(map, child_id, config, text_sizes, layouts);
        total_w += w + config.node_spacing;
    }
    total_w -= config.node_spacing;

    let layout = NodeLayout {
        pos: Pos2::ZERO,
        size: (node_w, th + config.padding_y * 2.0),
        subtree_top: 0.0,
        subtree_bottom: 0.0,
    };
    layouts.insert(id, layout);

    total_w.max(node_w)
}

/// 垂直分配位置 — 使用已缓存的子树宽度
#[allow(clippy::too_many_arguments)]
fn assign_positions_vertical(
    map: &MindMap,
    id: NodeId,
    y: f32,
    center_x: f32,
    config: &LayoutConfig,
    text_sizes: &HashMap<NodeId, (f32, f32)>,
    layouts: &mut HashMap<NodeId, NodeLayout>,
    _depth: usize,
) {
    let (tw, th) = text_sizes.get(&id).copied().unwrap_or((80.0, 24.0));
    let node_w = tw + config.padding_x * 2.0;
    let node_h = th + config.padding_y * 2.0;

    let children = map.visible_children(id);

    if children.is_empty() {
        let layout = NodeLayout {
            pos: Pos2::new(center_x - node_w / 2.0, y),
            size: (node_w, node_h),
            subtree_top: 0.0,
            subtree_bottom: 0.0,
        };
        layouts.insert(id, layout);
        return;
    }

    // 读取已缓存的子树宽度
    let mut child_widths = Vec::with_capacity(children.len());
    let mut total_w = 0.0;
    for &child_id in &children {
        let w = layouts.get(&child_id).map(|l| l.subtree_bottom).unwrap_or(node_w);
        child_widths.push(w);
        total_w += w + config.node_spacing;
    }
    total_w -= config.node_spacing;

    let mut current_x = center_x - total_w / 2.0;
    for (i, &child_id) in children.iter().enumerate() {
        let child_w = child_widths[i];
        let child_center = current_x + child_w / 2.0;
        let child_y = y + node_h + config.level_spacing;

        assign_positions_vertical(
            map,
            child_id,
            child_y,
            child_center,
            config,
            text_sizes,
            layouts,
            _depth + 1,
        );

        current_x += child_w + config.node_spacing;
    }

    let layout = NodeLayout {
        pos: Pos2::new(center_x - node_w / 2.0, y),
        size: (node_w, node_h),
        subtree_top: 0.0,
        subtree_bottom: 0.0,
    };
    layouts.insert(id, layout);
}
