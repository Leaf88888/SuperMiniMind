use std::collections::HashMap;

use eframe::egui::{
    Color32, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2,
};

use crate::core::mindmap::MindMap;
use crate::core::node::NodeId;
use crate::core::layout::NodeLayout;

/// 可选字体列表 — (font_id, key, display_name)
pub const FONT_OPTIONS: &[(u8, &str, &str)] = &[
    (0, "Proportional", "默认"),
    (1, "Monospace", "等宽"),
    (2, "msyh", "微软雅黑"),
    (3, "simhei", "黑体"),
];

/// 将 font_id 转为 egui FontFamily
pub fn font_id_to_family(id: u8) -> FontFamily {
    match id {
        0 => FontFamily::Proportional,
        1 => FontFamily::Monospace,
        2 => FontFamily::Name("msyh".into()),
        3 => FontFamily::Name("simhei".into()),
        _ => FontFamily::Proportional,
    }
}

/// 获取 font_id 对应的显示名称
pub fn font_id_to_name(id: u8) -> &'static str {
    FONT_OPTIONS
        .iter()
        .find(|(fid, _, _)| *fid == id)
        .map(|(_, _, name)| *name)
        .unwrap_or("默认")
}

/// 节点文字最大宽度（超过则自动换行），世界坐标
const MAX_TEXT_WIDTH: f32 = 200.0;

/// 视口 — 控制画布的平移和缩放
#[derive(Clone, Debug)]
pub struct Viewport {
    /// 画布偏移（屏幕坐标）
    pub offset: Vec2,
    /// 缩放比例
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset: Vec2::new(100.0, 300.0),
            zoom: 1.0,
        }
    }
}

impl Viewport {
    /// 世界坐标 → 屏幕坐标
    pub fn world_to_screen(&self, pos: Pos2) -> Pos2 {
        Pos2::new(
            pos.x * self.zoom + self.offset.x,
            pos.y * self.zoom + self.offset.y,
        )
    }

    /// 屏幕坐标 → 世界坐标
    pub fn screen_to_world(&self, pos: Pos2) -> Pos2 {
        Pos2::new(
            (pos.x - self.offset.x) / self.zoom,
            (pos.y - self.offset.y) / self.zoom,
        )
    }

    /// 缩放（以鼠标位置为中心）
    pub fn zoom_at(&mut self, mouse_pos: Pos2, delta: f32) {
        let world_before = self.screen_to_world(mouse_pos);
        self.zoom = (self.zoom * delta).clamp(0.2, 5.0);
        let world_after = self.screen_to_world(mouse_pos);
        let diff = world_after - world_before;
        self.offset.x -= diff.x * self.zoom;
        self.offset.y -= diff.y * self.zoom;
    }
}

/// 检查节点是否在可见区域内
fn is_on_screen(layout: &NodeLayout, viewport: &Viewport, clip: &Rect) -> bool {
    let sp = viewport.world_to_screen(layout.pos);
    let sw = layout.size.0 * viewport.zoom;
    let sh = layout.size.1 * viewport.zoom;
    let rect = Rect::from_min_size(sp, Vec2::new(sw, sh));
    clip.intersects(rect)
}

