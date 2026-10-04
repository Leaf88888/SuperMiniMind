use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{
    self, Color32, Context, Id, Key, Pos2, Rect, Sense, Stroke,
    Ui, Vec2,
};

use crate::core::layout::{LayoutConfig, layout_tree, NodeLayout};
use crate::core::mindmap::{MindMap, ClipboardNode};
use crate::core::node::NodeId;
use crate::persistence;
use crate::render::{font_id_to_family, font_id_to_name, hit_test, hit_test_collapse_button, measure_text, render_mindmap, Viewport};

/// 根据父节点计算新子节点的默认名
fn default_child_name(map: &MindMap, parent_id: NodeId) -> String {
    let depth = map.depth(parent_id) + 1;
    let sibling_index = map.get(parent_id).map(|n| n.children.len()).unwrap_or(0);
    match depth {
        1 => format!("分支主题 {}", sibling_index + 1),
        _ => format!("细分主题 {}", sibling_index + 1),
    }
}

/// 待执行的文件操作（保存确认弹窗后执行）
#[derive(Clone, Copy, PartialEq)]
enum PendingAction {
    Exit,
    CloseTab(usize),
}

/// 单个标签页 — 封装全部 per-tab 状态
struct Tab {
    map: MindMap,
    viewport: Viewport,
    layout_config: LayoutConfig,
    layouts: HashMap<NodeId, NodeLayout>,
    text_sizes: HashMap<NodeId, (f32, f32)>,
    layout_dirty: bool,

    // 交互状态
    selected: Option<NodeId>,
    multi_selected: Vec<NodeId>,
    editing: Option<NodeId>,
    edit_text: String,
    panning: bool,
    last_mouse: Pos2,
    pan_velocity: Vec2,
    box_select_start: Option<Pos2>,
    box_selecting: bool,
    root_dragging: bool,
    node_dragging: Option<NodeId>,
    file_path: Option<PathBuf>,
    dirty: bool,
    hovered_id: Option<NodeId>,
    center_requested: bool,
}

impl Tab {
    /// 创建新标签页（根节点 + 4 个二级子节点）
    fn new_empty() -> Self {
        let mut map = MindMap::new("中心主题");
        let root = map.root;
        map.add_child(root, "分支1");
        map.add_child(root, "分支2");
        map.add_child(root, "分支3");
        map.add_child(root, "分支4");
        Self {
            map,
            viewport: Viewport::default(),
            layout_config: LayoutConfig::default(),
            layouts: HashMap::with_capacity(4),
            text_sizes: HashMap::with_capacity(4),
            layout_dirty: true,
            selected: None,
            editing: None,
            edit_text: String::new(),
            panning: false,
            last_mouse: Pos2::ZERO,
            pan_velocity: Vec2::ZERO,
            multi_selected: Vec::new(),
            box_select_start: None,
            box_selecting: false,
            root_dragging: false,
            node_dragging: None,
            file_path: None,
            dirty: false,
            hovered_id: None,
            center_requested: false,
        }
    }

    /// 从已加载的脑图创建标签页
    fn from_map(map: MindMap, file_path: Option<PathBuf>) -> Self {
        let root = map.root;
        Self {
            map,
            viewport: Viewport::default(),
            layout_config: LayoutConfig::default(),
            layouts: HashMap::with_capacity(4),
            text_sizes: HashMap::with_capacity(4),
            layout_dirty: true,
            selected: Some(root),
            editing: None,
            edit_text: String::new(),
            panning: false,
            last_mouse: Pos2::ZERO,
            pan_velocity: Vec2::ZERO,
            multi_selected: Vec::new(),
            box_select_start: None,
            box_selecting: false,
            root_dragging: false,
            node_dragging: None,
            file_path,
            dirty: false,
            hovered_id: None,
            center_requested: false,
        }
    }