/// 渲染整个思维导图
pub fn render_mindmap(
    map: &MindMap,
    layouts: &HashMap<NodeId, NodeLayout>,
    viewport: &Viewport,
    painter: &Painter,
    selected_id: Option<NodeId>,
    editing_id: Option<NodeId>,
    multi_selected: &[NodeId],
    hovered_id: Option<NodeId>,
    clip_rect: Rect,
) {
    let visible = map.visible_nodes();

    // 1. 先绘制所有连线（在节点下方）— 跳过不可见的
    for &id in &visible {
        if let Some(layout) = layouts.get(&id) {
            if !is_on_screen(layout, viewport, &clip_rect) {
                continue;
            }
            let node = match map.get(id) {
                Some(n) => n,
                None => continue,
            };
            let parent_depth = map.depth(id);
            for &child_id in &node.children {
                if let Some(child_layout) = layouts.get(&child_id) {
                    if !is_on_screen(child_layout, viewport, &clip_rect) {
                        continue;
                    }
                    // 连线颜色用子节点所属分支的原始彩虹色
                    let conn_color = map.branch_color_of(child_id);
                    draw_connection(
                        painter,
                        viewport,
                        layout,
                        child_layout,
                        conn_color,
                        parent_depth,
                    );
                }
            }
        }
    }

    // 2. 绘制所有节点底色+文字（不含选中高亮）— 跳过不可见的
    for &id in &visible {
        if let Some(layout) = layouts.get(&id) {
            if !is_on_screen(layout, viewport, &clip_rect) {
                continue;
            }
            let node = match map.get(id) {
                Some(n) => n,
                None => continue,
            };
            let depth = map.depth(id);
            let is_editing = editing_id == Some(id);
            let has_children = !node.children.is_empty();
            let collapsed = node.collapsed;
            let is_hovered = hovered_id == Some(id);
            draw_node(
                painter,
                viewport,
                layout,
                &node.text,
                &node.style,
                depth,
                false,
                is_editing,
                false,
                has_children,
                collapsed,
                is_hovered,
            );
        }
    }

    // 3. 最后绘制所有选中/多选高亮（在节点上方，仅边框）
    for &id in &visible {
        if let Some(layout) = layouts.get(&id) {
            if !is_on_screen(layout, viewport, &clip_rect) {
                continue;
            }
            let is_selected = selected_id == Some(id);
            let is_multi = multi_selected.contains(&id);
            if is_selected || is_multi {
                let depth = map.depth(id);
                draw_highlight(painter, viewport, layout, depth, is_selected, is_multi);
            }
        }
    }
}

/// 根据深度获取圆角大小
fn corner_radius_for_depth(depth: usize) -> CornerRadius {
    match depth {
        0 => CornerRadius::same(10),
        1 => CornerRadius::same(8),
        _ => CornerRadius::same(6),
    }
}

/// 根据深度获取连线粗细
fn stroke_width_for_depth(depth: usize) -> f32 {
    match depth {
        0 => 3.0, // root→level1 最粗
        1 => 2.0, // level1→level2 中等
        _ => 1.5, // 更深更细
    }
}