    /// 标签页标题
    fn tab_title(&self) -> String {
        let name = self.file_path.as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                self.map.get(self.map.root)
                    .map(|n| n.text.clone())
                    .unwrap_or("未命名".to_string())
            });
        if self.dirty {
            format!("● {}", name)
        } else {
            name
        }
    }

    /// 绘制背景点阵
    fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        let grid_size = 36.0 * self.viewport.zoom;
        if grid_size < 6.0 {
            return;
        }

        let start_x = self.viewport.offset.x % grid_size;
        let start_y = self.viewport.offset.y % grid_size;

        let color = Color32::from_rgb(224, 226, 232);
        let dot_r = (1.0 * self.viewport.zoom).max(0.8).min(1.5);

        let mut x = start_x;
        while x < rect.width() {
            let mut y = start_y;
            while y < rect.height() {
                let center = Pos2::new(rect.min.x + x, rect.min.y + y);
                painter.circle_filled(center, dot_r, color);
                y += grid_size;
            }
            x += grid_size;
        }
    }

    /// 更新文本尺寸缓存
    fn update_text_sizes(&mut self, painter: &egui::Painter) {
        let visible = self.map.visible_nodes();
        for id in visible {
            if let Some(node) = self.map.get(id) {
                let (w, h) = measure_text(&node.text, node.style.font_size, node.style.font_id, painter);
                self.text_sizes.insert(id, (w, h));
            }
        }
    }

    /// 处理鼠标交互
    fn handle_mouse(&mut self, ui: &Ui, response: &egui::Response, _rect: Rect) {
        let pos = response.hover_pos();

        // 更新悬浮节点（用于中心节点高亮）
        self.hovered_id = pos.and_then(|p| hit_test(p, &self.layouts, &self.viewport));

        // 鼠标滚轮缩放
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if scroll.y != 0.0 {
            if let Some(mouse_pos) = pos {
                let zoom_factor = (scroll.y * 0.003).exp();
                self.viewport.zoom_at(mouse_pos, zoom_factor);
            }
        }

        // 中键拖拽平移 + 惯性滑动
        if ui.input(|i| i.pointer.middle_down()) {
            if self.panning {
                if let Some(curr) = pos {
                    let delta = curr - self.last_mouse;
                    self.viewport.offset.x += delta.x;
                    self.viewport.offset.y += delta.y;
                    self.pan_velocity = delta;
                }
            }
            self.panning = true;
            if let Some(p) = pos {
                self.last_mouse = p;
            }
        } else {
            self.panning = false;
        }

        // 左键点击
        if response.clicked() {
            if let Some(click_pos) = response.interact_pointer_pos() {
                // 优先检测 +/- 折叠按钮
                let collapse_id = hit_test_collapse_button(click_pos, &self.layouts, &self.viewport, &self.map);
                if let Some(id) = collapse_id {
                    self.map.toggle_collapse(id);
                    self.layout_dirty = true;
                } else {
                    if self.editing.is_some() {
                        self.finish_editing();
                    }

                    let hit = hit_test(click_pos, &self.layouts, &self.viewport);
                    let shift = ui.input(|i| i.modifiers.shift);
                    match hit {
                        Some(id) => {
                            if shift {
                                if let Some(pos) = self.multi_selected.iter().position(|&x| x == id) {
                                    self.multi_selected.remove(pos);
                                } else {
                                    self.multi_selected.push(id);
                                }
                                self.selected = Some(id);
                            } else {
                                self.multi_selected.clear();
                                self.selected = Some(id);
                            }
                        }
                        None => {
                            self.multi_selected.clear();
                            self.selected = None;
                        }
                    }
                }
            }
        }

        // 左键拖拽 → 拖拽节点/中心主题平移/框选
        if response.drag_started() && ui.input(|i| i.pointer.primary_down()) {
            if let Some(start_pos) = response.interact_pointer_pos() {
                let hit = hit_test(start_pos, &self.layouts, &self.viewport);
                match hit {
                    Some(id) if id == self.map.root => {
                        self.root_dragging = true;
                        self.last_mouse = start_pos;
                    }
                    Some(id) => {
                        // 拖拽非根节点 → 改变父节点
                        self.node_dragging = Some(id);
                    }
                    None => {
                        self.box_select_start = Some(start_pos);
                        self.box_selecting = true;
                    }
                }
            }
        }

        // 中心主题拖拽中 — 用 latest_pos 替代 hover_pos，确保拖出画布边界时仍流畅
        if self.root_dragging {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(curr) = ui.input(|i| i.pointer.latest_pos()) {
                    let delta = curr - self.last_mouse;
                    self.viewport.offset.x += delta.x;
                    self.viewport.offset.y += delta.y;
                    self.pan_velocity = delta;
                    self.last_mouse = curr;
                }
            } else {
                self.root_dragging = false;
            }
        }

        // 节点拖拽中 — hovered_id 即为潜在放置目标
        if let Some(drag_id) = self.node_dragging {
            if !ui.input(|i| i.pointer.primary_down()) {
                // 松开 → 执行移动或排序
                self.node_dragging = None;
                if let Some(target) = self.hovered_id {
                    if target != drag_id && target != self.map.root
                        && self.map.get(drag_id).is_some()
                        && self.map.get(target).is_some()
                    {
                        let drag_parent = self.map.get(drag_id).and_then(|n| n.parent);
                        let target_parent = self.map.get(target).and_then(|n| n.parent);

                        if drag_parent == target_parent && drag_parent.is_some() {
                            // 同级排序 — 根据光标 Y 判断插入位置
                            let before = if let Some(cursor_pos) = pos {
                                if let Some(tgt_layout) = self.layouts.get(&target) {
                                    let tgt_center_y = self.viewport.world_to_screen(tgt_layout.pos).y
                                        + tgt_layout.size.1 * self.viewport.zoom * 0.5;
                                    cursor_pos.y < tgt_center_y
                                } else {
                                    true
                                }
                            } else {
                                true
                            };
                            self.map.reorder_sibling(drag_id, target, before);
                            self.layout_dirty = true;
                            self.dirty = true;
                        } else if !self.map.is_descendant(drag_id, target) {
                            // 跨父节点移动
                            self.map.move_node(drag_id, target);
                            self.layout_dirty = true;
                            self.dirty = true;
                        }
                    }
                }
            }
        }

        // 释放后惯性滑动
        if !self.panning && !self.root_dragging {
            let speed = self.pan_velocity.length();
            if speed > 0.5 {
                self.viewport.offset.x += self.pan_velocity.x;
                self.viewport.offset.y += self.pan_velocity.y;
                self.pan_velocity *= 0.88;
            } else {
                self.pan_velocity = Vec2::ZERO;
            }
        }

        if response.drag_stopped() {
            if self.box_selecting {
                let start = self.box_select_start;
                let end = response.interact_pointer_pos();
                if let (Some(s), Some(e)) = (start, end) {
                    let min = Pos2::new(s.x.min(e.x), s.y.min(e.y));
                    let max = Pos2::new(s.x.max(e.x), s.y.max(e.y));
                    let sel_rect = Rect::from_min_max(min, max);
                    let vp = &self.viewport;
                    let layouts = &self.layouts;
                    let hit_ids: Vec<NodeId> = layouts.iter()
                        .filter(|(_, l)| {
                            let sp = vp.world_to_screen(l.pos);
                            let sw = l.size.0 * vp.zoom;
                            let sh = l.size.1 * vp.zoom;
                            sel_rect.intersects(Rect::from_min_size(sp, Vec2::new(sw, sh)))
                        })
                        .map(|(&id, _)| id)
                        .collect();
                    if !hit_ids.is_empty() {
                        self.multi_selected = hit_ids;
                        self.selected = Some(self.multi_selected[0]);
                    }
                }
                self.box_selecting = false;
                self.box_select_start = None;
            }
            if self.root_dragging {
                self.root_dragging = false;
            }
            self.node_dragging = None;
        }

        // 双击进入编辑
        if response.double_clicked() {
            if let Some(click_pos) = response.interact_pointer_pos() {
                let hit = hit_test(click_pos, &self.layouts, &self.viewport);
                if let Some(id) = hit {
                    self.start_editing(id);
                }
            }
        }

        // 右键上下文菜单
        if response.secondary_clicked() {
            if let Some(click_pos) = response.interact_pointer_pos() {
                let hit = hit_test(click_pos, &self.layouts, &self.viewport);
                if let Some(id) = hit {
                    self.multi_selected.clear();
                    self.selected = Some(id);
                    self.show_context_menu(ui.ctx(), id, click_pos);
                }
            }
        }
    }

    /// 绘制并处理画布滚动条
    fn draw_scrollbars(&mut self, ui: &Ui, painter: &egui::Painter, canvas_rect: Rect) {
        if self.layouts.is_empty() {
            return;
        }

        let mut min = Pos2::new(f32::MAX, f32::MAX);
        let mut max = Pos2::new(f32::MIN, f32::MIN);
        for l in self.layouts.values() {
            min.x = min.x.min(l.pos.x);
            min.y = min.y.min(l.pos.y);
            max.x = max.x.max(l.pos.x + l.size.0);
            max.y = max.y.max(l.pos.y + l.size.1);
        }
        let c_min = Pos2::new(min.x - 50.0, min.y - 50.0);
        let c_max = Pos2::new(max.x + 50.0, max.y + 50.0);
        let content_w = (c_max.x - c_min.x).max(1.0);
        let content_h = (c_max.y - c_min.y).max(1.0);

        let sb = 8.0;
        let margin = 2.0;
        let cr = egui::CornerRadius::same(4);

        let vis_left = (-self.viewport.offset.x) / self.viewport.zoom;
        let vis_top = (-self.viewport.offset.y) / self.viewport.zoom;
        let vis_w = canvas_rect.width() / self.viewport.zoom;
        let vis_h = canvas_rect.height() / self.viewport.zoom;

        // 水平滚动条
        let thumb_w_ratio = (vis_w / content_w).clamp(0.05, 1.0);
        let need_h = thumb_w_ratio < 1.0;
        if need_h {
            let hsb_rect = Rect::from_min_size(
                Pos2::new(canvas_rect.left() + margin, canvas_rect.bottom() - sb - margin),
                Vec2::new(canvas_rect.width() - sb - margin * 3.0, sb),
            );
            painter.rect_filled(hsb_rect, cr, Color32::from_rgb(228, 230, 234));

            let track_w = hsb_rect.width();
            let thumb_w = track_w * thumb_w_ratio;
            let thumb_left_ratio = ((vis_left - c_min.x) / content_w).clamp(0.0, 1.0 - thumb_w_ratio);
            let thumb_x = hsb_rect.left() + thumb_left_ratio * (track_w - thumb_w);
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(thumb_x, hsb_rect.top()), Vec2::new(thumb_w, sb)),
                cr,
                Color32::from_rgb(168, 172, 180),
            );

            let h_resp = ui.interact(hsb_rect, Id::new("hscrollbar"), Sense::drag());
            if h_resp.dragged() {
                if let Some(mp) = ui.input(|i| i.pointer.hover_pos()) {
                    let rel = ((mp.x - hsb_rect.left()) / track_w).clamp(0.0, 1.0);
                    let target = (rel - thumb_w_ratio / 2.0).clamp(0.0, 1.0 - thumb_w_ratio);
                    let world_left = c_min.x + target * content_w;
                    self.viewport.offset.x = canvas_rect.left() - world_left * self.viewport.zoom;
                }
            }
        }

        // 垂直滚动条
        let thumb_h_ratio = (vis_h / content_h).clamp(0.05, 1.0);
        let need_v = thumb_h_ratio < 1.0;
        if need_v {
            let vsb_rect = Rect::from_min_size(
                Pos2::new(canvas_rect.right() - sb - margin, canvas_rect.top() + margin),
                Vec2::new(sb, canvas_rect.height() - sb - margin * 3.0),
            );
            painter.rect_filled(vsb_rect, cr, Color32::from_rgb(228, 230, 234));

            let track_h = vsb_rect.height();
            let thumb_h = track_h * thumb_h_ratio;
            let thumb_top_ratio = ((vis_top - c_min.y) / content_h).clamp(0.0, 1.0 - thumb_h_ratio);
            let thumb_y = vsb_rect.top() + thumb_top_ratio * (track_h - thumb_h);
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(vsb_rect.left(), thumb_y), Vec2::new(sb, thumb_h)),
                cr,
                Color32::from_rgb(168, 172, 180),
            );

            let v_resp = ui.interact(vsb_rect, Id::new("vscrollbar"), Sense::drag());
            if v_resp.dragged() {
                if let Some(mp) = ui.input(|i| i.pointer.hover_pos()) {
                    let rel = ((mp.y - vsb_rect.top()) / track_h).clamp(0.0, 1.0);
                    let target = (rel - thumb_h_ratio / 2.0).clamp(0.0, 1.0 - thumb_h_ratio);
                    let world_top = c_min.y + target * content_h;
                    self.viewport.offset.y = canvas_rect.top() - world_top * self.viewport.zoom;
                }
            }
        }
    }

    /// 显示右键上下文菜单
    fn show_context_menu(&mut self, ctx: &Context, id: NodeId, pos: Pos2) {
        let mut open = true;
        egui::Area::new(Id::new("context_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    if ui.button("添加子节点 (Tab)").clicked() {
                        let name = default_child_name(&self.map, id);
                        let new_id = self.map.add_child(id, &name);
                        self.layout_dirty = true;
                        self.selected = Some(new_id);
                        self.start_editing(new_id);
                        open = false;
                    }
                    if !self.map.get(id).map(|n| n.is_root()).unwrap_or(true) {
                        if ui.button("添加同级节点 (Enter)").clicked() {
                            if let Some(parent) = self.map.get(id).and_then(|n| n.parent) {
                                let name = default_child_name(&self.map, parent);
                                let new_id = self.map.add_child(parent, &name);
                                self.layout_dirty = true;
                                self.selected = Some(new_id);
                                self.start_editing(new_id);
                            }
                            open = false;
                        }
                    }
                    ui.separator();
                    if ui.button("编辑文本").clicked() {
                        self.start_editing(id);
                        open = false;
                    }
                    if ui.button("折叠/展开").clicked() {
                        self.map.toggle_collapse(id);
                        self.layout_dirty = true;
                        open = false;
                    }
                    ui.separator();
                    if !self.map.get(id).map(|n| n.is_root()).unwrap_or(true) {
                        if ui.button("删除节点 (Del)").clicked() {
                            self.map.delete_node(id);
                            self.layout_dirty = true;
                            self.selected = Some(self.map.root);
                            open = false;
                        }
                    }
                });
            });
        let _ = open;
    }

    /// 开始编辑节点
    fn start_editing(&mut self, id: NodeId) {
        self.finish_editing();
        if let Some(node) = self.map.get(id) {
            self.edit_text = node.text.clone();
            self.editing = Some(id);
        }
    }

    /// 完成编辑
    fn finish_editing(&mut self) {
        if let Some(id) = self.editing.take() {
            let text = std::mem::take(&mut self.edit_text);
            self.map.edit_text(id, &text);
            self.dirty = true;
            self.layout_dirty = true;
        }
    }

    /// 保存文件
    fn save_file(&mut self) {
        if let Some(ref path) = self.file_path {
            let _ = persistence::save_mindmap(&self.map, path);
            self.dirty = false;
            self.layout_dirty = true;
        } else {
            let root_text = self.map.get(self.map.root).map(|n| n.text.as_str()).unwrap_or("untitled");
            if let Some(path) = persistence::save_dialog(root_text) {
                self.file_path = Some(path.clone());
                let _ = persistence::save_mindmap(&self.map, &path);
                self.dirty = false;
                self.layout_dirty = true;
            }
        }
    }

    /// 适应窗口
    fn fit_to_view(&mut self) {
        if self.layouts.is_empty() {
            return;
        }

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for layout in self.layouts.values() {
            min_x = min_x.min(layout.pos.x);
            min_y = min_y.min(layout.pos.y);
            max_x = max_x.max(layout.pos.x + layout.size.0);
            max_y = max_y.max(layout.pos.y + layout.size.1);
        }

        self.viewport.offset = Vec2::new(-min_x * self.viewport.zoom + 50.0, -min_y * self.viewport.zoom + 50.0);
    }

    // === 节点导航 ===

    fn navigate_up(&self, id: NodeId) -> Option<NodeId> {
        let node = self.map.get(id)?;
        let parent_id = node.parent?;
        let parent = self.map.get(parent_id)?;
        let idx = parent.children.iter().position(|&c| c == id)?;
        if idx > 0 {
            Some(parent.children[idx - 1])
        } else {
            None
        }
    }

    fn navigate_down(&self, id: NodeId) -> Option<NodeId> {
        let node = self.map.get(id)?;
        let parent_id = node.parent?;
        let parent = self.map.get(parent_id)?;
        let idx = parent.children.iter().position(|&c| c == id)?;
        if idx < parent.children.len() - 1 {
            Some(parent.children[idx + 1])
        } else {
            None
        }
    }

    fn navigate_left(&self, id: NodeId) -> Option<NodeId> {
        self.map.get(id)?.parent
    }

    fn navigate_right(&self, id: NodeId) -> Option<NodeId> {
        let node = self.map.get(id)?;
        if !node.children.is_empty() {
            Some(node.children[0])
        } else {
            None
        }
    }
}