/// 绘制节点（底色+文字，不含选中高亮）
fn draw_node(
    painter: &Painter,
    viewport: &Viewport,
    layout: &NodeLayout,
    text: &str,
    style: &crate::core::node::NodeStyle,
    depth: usize,
    _is_selected: bool,
    is_editing: bool,
    _is_multi: bool,
    has_children: bool,
    collapsed: bool,
    is_hovered: bool,
) {
    let screen_pos = viewport.world_to_screen(layout.pos);
    let screen_w = layout.size.0 * viewport.zoom;
    let screen_h = layout.size.1 * viewport.zoom;
    let corner = corner_radius_for_depth(depth);

    let rect = Rect::from_min_size(screen_pos, Vec2::new(screen_w, screen_h));

    // 悬浮高亮（仅中心节点，绘制在节点下方作为边框光晕）
    if is_hovered {
        let hover_rect = Rect::from_min_size(
            screen_pos - Vec2::new(3.0, 3.0),
            Vec2::new(screen_w + 6.0, screen_h + 6.0),
        );
        painter.rect(
            hover_rect,
            corner,
            Color32::from_rgba_unmultiplied(100, 120, 150, 25),
            Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(100, 120, 150, 120)),
            egui::StrokeKind::Inside,
        );
    }

    // 柔和阴影（多层叠加，由浅到深）
    let shadow_offset = if depth <= 1 { 3.0 } else { 2.0 };
    let shadow_alpha = if depth == 0 { 28 } else if depth == 1 { 18 } else { 10 };
    // 外层大范围浅阴影
    let shadow_rect1 = Rect::from_min_size(
        screen_pos + Vec2::new(shadow_offset * 0.3, shadow_offset),
        Vec2::new(screen_w + 2.0, screen_h + 2.0),
    );
    painter.rect_filled(
        shadow_rect1,
        corner,
        Color32::from_black_alpha(shadow_alpha / 3),
    );
    // 内层紧凑深阴影
    let shadow_rect2 = Rect::from_min_size(
        screen_pos + Vec2::new(shadow_offset * 0.5, shadow_offset),
        Vec2::new(screen_w, screen_h),
    );
    painter.rect_filled(
        shadow_rect2,
        corner,
        Color32::from_black_alpha(shadow_alpha),
    );

    // 边框
    let border_stroke = if style.border {
        Stroke::new(1.0_f32, Color32::from_rgb(180, 180, 190))
    } else {
        Stroke::NONE
    };

    // 节点背景
    painter.rect(
        rect,
        corner,
        style.bg_color,
        border_stroke,
        egui::StrokeKind::Inside,
    );

    // 文字（编辑中不画，由 TextEdit 覆盖）
    if !is_editing && !text.is_empty() {
        // 量化字体大小到整数，避免缩放时产生大量唯一缓存条目（内存泄漏根因）
        let font = FontId::new(
            (style.font_size * viewport.zoom).round(),
            font_id_to_family(style.font_id),
        );
        let max_w = MAX_TEXT_WIDTH * viewport.zoom;
        let galley = painter.layout(text.to_string(), font, style.text_color, max_w);

        let text_pos = Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        );

        // 粗体：偏移 0.5px 重绘（伪粗体效果），复用同一 galley 避免 2x 缓存
        if style.bold {
            painter.galley(
                Pos2::new(text_pos.x + 0.5, text_pos.y),
                galley.clone(),
                style.text_color,
            );
        }

        painter.galley(text_pos, galley, style.text_color);
    }

    // +/- 折叠按钮（有子节点时在右侧显示）
    if has_children {
        let btn_size = 16.0_f32;
        let btn_center = Pos2::new(
            rect.right() + btn_size / 2.0 + 2.0,
            rect.center().y,
        );
        let btn_rect = Rect::from_center_size(btn_center, Vec2::new(btn_size, btn_size));

        painter.rect(
            btn_rect,
            egui::CornerRadius::same(8),
            Color32::from_rgb(240, 240, 245),
            Stroke::new(1.0_f32, Color32::from_rgb(180, 182, 188)),
            egui::StrokeKind::Inside,
        );

        let symbol = if collapsed { "+" } else { "\u{2212}" };
        let font = FontId::new(12.0, FontFamily::Proportional);
        let galley = painter.layout(symbol.to_string(), font, Color32::from_rgb(80, 82, 88), 20.0);
        let text_pos = Pos2::new(
            btn_center.x - galley.size().x / 2.0,
            btn_center.y - galley.size().y / 2.0,
        );
        painter.galley(text_pos, galley, Color32::from_rgb(80, 82, 88));
    }
}

/// 绘制选中/多选高亮（仅边框，不遮挡文字和节点内容）
fn draw_highlight(
    painter: &Painter,
    viewport: &Viewport,
    layout: &NodeLayout,
    depth: usize,
    is_selected: bool,
    is_multi: bool,
) {
    let screen_pos = viewport.world_to_screen(layout.pos);
    let screen_w = layout.size.0 * viewport.zoom;
    let screen_h = layout.size.1 * viewport.zoom;
    let corner = corner_radius_for_depth(depth);

    if is_selected {
        // 单选高亮：蓝色边框
        let highlight_rect = Rect::from_min_size(
            screen_pos - Vec2::new(4.0, 4.0),
            Vec2::new(screen_w + 8.0, screen_h + 8.0),
        );
        painter.rect(
            highlight_rect,
            corner,
            Color32::TRANSPARENT,
            Stroke::new(2.5_f32, Color32::from_rgb(33, 150, 243)),
            egui::StrokeKind::Inside,
        );
    } else if is_multi {
        // 多选高亮：绿色边框
        let highlight_rect = Rect::from_min_size(
            screen_pos - Vec2::new(3.0, 3.0),
            Vec2::new(screen_w + 6.0, screen_h + 6.0),
        );
        painter.rect(
            highlight_rect,
            corner,
            Color32::TRANSPARENT,
            Stroke::new(2.5_f32, Color32::from_rgb(76, 175, 80)),
            egui::StrokeKind::Inside,
        );
    }
}

/// 绘制连线（贝塞尔曲线）
fn draw_connection(
    painter: &Painter,
    viewport: &Viewport,
    parent_layout: &NodeLayout,
    child_layout: &NodeLayout,
    color: Color32,
    parent_depth: usize,
) {
    // 父节点右侧中点
    let p1 = viewport.world_to_screen(Pos2::new(
        parent_layout.pos.x + parent_layout.size.0,
        parent_layout.pos.y + parent_layout.size.1 / 2.0,
    ));

    // 子节点左侧中点
    let p2 = viewport.world_to_screen(Pos2::new(
        child_layout.pos.x,
        child_layout.pos.y + child_layout.size.1 / 2.0,
    ));

    // 控制点 — 水平偏移
    let dx = (p2.x - p1.x).abs() * 0.5;
    let cp1 = Pos2::new(p1.x + dx, p1.y);
    let cp2 = Pos2::new(p2.x - dx, p2.y);

    // 采样贝塞尔曲线点 — 自适应步数（近距少步、远距多步）
    let dist = (p2.x - p1.x).abs();
    let steps = if dist < 80.0 { 12 } else if dist < 200.0 { 18 } else { 24 };
    let mut points: Vec<Pos2> = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let pt = cubic_bezier(p1, cp1, cp2, p2, t);
        points.push(pt);
    }

    let stroke_width = (stroke_width_for_depth(parent_depth) * viewport.zoom).max(1.0);
    let stroke = Stroke::new(stroke_width, color);

    // 逐段绘制
    for window in points.windows(2) {
        let shape = Shape::line_segment([window[0], window[1]], stroke);
        painter.add(shape);
    }
}

/// 三次贝塞尔曲线计算
fn cubic_bezier(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, t: f32) -> Pos2 {
    let u = 1.0 - t;
    let tt = t * t;
    let uu = u * u;
    let uuu = uu * u;
    let ttt = tt * t;

    Pos2::new(
        uuu * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + ttt * p3.x,
        uuu * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + ttt * p3.y,
    )
}

/// 估算文本尺寸（支持自动换行，返回实际宽高）
pub fn measure_text(text: &str, font_size: f32, font_id: u8, painter: &Painter) -> (f32, f32) {
    let font = FontId::new(font_size, font_id_to_family(font_id));
    let galley = painter.layout(text.to_string(), font, Color32::BLACK, MAX_TEXT_WIDTH);
    (galley.size().x, galley.size().y)
}

/// 检测点是否在节点内
pub fn hit_test(
    screen_pos: Pos2,
    layouts: &HashMap<NodeId, NodeLayout>,
    viewport: &Viewport,
) -> Option<NodeId> {
    for (&id, layout) in layouts {
        let screen_pos_layout = viewport.world_to_screen(layout.pos);
        let w = layout.size.0 * viewport.zoom;
        let h = layout.size.1 * viewport.zoom;
        let rect = Rect::from_min_size(screen_pos_layout, Vec2::new(w, h));
        if rect.contains(screen_pos) {
            return Some(id);
        }
    }
    None
}

/// 检测点击是否在节点的折叠按钮（+/-）上
pub fn hit_test_collapse_button(
    screen_pos: Pos2,
    layouts: &HashMap<NodeId, NodeLayout>,
    viewport: &Viewport,
    map: &MindMap,
) -> Option<NodeId> {
    let btn_size = 16.0_f32;
    for (&id, layout) in layouts {
        if let Some(node) = map.get(id) {
            if node.children.is_empty() {
                continue;
            }
            let sp = viewport.world_to_screen(layout.pos);
            let w = layout.size.0 * viewport.zoom;
            let h = layout.size.1 * viewport.zoom;
            let btn_center = Pos2::new(
                sp.x + w + btn_size / 2.0 + 2.0,
                sp.y + h / 2.0,
            );
            let btn_rect = Rect::from_center_size(btn_center, Vec2::new(btn_size, btn_size));
            if btn_rect.contains(screen_pos) {
                return Some(id);
            }
        }
    }
    None
}