/// 主应用
pub struct MindMapApp {
    tabs: Vec<Tab>,
    active_tab: usize,

    // 全局状态
    clipboard: String,
    show_style_panel: bool,
    available_font_ids: Vec<u8>,

    // 对话框状态
    pending_action: Option<PendingAction>,
    show_save_prompt: bool,
    deferred_save: bool,
    need_clear_frame: bool,

    // 退出保存流程
    exit_queue: Vec<usize>,
    exit_current: Option<usize>,
    exit_save_deferred: bool,
    exit_force_close: bool,
}

impl Default for MindMapApp {
    fn default() -> Self {
        Self::with_fonts(vec![0u8, 1u8], None)
    }
}

impl MindMapApp {
    /// 构造应用，传入可用字体 ID 列表和可选初始文件
    pub fn with_fonts(font_ids: Vec<u8>, initial_file: Option<PathBuf>) -> Self {
        let tabs = if let Some(ref path) = initial_file {
            match persistence::load_mindmap(path) {
                Ok(map) => vec![Tab::from_map(map, initial_file.clone())],
                Err(_) => vec![],
            }
        } else {
            vec![]
        };
        let active_tab = if tabs.is_empty() { 0 } else { tabs.len() - 1 };
        Self {
            tabs,
            active_tab,
            clipboard: String::new(),
            show_style_panel: false,
            available_font_ids: font_ids,
            pending_action: None,
            show_save_prompt: false,
            deferred_save: false,
            need_clear_frame: false,
            exit_queue: Vec::new(),
            exit_current: None,
            exit_save_deferred: false,
            exit_force_close: false,
        }
    }

    /// 保存当前活动标签页
    fn save_active_tab(&mut self) {
        let active = self.active_tab;
        self.tabs[active].save_file();
    }

    /// 根据待执行操作保存相应标签页
    fn save_for_pending(&mut self) {
        match self.pending_action {
            Some(PendingAction::CloseTab(idx)) => {
                if idx < self.tabs.len() {
                    self.tabs[idx].save_file();
                }
            }
            _ => {} // Exit 由退出流程逐个处理
        }
    }

    /// 判断待保存操作是否需要弹出原生对话框
    fn pending_save_needs_dialog(&self) -> bool {
        match self.pending_action {
            Some(PendingAction::CloseTab(idx)) => {
                idx < self.tabs.len() && self.tabs[idx].file_path.is_none()
            }
            _ => false,
        }
    }

    /// 请求退出 — 收集所有脏标签页，逐个提示保存
    fn request_exit(&mut self, ctx: &Context) {
        if self.show_save_prompt || self.exit_force_close {
            return;
        }
        let any_dirty = self.tabs.iter().any(|t| t.dirty);
        if !any_dirty {
            self.exit_force_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.exit_queue = self.tabs.iter().enumerate()
                .filter(|(_, t)| t.dirty)
                .map(|(i, _)| i)
                .collect();
            self.exit_current = Some(self.exit_queue.remove(0));
            if let Some(idx) = self.exit_current {
                if idx < self.tabs.len() {
                    self.tabs[idx].editing = None;
                }
            }
            self.show_save_prompt = true;
            self.pending_action = Some(PendingAction::Exit);
        }
    }

    /// 推进到下一个脏标签页，或完成退出
    fn advance_exit(&mut self, ctx: &Context) {
        if self.exit_queue.is_empty() {
            self.exit_current = None;
            self.exit_force_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.exit_current = Some(self.exit_queue.remove(0));
            if let Some(idx) = self.exit_current {
                if idx < self.tabs.len() {
                    self.tabs[idx].editing = None;
                }
            }
            self.show_save_prompt = true;
        }
    }
}

impl eframe::App for MindMapApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 0a. 检测窗口关闭请求（X 按钮），逐个提示保存
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.exit_force_close && !self.show_save_prompt {
            let any_dirty = self.tabs.iter().any(|t| t.dirty);
            if any_dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.request_exit(ctx);
            }
        }

        // 0. 清空帧标记
        let mut just_cleared = false;
        if self.need_clear_frame {
            self.need_clear_frame = false;
            just_cleared = true;
            ctx.request_repaint();
        }

        // 1. 延迟保存
        if !just_cleared && self.deferred_save {
            self.deferred_save = false;
            let needs_clear = self.pending_save_needs_dialog();
            self.save_for_pending();
            if needs_clear {
                self.need_clear_frame = true;
                ctx.request_repaint();
                return;
            }
        }

        // 1b. 退出流程延迟保存（原生保存对话框）
        if !just_cleared && self.exit_save_deferred {
            self.exit_save_deferred = false;
            let idx_opt = self.exit_current;
            if let Some(idx) = idx_opt {
                if idx < self.tabs.len() {
                    let root_text = self.tabs[idx].map.get(self.tabs[idx].map.root)
                        .map(|n| n.text.as_str()).unwrap_or("untitled");
                    if let Some(path) = persistence::save_dialog(root_text) {
                        self.tabs[idx].file_path = Some(path.clone());
                        let _ = persistence::save_mindmap(&self.tabs[idx].map, &path);
                        self.tabs[idx].dirty = false;
                        self.tabs[idx].layout_dirty = true;
                    }
                }
            }
            self.advance_exit(ctx);
            ctx.request_repaint();
        }

        // 2. 执行待定操作
        if !just_cleared && self.pending_action.is_some() && !self.show_save_prompt {
            self.execute_pending_action(ctx);
            ctx.request_repaint();
        }

        // 3. 菜单栏
        self.render_menu(ctx);

        // 4. 标签栏
        self.render_tab_bar(ctx);

        // 5. 侧边栏（样式面板）
        if self.show_style_panel {
            self.render_style_panel(ctx);
        }

        // 6. 主画布
        if self.tabs.is_empty() {
            self.render_welcome(ctx);
        } else {
            self.render_canvas(ctx);
        }

        // 7. 键盘快捷键
        if !self.show_save_prompt && !self.tabs.is_empty() {
            self.handle_keyboard(ctx);
        }

        // 8. 保存确认弹窗
        if self.show_save_prompt {
            self.render_save_prompt(ctx);
        }

        // 9. 按需重绘
        let need_repaint = if self.tabs.is_empty() {
            self.show_save_prompt
        } else {
            let active = self.active_tab;
            let tab = &self.tabs[active];
            tab.panning || tab.editing.is_some() || self.show_save_prompt || tab.box_selecting || tab.root_dragging || tab.node_dragging.is_some() || tab.pan_velocity.length() > 0.5
        };
        if need_repaint {
            ctx.request_repaint();
        }
    }
}

impl MindMapApp {
    /// 渲染顶部菜单
    fn render_menu(&mut self, ctx: &Context) {
        let active = self.active_tab;
        let has_tabs = !self.tabs.is_empty();

        egui::TopBottomPanel::top("menu_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("文件", |ui| {
                    if ui.button("新建  Ctrl+N").clicked() {
                        if !self.show_save_prompt {
                            self.tabs.push(Tab::new_empty());
                            self.active_tab = self.tabs.len() - 1;
                        }
                        ui.close_menu();
                    }
                    if ui.button("打开  Ctrl+O").clicked() {
                        if !self.show_save_prompt {
                            if let Some(path) = persistence::open_dialog() {
                                match persistence::load_mindmap(&path) {
                                    Ok(map) => {
                                        self.tabs.push(Tab::from_map(map, Some(path)));
                                        self.active_tab = self.tabs.len() - 1;
                                    }
                                    Err(e) => {
                                        eprintln!("打开失败: {}", e);
                                    }
                                }
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_tabs, egui::Button::new("保存  Ctrl+S")).clicked() {
                        if !self.show_save_prompt && has_tabs {
                            self.save_active_tab();
                        }
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_tabs, egui::Button::new("另存为...  Ctrl+Shift+S")).clicked() {
                        if !self.show_save_prompt && has_tabs {
                            let root_text = self.tabs[active].map.get(self.tabs[active].map.root)
                                .map(|n| n.text.as_str()).unwrap_or("untitled");
                            if let Some(path) = persistence::save_dialog(root_text) {
                                self.tabs[active].file_path = Some(path.clone());
                                let _ = persistence::save_mindmap(&self.tabs[active].map, &path);
                                self.tabs[active].dirty = false;
                                self.tabs[active].layout_dirty = true;
                            }
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("退出").clicked() {
                        self.request_exit(ctx);
                        ui.close_menu();
                    }
                });

                if has_tabs {
                ui.menu_button("编辑", |ui| {
                    if ui.button("撤销  Ctrl+Z").clicked() {
                        let tab = &mut self.tabs[active];
                        tab.map.undo();
                        tab.dirty = true;
                        tab.layout_dirty = true;
                        ui.close_menu();
                    }
                    if ui.button("重做  Ctrl+Y").clicked() {
                        let tab = &mut self.tabs[active];
                        tab.map.redo();
                        tab.dirty = true;
                        tab.layout_dirty = true;
                        ui.close_menu();
                    }
                });

                ui.menu_button("视图", |ui| {
                    if ui.button("适应窗口").clicked() {
                        self.tabs[active].fit_to_view();
                        ui.close_menu();
                    }
                    if ui.button("回到中心").clicked() {
                        self.tabs[active].center_requested = true;
                        ui.close_menu();
                    }
                    if ui.button("放大  Ctrl++").clicked() {
                        self.tabs[active].viewport.zoom *= 1.2;
                        ui.close_menu();
                    }
                    if ui.button("缩小  Ctrl+-").clicked() {
                        self.tabs[active].viewport.zoom *= 0.8;
                        ui.close_menu();
                    }
                    if ui.button("重置缩放  Ctrl+0").clicked() {
                        self.tabs[active].viewport.zoom = 1.0;
                        ui.close_menu();
                    }
                });

                ui.menu_button("插入", |ui| {
                    if ui.button("添加子节点  Tab").clicked() {
                        let tab = &mut self.tabs[active];
                        if let Some(id) = tab.selected {
                            let name = default_child_name(&tab.map, id);
                            let new_id = tab.map.add_child(id, &name);
                            tab.layout_dirty = true;
                            tab.selected = Some(new_id);
                            tab.start_editing(new_id);
                        }
                        ui.close_menu();
                    }
                    if ui.button("添加同级节点  Enter").clicked() {
                        let tab = &mut self.tabs[active];
                        if let Some(id) = tab.selected {
                            if let Some(parent) = tab.map.get(id).and_then(|n| n.parent) {
                                let name = default_child_name(&tab.map, parent);
                                let new_id = tab.map.add_child(parent, &name);
                                tab.layout_dirty = true;
                                tab.selected = Some(new_id);
                                tab.start_editing(new_id);
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("删除节点  Del").clicked() {
                        let tab = &mut self.tabs[active];
                        if let Some(id) = tab.selected {
                            tab.map.delete_node(id);
                            tab.layout_dirty = true;
                            tab.selected = Some(tab.map.root);
                            tab.dirty = true;
                        }
                        ui.close_menu();
                    }
                });

                ui.menu_button("格式", |ui| {
                    if ui.button("样式面板").clicked() {
                        self.show_style_panel = !self.show_style_panel;
                        ui.close_menu();
                    }
                });
                }

                // 右侧状态
                if has_tabs {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 缩放百分比（可拖拽/输入，整数）
                    let mut zoom_val = self.tabs[active].viewport.zoom * 100.0;
                    let zoom_resp = ui.add(
                        egui::DragValue::new(&mut zoom_val)
                            .clamp_existing_to_range(true)
                            .range(10.0..=500.0)
                            .suffix("%")
                            .speed(0.2)
                            .fixed_decimals(0),
                    );
                    if zoom_resp.changed() {
                        self.tabs[active].viewport.zoom = (zoom_val.round() / 100.0).max(0.1);
                    }
                    ui.separator();
                    if self.tabs[active].dirty {
                        ui.label("● 未保存");
                    } else if let Some(ref path) = self.tabs[active].file_path {
                        if let Some(name) = path.file_name() {
                            let path_str = path.to_string_lossy().to_string();
                            let label_resp = ui.label(format!("{}", name.to_string_lossy()));
                            label_resp.context_menu(|ui| {
                                if ui.button("打开文件所在文件夹").clicked() {
                                    use std::os::windows::process::CommandExt;
                                    let _ = std::process::Command::new("explorer")
                                        .raw_arg(format!("/select,\"{}\"", path_str))
                                        .spawn();
                                    ui.close_menu();
                                }
                            });
                        }
                    } else {
                        ui.label("未命名");
                    }
                });
                }
            });
        });
    }

    /// 渲染标签栏
    fn render_tab_bar(&mut self, ctx: &Context) {
        let mut switch_to: Option<usize> = None;
        let mut close_idx: Option<usize> = None;
        let mut new_tab = false;

        egui::TopBottomPanel::top("tab_bar")
            .frame(egui::Frame::canvas(&ctx.style()).fill(Color32::from_rgb(240, 240, 245)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let tab_count = self.tabs.len();
                    for idx in 0..tab_count {
                        let is_active = idx == self.active_tab;
                        let title = self.tabs[idx].tab_title();
                        let tab_path = self.tabs[idx].file_path.clone();

                        let bg = if is_active {
                            Color32::from_rgb(255, 255, 255)
                        } else {
                            Color32::from_rgb(225, 228, 232)
                        };

                        let resp = ui.add(
                            egui::Button::new(title)
                                .fill(bg)
                                .corner_radius(egui::CornerRadius::same(4))
                                .min_size(Vec2::new(80.0, 20.0))
                                .stroke(if is_active {
                                    egui::Stroke::new(2.0_f32, Color32::from_rgb(33, 150, 243))
                                } else {
                                    egui::Stroke::NONE
                                }),
                        );
                        if resp.clicked() {
                            switch_to = Some(idx);
                        }

                        resp.context_menu(|ui| {
                            if let Some(ref path) = tab_path {
                                let path_str = path.to_string_lossy().to_string();
                                if ui.button("打开文件所在文件夹").clicked() {
                                    use std::os::windows::process::CommandExt;
                                    let _ = std::process::Command::new("explorer")
                                        .raw_arg(format!("/select,\"{}\"", path_str))
                                        .spawn();
                                    ui.close_menu();
                                }
                            } else {
                                ui.label("未保存");
                            }
                        });

                        let close_resp = ui.add(
                            egui::Button::new("×")
                                .fill(Color32::TRANSPARENT)
                                .corner_radius(egui::CornerRadius::same(4)),
                        );
                        if close_resp.clicked() {
                            close_idx = Some(idx);
                        }

                        ui.separator();
                    }

                    if ui.button("+").clicked() {
                        new_tab = true;
                    }
                });
            });

        // 在渲染完成后处理操作（避免迭代时修改 Vec）
        if let Some(idx) = switch_to {
            if idx != self.active_tab {
                let old = self.active_tab;
                self.tabs[old].finish_editing();
                self.tabs[old].text_sizes.clear();
                self.tabs[old].text_sizes.shrink_to_fit();
                self.tabs[old].layouts.clear();
                self.tabs[old].layouts.shrink_to_fit();
                self.tabs[old].layout_dirty = true;
                self.tabs[old].pan_velocity = Vec2::ZERO;
                self.tabs[old].box_selecting = false;
                self.tabs[old].box_select_start = None;
                self.tabs[old].root_dragging = false;
                self.tabs[old].panning = false;
                self.tabs[old].multi_selected.shrink_to_fit();
                self.tabs[old].map.shrink_to_fit();
            }
            self.active_tab = idx;
        }

        if let Some(idx) = close_idx {
            if self.tabs[idx].dirty {
                self.tabs[idx].editing = None;
                self.pending_action = Some(PendingAction::CloseTab(idx));
                self.show_save_prompt = true;
            } else {
                self.tabs.remove(idx);
                if self.tabs.is_empty() {
                    self.active_tab = 0;
                } else if idx < self.active_tab {
                    self.active_tab -= 1;
                } else if self.active_tab >= self.tabs.len() {
                    self.active_tab = self.tabs.len() - 1;
                }
            }
        }

        if new_tab {
            self.tabs.push(Tab::new_empty());
            self.active_tab = self.tabs.len() - 1;
        }
    }

    /// 渲染样式面板
    fn render_style_panel(&mut self, ctx: &Context) {
        let active = self.active_tab;
        let font_ids = self.available_font_ids.clone();

        egui::SidePanel::right("style_panel")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                // === 节点样式区域 ===
                let mut close_panel = false;
                ui.horizontal(|ui| {
                    ui.heading("节点样式");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(
                            egui::Button::new(egui::RichText::new("×").size(16.0))
                                .fill(Color32::TRANSPARENT)
                                .corner_radius(egui::CornerRadius::same(4))
                        ).clicked() {
                            close_panel = true;
                        }
                    });
                });
                if close_panel {
                    self.show_style_panel = false;
                }
                ui.separator();

                let selected = self.tabs[active].selected;
                if let Some(id) = selected {
                    if let Some(node) = self.tabs[active].map.get(id) {
                        let node_text = node.text.clone();
                        let node_style = node.style;

                        ui.label(format!("节点: {}", node_text));
                        ui.add_space(8.0);

                        // 文本内容编辑
                        ui.label("文本内容:");
                        let mut text = node_text.clone();
                        let resp = ui.text_edit_singleline(&mut text);
                        if resp.changed() {
                            self.tabs[active].map.edit_text(id, &text);
                            self.tabs[active].dirty = true;
                            self.tabs[active].layout_dirty = true;
                        }

                        ui.add_space(8.0);

                        // 字体大小
                        ui.label("字体大小:");
                        let mut font_size = node_style.font_size;
                        let resp = ui.add(egui::Slider::new(&mut font_size, 10.0..=32.0).suffix("px"));
                        if resp.changed() {
                            let mut style = node_style;
                            style.font_size = font_size;
                            self.tabs[active].map.edit_style(id, style);
                            self.tabs[active].dirty = true;
                            self.tabs[active].layout_dirty = true;
                        }

                        ui.add_space(8.0);

                        // 粗体
                        let mut bold = node_style.bold;
                        if ui.checkbox(&mut bold, "粗体").changed() {
                            let mut style = node_style;
                            style.bold = bold;
                            self.tabs[active].map.edit_style(id, style);
                            self.tabs[active].dirty = true;
                            self.tabs[active].layout_dirty = true;
                        }

                        ui.add_space(8.0);

                        // 背景色
                        ui.label("背景颜色:");
                        let mut bg = node_style.bg_color;
                        if ui.color_edit_button_srgba(&mut bg).changed() {
                            let mut style = node_style;
                            style.bg_color = bg;
                            self.tabs[active].map.edit_style(id, style);
                            self.tabs[active].dirty = true;
                            self.tabs[active].layout_dirty = true;
                        }

                        ui.add_space(8.0);

                        // 文字颜色
                        ui.label("文字颜色:");
                        let mut tc = node_style.text_color;
                        if ui.color_edit_button_srgba(&mut tc).changed() {
                            let mut style = node_style;
                            style.text_color = tc;
                            self.tabs[active].map.edit_style(id, style);
                            self.tabs[active].dirty = true;
                            self.tabs[active].layout_dirty = true;
                        }

                        ui.add_space(8.0);

                        // 边框
                        let mut border = node_style.border;
                        if ui.checkbox(&mut border, "显示边框").changed() {
                            let mut style = node_style;
                            style.border = border;
                            self.tabs[active].map.edit_style(id, style);
                            self.tabs[active].dirty = true;
                            self.tabs[active].layout_dirty = true;
                        }

                        ui.add_space(8.0);

                        // 字体
                        ui.label("字体:");
                        let mut font_id = node_style.font_id;
                        let mut font_changed = false;
                        egui::ComboBox::from_id_salt("font_combo")
                            .selected_text(font_id_to_name(font_id))
                            .show_ui(ui, |ui| {
                                for &fid in &font_ids {
                                    if ui.selectable_label(font_id == fid, font_id_to_name(fid)).clicked() {
                                        font_id = fid;
                                        font_changed = true;
                                    }
                                }
                            });
                        if font_changed {
                            let mut style = node_style;
                            style.font_id = font_id;
                            self.tabs[active].map.edit_style(id, style);
                            self.tabs[active].dirty = true;
                            self.tabs[active].layout_dirty = true;
                        }

                        ui.add_space(12.0);

                        // 预设主题
                        ui.label("预设主题:");
                        ui.add_space(4.0);
                        if ui.button("应用默认主题").clicked() {
                            let style = self.tabs[active].map.style_for(id);
                            self.tabs[active].map.edit_style(id, style);
                            self.tabs[active].dirty = true;
                            self.tabs[active].layout_dirty = true;
                        }
                    }
                } else {
                    ui.label("请先选择一个节点");
                }

                // === 间距设置区域 ===
                ui.add_space(16.0);
                ui.separator();
                ui.heading("间距设置");
                ui.add_space(4.0);

                // 水平间距
                ui.label("水平间距 (层级):");
                let mut level_spacing = self.tabs[active].layout_config.level_spacing;
                let resp = ui.add(egui::Slider::new(&mut level_spacing, 40.0..=800.0).suffix("px"));
                if resp.changed() {
                    self.tabs[active].layout_config.level_spacing = level_spacing;
                    self.tabs[active].layout_dirty = true;
                }

                ui.add_space(8.0);

                // 垂直间距
                ui.label("垂直间距 (同级):");
                let mut node_spacing = self.tabs[active].layout_config.node_spacing;
                let resp = ui.add(egui::Slider::new(&mut node_spacing, 2.0..=60.0).suffix("px"));
                if resp.changed() {
                    self.tabs[active].layout_config.node_spacing = node_spacing;
                    self.tabs[active].layout_dirty = true;
                }

                ui.add_space(8.0);

                // 自动匹配按钮
                if ui.button("自动匹配间距").clicked() {
                    let max_w = self.tabs[active].text_sizes.values().map(|(w, _)| *w).fold(0.0f32, f32::max);
                    let max_h = self.tabs[active].text_sizes.values().map(|(_, h)| *h).fold(0.0f32, f32::max);
                    self.tabs[active].layout_config.level_spacing = (max_w * 0.8 + 40.0).clamp(40.0, 800.0);
                    self.tabs[active].layout_config.node_spacing = (max_h * 0.4 + 4.0).clamp(2.0, 60.0);
                    self.tabs[active].layout_dirty = true;
                }

                ui.add_space(4.0);

                // 重置间距按钮
                if ui.button("恢复默认间距").clicked() {
                    self.tabs[active].layout_config.level_spacing = 200.0;
                    self.tabs[active].layout_config.node_spacing = 10.0;
                    self.tabs[active].layout_dirty = true;
                }
            });
    }

    /// 空状态欢迎界面
    fn render_welcome(&mut self, ctx: &Context) {
        let bg = Color32::from_rgb(248, 249, 251);
        egui::CentralPanel::default()
            .frame(egui::Frame::canvas(&ctx.style()).fill(bg))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.label(egui::RichText::new("SuperMiniMind").size(32.0).strong().color(Color32::from_rgb(60, 65, 80)));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("轻量级思维导图").size(14.0).color(Color32::from_rgb(130, 135, 150)));
                    ui.add_space(36.0);
                    let new_btn = egui::Button::new(
                        egui::RichText::new("  新建  Ctrl+N ").color(Color32::WHITE).strong()
                    ).fill(Color32::from_rgb(33, 150, 243))
                     .corner_radius(egui::CornerRadius::same(6));
                    if ui.add(new_btn).clicked() {
                        self.tabs.push(Tab::new_empty());
                        self.active_tab = self.tabs.len() - 1;
                    }
                    ui.add_space(10.0);
                    let open_btn = egui::Button::new(
                        egui::RichText::new("  打开  Ctrl+O ").color(Color32::from_rgb(80, 85, 100))
                    ).fill(Color32::from_rgb(235, 238, 242))
                     .corner_radius(egui::CornerRadius::same(6));
                    if ui.add(open_btn).clicked() {
                        if let Some(path) = persistence::open_dialog() {
                            match persistence::load_mindmap(&path) {
                                Ok(map) => {
                                    self.tabs.push(Tab::from_map(map, Some(path)));
                                    self.active_tab = self.tabs.len() - 1;
                                }
                                Err(e) => {
                                    eprintln!("打开失败: {}", e);
                                }
                            }
                        }
                    }
                });

                // 底部版本号
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("当前版本：v{}", env!("CARGO_PKG_VERSION")))
                            .size(12.0)
                            .color(Color32::from_rgb(180, 183, 190)),
                    );
                });
            });
    }

    /// 渲染主画布
    fn render_canvas(&mut self, ctx: &Context) {
        let active = self.active_tab;
        let canvas_bg = Color32::from_rgb(250, 251, 253);

        let panel_response = egui::CentralPanel::default()
            .frame(egui::Frame::canvas(&ctx.style()).fill(canvas_bg))
            .show(ctx, |ui| {
                let (rect, _response) = ui.allocate_exact_size(
                    ui.available_size(),
                    Sense::hover(),
                );

                let painter = ui.painter_at(rect);

                // 提取活动标签页可变引用
                let tab = &mut self.tabs[active];

                tab.draw_grid(&painter, rect);

                // 仅在结构/文字变化时重新计算
                if tab.layout_dirty {
                    tab.text_sizes.clear();
                    tab.update_text_sizes(&painter);
                    tab.layouts = layout_tree(&mut tab.map, &tab.layout_config, &tab.text_sizes);
                    tab.text_sizes.shrink_to_fit();
                    tab.layouts.shrink_to_fit();
                    tab.layout_dirty = false;
                }

                // 回到中心：将根节点居中显示
                if tab.center_requested {
                    tab.center_requested = false;
                    if let Some(layout) = tab.layouts.get(&tab.map.root).cloned() {
                        let root_cx = layout.pos.x + layout.size.0 / 2.0;
                        let root_cy = layout.pos.y + layout.size.1 / 2.0;
                        tab.viewport.offset.x = rect.center().x - root_cx * tab.viewport.zoom;
                        tab.viewport.offset.y = rect.center().y - root_cy * tab.viewport.zoom;
                    }
                }

                // 渲染思维导图
                render_mindmap(
                    &tab.map,
                    &tab.layouts,
                    &tab.viewport,
                    &painter,
                    tab.selected,
                    tab.editing,
                    &tab.multi_selected,
                    tab.hovered_id,
                    rect,
                );

                // 节点拖拽动效：幽灵预览 + 连接线 + 放置目标高亮
                if let Some(drag_id) = tab.node_dragging {
                    let time = ui.input(|i| i.time) as f32;
                    let pulse = 0.5 + 0.5 * (time * 4.0).sin();

                    // 1. 从原始位置到光标的虚线连接
                    if let Some(drag_layout) = tab.layouts.get(&drag_id) {
                        let orig_screen = tab.viewport.world_to_screen(drag_layout.pos)
                            + Vec2::new(
                                drag_layout.size.0 * tab.viewport.zoom * 0.5,
                                drag_layout.size.1 * tab.viewport.zoom * 0.5,
                            );
                        if let Some(cursor) = ui.input(|i| i.pointer.hover_pos()) {
                            painter.line_segment(
                                [orig_screen, cursor],
                                Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(120, 120, 140, 120)),
                            );
                        }
                    }

                    // 2. 幽灵预览：在光标位置绘制半透明节点
                    if let (Some(drag_layout), Some(cursor)) = (tab.layouts.get(&drag_id), ui.input(|i| i.pointer.hover_pos())) {
                        if let Some(node) = tab.map.get(drag_id) {
                            let gw = drag_layout.size.0 * tab.viewport.zoom;
                            let gh = drag_layout.size.1 * tab.viewport.zoom;
                            let ghost_rect = Rect::from_min_size(
                                cursor - Vec2::new(gw * 0.5, gh * 0.5),
                                Vec2::new(gw, gh),
                            );
                            let corner = if tab.map.depth(drag_id) <= 1 {
                                egui::CornerRadius::same(8)
                            } else {
                                egui::CornerRadius::same(4)
                            };
                            painter.rect(
                                ghost_rect,
                                corner,
                                Color32::from_rgba_unmultiplied(
                                    node.style.bg_color.r(),
                                    node.style.bg_color.g(),
                                    node.style.bg_color.b(),
                                    100,
                                ),
                                Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 150)),
                                egui::StrokeKind::Inside,
                            );
                            // 幽灵文字
                            let text_color = if node.style.text_color == Color32::WHITE {
                                Color32::from_rgba_unmultiplied(255, 255, 255, 200)
                            } else {
                                Color32::from_rgba_unmultiplied(40, 40, 50, 200)
                            };
                            painter.text(
                                cursor,
                                egui::Align2::CENTER_CENTER,
                                &node.text,
                                egui::FontId::new(node.style.font_size * tab.viewport.zoom, egui::FontFamily::Proportional),
                                text_color,
                            );
                        }
                    }

                    // 3. 放置目标高亮（蓝=同级排序，绿=跨父移动），带脉冲
                    if let Some(target) = tab.hovered_id {
                        if target != drag_id && target != tab.map.root {
                            let drag_parent = tab.map.get(drag_id).and_then(|n| n.parent);
                            let target_parent = tab.map.get(target).and_then(|n| n.parent);
                            let is_sibling = drag_parent == target_parent && drag_parent.is_some();
                            let valid = is_sibling || !tab.map.is_descendant(drag_id, target);

                            if valid {
                                if let Some(layout) = tab.layouts.get(&target) {
                                    let sp = tab.viewport.world_to_screen(layout.pos);
                                    let sw = layout.size.0 * tab.viewport.zoom;
                                    let sh = layout.size.1 * tab.viewport.zoom;
                                    let drop_rect = Rect::from_min_size(
                                        sp - Vec2::new(6.0, 6.0),
                                        Vec2::new(sw + 12.0, sh + 12.0),
                                    );
                                    let (line_col, fill_col) = if is_sibling {
                                        (
                                            Color32::from_rgb(33, 150, 243),
                                            Color32::from_rgba_unmultiplied(33, 150, 243, (25.0 + 20.0 * pulse) as u8),
                                        )
                                    } else {
                                        (
                                            Color32::from_rgb(76, 175, 80),
                                            Color32::from_rgba_unmultiplied(76, 175, 80, (25.0 + 20.0 * pulse) as u8),
                                        )
                                    };
                                    painter.rect(
                                        drop_rect,
                                        egui::CornerRadius::same(8),
                                        fill_col,
                                        Stroke::new(2.5_f32 + pulse, line_col),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                            }
                        }
                    }
                }

                // 框选矩形 — 粗线边框 + 极淡背景
                if tab.box_selecting {
                    if let Some(start) = tab.box_select_start {
                        if let Some(curr) = ui.input(|i| i.pointer.hover_pos()) {
                            let min = Pos2::new(start.x.min(curr.x), start.y.min(curr.y));
                            let max = Pos2::new(start.x.max(curr.x), start.y.max(curr.y));
                            let sel_rect = Rect::from_min_max(min, max);
                            // 极淡背景填充
                            painter.rect_filled(
                                sel_rect,
                                egui::CornerRadius::same(2),
                                Color32::from_rgba_unmultiplied(33, 150, 243, 20),
                            );
                            // 粗边框（实线）
                            painter.rect_stroke(
                                sel_rect,
                                egui::CornerRadius::same(2),
                                Stroke::new(2.0_f32, Color32::from_rgb(33, 150, 243)),
                                egui::StrokeKind::Inside,
                            );
                            // 四角加粗标记
                            let corner_len = 8.0;
                            let accent = Color32::from_rgb(33, 150, 243);
                            // 左上
                            painter.line_segment([min, Pos2::new(min.x + corner_len, min.y)], Stroke::new(3.0_f32, accent));
                            painter.line_segment([min, Pos2::new(min.x, min.y + corner_len)], Stroke::new(3.0_f32, accent));
                            // 右上
                            let tr = Pos2::new(max.x, min.y);
                            painter.line_segment([tr, Pos2::new(max.x - corner_len, min.y)], Stroke::new(3.0_f32, accent));
                            painter.line_segment([tr, Pos2::new(max.x, min.y + corner_len)], Stroke::new(3.0_f32, accent));
                            // 左下
                            let bl = Pos2::new(min.x, max.y);
                            painter.line_segment([bl, Pos2::new(min.x + corner_len, max.y)], Stroke::new(3.0_f32, accent));
                            painter.line_segment([bl, Pos2::new(min.x, max.y - corner_len)], Stroke::new(3.0_f32, accent));
                            // 右下
                            painter.line_segment([max, Pos2::new(max.x - corner_len, max.y)], Stroke::new(3.0_f32, accent));
                            painter.line_segment([max, Pos2::new(max.x, max.y - corner_len)], Stroke::new(3.0_f32, accent));
                        }
                    }
                }

                let response = ui.interact(rect, Id::new("canvas_interact"), Sense::click_and_drag());

                tab.handle_mouse(ui, &response, rect);

                tab.draw_scrollbars(ui, &painter, rect);
            });

        let _ = panel_response;

        // 编辑模式下叠加 TextEdit
        if let Some(edit_id) = self.tabs[active].editing {
            let edit_info = {
                let tab = &self.tabs[active];
                tab.layouts.get(&edit_id).and_then(|l| {
                    tab.map.get(edit_id).map(|n| (l.pos, l.size, n.style))
                })
            };
            if let Some((pos, size, style)) = edit_info {
                let (offset, zoom) = {
                    let tab = &self.tabs[active];
                    (tab.viewport.offset, tab.viewport.zoom)
                };
                let screen_pos = Pos2::new(pos.x * zoom + offset.x, pos.y * zoom + offset.y);
                let screen_w = size.0 * zoom;
                let screen_h = size.1 * zoom;
                let font_size = style.font_size * zoom;
                let text_color = style.text_color;
                let bg_color = style.bg_color;

                egui::Area::new(Id::new("node_edit_area"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(screen_pos)
                    .show(ctx, |ui| {
                        let bg_rect = Rect::from_min_size(screen_pos, Vec2::new(screen_w, screen_h));
                        ui.painter().rect_filled(bg_rect, egui::CornerRadius::same(6), bg_color);

                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        ui.set_min_size(Vec2::new(screen_w, screen_h));
                        ui.set_max_size(Vec2::new(screen_w, screen_h));
                        ui.vertical_centered(|ui| {
                            let resp = ui.add_sized(
                                Vec2::new(screen_w, screen_h),
                                egui::TextEdit::singleline(&mut self.tabs[active].edit_text)
                                    .text_color(text_color)
                                    .font(egui::FontId::new(font_size, font_id_to_family(style.font_id)))
                                    .frame(false)
                                    .clip_text(true)
                                    .hint_text("")
                                    .desired_width(screen_w),
                            );
                            resp.request_focus();
                        });
                    });
            }
        }
    }

    /// 处理键盘快捷键
    fn handle_keyboard(&mut self, ctx: &Context) {
        let active = self.active_tab;

        // 编辑模式下只处理 Enter / Escape
        if self.tabs[active].editing.is_some() {
            if ctx.input(|i| i.key_pressed(Key::Enter)) && !ctx.input(|i| i.modifiers.shift) {
                self.tabs[active].finish_editing();
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.tabs[active].editing = None;
                self.tabs[active].edit_text.clear();
            }
            return;
        }

        let modifiers = ctx.input(|i| i.modifiers);

        // Ctrl+Z 撤销
        if modifiers.ctrl && ctx.input(|i| i.key_pressed(Key::Z)) {
            let tab = &mut self.tabs[active];
            tab.map.undo();
            tab.dirty = true;
            tab.layout_dirty = true;
        }

        // Ctrl+Y 或 Ctrl+Shift+Z 重做
        if (modifiers.ctrl && ctx.input(|i| i.key_pressed(Key::Y)))
            || (modifiers.ctrl && modifiers.shift && ctx.input(|i| i.key_pressed(Key::Z)))
        {
            let tab = &mut self.tabs[active];
            tab.map.redo();
            tab.dirty = true;
            tab.layout_dirty = true;
        }

        // Ctrl+N 新建标签页
        if modifiers.ctrl && ctx.input(|i| i.key_pressed(Key::N)) {
            self.tabs.push(Tab::new_empty());
            self.active_tab = self.tabs.len() - 1;
        }

        // Ctrl+O 打开文件到新标签页
        if modifiers.ctrl && ctx.input(|i| i.key_pressed(Key::O)) {
            if let Some(path) = persistence::open_dialog() {
                match persistence::load_mindmap(&path) {
                    Ok(map) => {
                        self.tabs.push(Tab::from_map(map, Some(path)));
                        self.active_tab = self.tabs.len() - 1;
                    }
                    Err(e) => {
                        eprintln!("打开失败: {}", e);
                    }
                }
            }
        }

        // Ctrl+S 保存
        if modifiers.ctrl && !modifiers.shift && ctx.input(|i| i.key_pressed(Key::S)) {
            self.save_active_tab();
        }

        // Ctrl+Shift+S 另存为
        if modifiers.ctrl && modifiers.shift && ctx.input(|i| i.key_pressed(Key::S)) {
            let root_text = self.tabs[active].map.get(self.tabs[active].map.root)
                .map(|n| n.text.as_str()).unwrap_or("untitled");
            if let Some(path) = persistence::save_dialog(root_text) {
                self.tabs[active].file_path = Some(path.clone());
                let _ = persistence::save_mindmap(&self.tabs[active].map, &path);
                self.tabs[active].dirty = false;
                self.tabs[active].layout_dirty = true;
            }
        }

        // Ctrl+0 重置缩放
        if modifiers.ctrl && ctx.input(|i| i.key_pressed(Key::Num0)) {
            self.tabs[active].viewport.zoom = 1.0;
        }

        // Tab 添加子节点
        if ctx.input(|i| i.key_pressed(Key::Tab)) && !modifiers.shift {
            let tab = &mut self.tabs[active];
            if let Some(id) = tab.selected {
                let name = default_child_name(&tab.map, id);
                let new_id = tab.map.add_child(id, &name);
                tab.layout_dirty = true;
                tab.selected = Some(new_id);
                tab.start_editing(new_id);
            }
        }

        // Shift+Tab 或 Backspace 删除节点
        if (modifiers.shift && ctx.input(|i| i.key_pressed(Key::Tab)))
            || ctx.input(|i| i.key_pressed(Key::Backspace))
        {
            let tab = &mut self.tabs[active];
            if !tab.multi_selected.is_empty() {
                let ids = std::mem::take(&mut tab.multi_selected);
                tab.map.delete_nodes(&ids);
                tab.selected = Some(tab.map.root);
                tab.dirty = true;
                tab.layout_dirty = true;
            } else if let Some(id) = tab.selected {
                if id != tab.map.root {
                    tab.map.delete_node(id);
                    tab.layout_dirty = true;
                    tab.selected = Some(tab.map.root);
                    tab.dirty = true;
                }
            }
        }

        // Enter 添加同级节点
        if ctx.input(|i| i.key_pressed(Key::Enter)) && !modifiers.shift {
            let tab = &mut self.tabs[active];
            if let Some(id) = tab.selected {
                if let Some(parent) = tab.map.get(id).and_then(|n| n.parent) {
                    let name = default_child_name(&tab.map, parent);
                    let new_id = tab.map.add_child(parent, &name);
                    tab.layout_dirty = true;
                    tab.selected = Some(new_id);
                    tab.start_editing(new_id);
                }
            }
        }

        // Delete 删除节点
        if ctx.input(|i| i.key_pressed(Key::Delete)) {
            let tab = &mut self.tabs[active];
            if !tab.multi_selected.is_empty() {
                let ids = std::mem::take(&mut tab.multi_selected);
                tab.map.delete_nodes(&ids);
                tab.selected = Some(tab.map.root);
                tab.dirty = true;
                tab.layout_dirty = true;
            } else if let Some(id) = tab.selected {
                if id != tab.map.root {
                    tab.map.delete_node(id);
                    tab.layout_dirty = true;
                    tab.selected = Some(tab.map.root);
                    tab.dirty = true;
                }
            }
        }

        // Ctrl+C 复制
        if modifiers.ctrl && ctx.input(|i| i.key_pressed(Key::C)) {
            let mut ids: Vec<NodeId> = self.tabs[active].multi_selected.clone();
            if let Some(sid) = self.tabs[active].selected {
                if !ids.contains(&sid) {
                    ids.push(sid);
                }
            }
            let clips: Vec<ClipboardNode> = ids.iter()
                .filter_map(|&id| self.tabs[active].map.copy_node(id))
                .collect();
            if !clips.is_empty() {
                self.clipboard = serde_json::to_string(&clips).unwrap_or_default();
            }
        }

        // Ctrl+V 粘贴
        if modifiers.ctrl && ctx.input(|i| i.key_pressed(Key::V)) {
            if !self.clipboard.is_empty() {
                if let Ok(clips) = serde_json::from_str::<Vec<ClipboardNode>>(&self.clipboard) {
                    let tab = &mut self.tabs[active];
                    let parent = tab.selected.unwrap_or(tab.map.root);
                    for clip in &clips {
                        tab.map.paste_node(parent, clip);
                    }
                    tab.dirty = true;
                    tab.layout_dirty = true;
                }
            }
        }

        // Ctrl+A 全选
        if modifiers.ctrl && ctx.input(|i| i.key_pressed(Key::A)) {
            let tab = &mut self.tabs[active];
            tab.multi_selected = tab.map.all_node_ids()
                .into_iter()
                .filter(|&id| id != tab.map.root)
                .collect();
            tab.selected = Some(tab.map.root);
        }

        // 方向键导航
        let nav = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
            )
        });

        if let Some(id) = self.tabs[active].selected {
            let new_sel = match nav {
                (true, _, _, _) => self.tabs[active].navigate_up(id),
                (_, true, _, _) => self.tabs[active].navigate_down(id),
                (_, _, true, _) => self.tabs[active].navigate_left(id),
                (_, _, _, true) => self.tabs[active].navigate_right(id),
                _ => None,
            };
            if let Some(new_id) = new_sel {
                self.tabs[active].selected = Some(new_id);
            }
        }
    }

    /// 保存确认弹窗
    fn render_save_prompt(&mut self, ctx: &Context) {
        let mut action = None;
        let mut dismiss = false;

        // 根据退出流程或关闭标签页，显示不同的提示文字
        let exit_idx = self.exit_current;
        let (title_text, desc_text) = if let Some(idx) = exit_idx {
            if idx < self.tabs.len() {
                let name = self.tabs[idx].tab_title();
                (
                    format!("标签页「{}」有未保存的修改", name),
                    "是否保存当前标签页的修改？".to_string(),
                )
            } else {
                ("未保存的修改".to_string(), "当前标签页有未保存的修改，是否保存？".to_string())
            }
        } else {
            ("未保存的修改".to_string(), "当前标签页有未保存的修改，是否保存？".to_string())
        };

        egui::Window::new("保存确认")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(egui::Frame {
                fill: Color32::from_rgb(253, 253, 255),
                stroke: egui::Stroke::new(1.0_f32, Color32::from_rgb(210, 215, 225)),
                corner_radius: egui::CornerRadius::same(10),
                shadow: egui::Shadow {
                    offset: [0, 4],
                    blur: 24,
                    spread: 2,
                    color: Color32::from_black_alpha(40),
                },
                inner_margin: egui::Margin {
                    left: 10, right: 10, top: 8, bottom: 8,
                },
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("\u{26A0}")
                        .color(Color32::from_rgb(255, 170, 0))
                        .size(28.0));
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&title_text)
                            .strong()
                            .size(16.0));
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(&desc_text)
                            .color(Color32::from_rgb(90, 95, 110))
                            .size(13.0));
                    });
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let save_btn = egui::Button::new(
                        egui::RichText::new("  保存  ").color(Color32::WHITE).strong()
                    ).fill(Color32::from_rgb(33, 150, 243))
                     .corner_radius(egui::CornerRadius::same(6));
                    if ui.add(save_btn).clicked() {
                        action = Some(true);
                    }
                    ui.add_space(6.0);
                    let skip_btn = egui::Button::new(
                        egui::RichText::new(" 不保存 ").color(Color32::from_rgb(100, 105, 120))
                    ).fill(Color32::from_rgb(235, 238, 242))
                     .corner_radius(egui::CornerRadius::same(6));
                    if ui.add(skip_btn).clicked() {
                        action = Some(false);
                    }
                    ui.add_space(6.0);
                    let cancel_btn = egui::Button::new(
                        egui::RichText::new(" 取消 ").color(Color32::from_rgb(100, 105, 120))
                    ).fill(Color32::from_rgb(235, 238, 242))
                     .corner_radius(egui::CornerRadius::same(6));
                    if ui.add(cancel_btn).clicked() {
                        dismiss = true;
                    }
                });
                ui.add_space(4.0);
            });

        if dismiss {
            self.show_save_prompt = false;
            self.pending_action = None;
            self.exit_current = None;
            self.exit_queue.clear();
        }
        if let Some(save_first) = action {
            self.show_save_prompt = false;
            if save_first {
                if let Some(idx) = exit_idx {
                    // 退出流程：保存当前标签页
                    if idx < self.tabs.len() {
                        if self.tabs[idx].file_path.is_some() {
                            // 有文件路径：静默保存
                            let path = self.tabs[idx].file_path.clone().unwrap();
                            let _ = persistence::save_mindmap(&self.tabs[idx].map, &path);
                            self.tabs[idx].dirty = false;
                            self.tabs[idx].layout_dirty = true;
                            self.advance_exit(ctx);
                        } else {
                            // 无文件路径：需要原生对话框
                            self.exit_save_deferred = true;
                            self.need_clear_frame = true;
                        }
                    }
                } else {
                    // 关闭标签页流程：使用原有机制
                    self.deferred_save = true;
                    if self.pending_save_needs_dialog() {
                        self.need_clear_frame = true;
                    }
                }
            } else {
                // 不保存
                if exit_idx.is_some() {
                    self.advance_exit(ctx);
                }
            }
        }
    }

    /// 执行待执行的文件操作
    fn execute_pending_action(&mut self, ctx: &Context) {
        if let Some(action) = self.pending_action.take() {
            match action {
                PendingAction::Exit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                PendingAction::CloseTab(idx) => {
                    if idx < self.tabs.len() {
                        self.tabs.remove(idx);
                        if self.tabs.is_empty() {
                            self.active_tab = 0;
                        } else if idx < self.active_tab {
                            self.active_tab -= 1;
                        } else if self.active_tab >= self.tabs.len() {
                            self.active_tab = self.tabs.len() - 1;
                        }
                    }
                }
            }
        }
    }
}
