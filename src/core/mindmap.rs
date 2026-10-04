use std::collections::HashMap;

use eframe::egui::{Color32, Pos2};
use serde::{Deserialize, Serialize};

use super::node::{MindNode, NodeId, NodeStyle, branch_color, style_for_node};

/// 操作记录 — 用于撤销/重做
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    /// 添加节点
    AddNode {
        id: NodeId,
        parent: NodeId,
        text: String,
        style: NodeStyle,
    },
    /// 删除节点（含所有子节点）
    DeleteNode {
        id: NodeId,
        parent: NodeId,
        node: MindNode,
    },
    /// 修改文本
    EditText {
        id: NodeId,
        old_text: String,
        new_text: String,
    },
    /// 修改样式
    EditStyle {
        id: NodeId,
        old_style: NodeStyle,
        new_style: NodeStyle,
    },
    /// 切换折叠
    ToggleCollapse {
        id: NodeId,
    },
    /// 移动节点（改变父节点）
    MoveNode {
        id: NodeId,
        old_parent: NodeId,
        new_parent: NodeId,
    },
    /// 同级排序
    ReorderNode {
        id: NodeId,
        parent: NodeId,
        old_idx: usize,
        new_idx: usize,
    },
}

/// 脑图文档
#[derive(Serialize, Deserialize)]
pub struct MindMap {
    /// 所有节点（arena 分配器）
    pub nodes: Vec<MindNode>,
    /// 根节点ID
    pub root: NodeId,
    /// 文档标题
    pub title: String,
    /// 操作历史
    #[serde(skip)]
    pub undo_stack: Vec<Action>,
    #[serde(skip)]
    pub redo_stack: Vec<Action>,
    /// 布局缓存 — 节点ID -> 屏幕位置
    #[serde(skip)]
    pub positions: HashMap<NodeId, Pos2>,
}

impl Default for MindMap {
    fn default() -> Self {
        Self::new("中心主题")
    }
}

impl MindMap {
    /// 创建新脑图（title 同时作为根节点文本和文档标题）
    pub fn new(title: &str) -> Self {
        let root_node = MindNode::new(0, title, None);
        Self {
            nodes: vec![root_node],
            root: 0,
            title: title.to_string(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            positions: HashMap::new(),
        }
    }

    /// 分配新节点ID
    fn alloc_id(&self) -> NodeId {
        self.nodes.len()
    }

    /// 获取节点引用
    pub fn get(&self, id: NodeId) -> Option<&MindNode> {
        self.nodes.get(id)
    }

    /// 获取节点可变引用
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut MindNode> {
        self.nodes.get_mut(id)
    }

    /// 计算节点深度
    pub fn depth(&self, id: NodeId) -> usize {
        let mut depth = 0;
        let mut current = id;
        while let Some(node) = self.get(current) {
            if let Some(parent) = node.parent {
                depth += 1;
                current = parent;
            } else {
                break;
            }
        }
        depth
    }

    /// 计算节点所属的分支索引（根节点的第几个子节点为祖先）
    pub fn branch_index(&self, id: NodeId) -> Option<usize> {
        if id == self.root {
            return None;
        }
        let mut current = id;
        loop {
            let node = self.get(current)?;
            match node.parent {
                Some(parent_id) if parent_id == self.root => {
                    return self
                        .get(self.root)?
                        .children
                        .iter()
                        .position(|&c| c == current);
                }
                Some(parent_id) => {
                    current = parent_id;
                }
                None => return None,
            }
        }
    }

    /// 获取节点的分支颜色
    pub fn branch_color_of(&self, id: NodeId) -> Color32 {
        branch_color(self.branch_index(id).unwrap_or(0))
    }

    /// 计算节点应该使用的样式
    pub fn style_for(&self, id: NodeId) -> NodeStyle {
        let depth = self.depth(id);
        let branch = self.branch_index(id);
        style_for_node(depth, branch)
    }

    /// 添加子节点
    pub fn add_child(&mut self, parent_id: NodeId, text: &str) -> NodeId {
        let id = self.alloc_id();
        let depth = self.depth(parent_id) + 1;
        // 计算新子节点的分支索引
        let branch_idx = if parent_id == self.root {
            // 根的新子节点：分支索引 = 当前根的子节点数
            self.get(self.root).map(|r| r.children.len())
        } else {
            // 非根节点的子节点：继承父节点的分支索引
            self.branch_index(parent_id)
        };
        let mut node = MindNode::new(id, text, Some(parent_id));
        node.style = style_for_node(depth, branch_idx);

        // 加入 arena
        self.nodes.push(node.clone());

        // 添加到父节点的 children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.children.push(id);
        }

        // 记录操作
        self.undo_stack.push(Action::AddNode {
            id,
            parent: parent_id,
            text: text.to_string(),
            style: node.style.clone(),
        });
        self.redo_stack.clear();
        self.trim_undo_stack();

        id
    }

    /// 删除节点（含子树）
    pub fn delete_node(&mut self, id: NodeId) -> Option<MindNode> {
        if id == self.root {
            return None; // 不允许删除根节点
        }

        let parent_id = self.get(id)?.parent?;

        // 递归收集所有子节点ID
        let to_delete = self.collect_subtree_ids(id);

        // 从父节点的 children 中移除
        if let Some(parent) = self.get_mut(parent_id) {
            parent.children.retain(|&c| c != id);
        }

        // 保存被删除的节点树（用于撤销）
        let deleted_node = self.get(id).cloned();

        // 标记删除（设为空文本 + 从 arena 中移除）
        // 注意：因为 arena 用索引，不能真正删除（否则索引会移位）
        // 改为标记删除 + children 清空
        // 实际做法：保留位置但标记为已删除
        for &del_id in &to_delete {
            if let Some(n) = self.get_mut(del_id) {
                n.text = String::new();
                n.children.clear();
                n.parent = None;
            }
        }

        // 记录操作
        if let Some(node) = &deleted_node {
            self.undo_stack.push(Action::DeleteNode {
                id,
                parent: parent_id,
                node: node.clone(),
            });
            self.redo_stack.clear();
        self.trim_undo_stack();
        }

        deleted_node
    }

    /// 收集子树所有节点ID
    pub fn collect_subtree_ids(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = vec![id];
        let mut stack = vec![id];
        while let Some(curr) = stack.pop() {
            if let Some(node) = self.get(curr) {
                for &child_id in &node.children {
                    result.push(child_id);
                    stack.push(child_id);
                }
            }
        }
        result
    }

    /// 修改节点文本
    pub fn edit_text(&mut self, id: NodeId, new_text: &str) {
        if let Some(node) = self.get_mut(id) {
            let old_text = node.text.clone();
            if old_text != new_text {
                node.text = new_text.to_string();
                self.undo_stack.push(Action::EditText {
                    id,
                    old_text,
                    new_text: new_text.to_string(),
                });
                self.redo_stack.clear();
        self.trim_undo_stack();
            }
        }
    }

    /// 修改节点样式
    pub fn edit_style(&mut self, id: NodeId, new_style: NodeStyle) {
        let old_style = {
            if let Some(node) = self.get_mut(id) {
                let old = node.style.clone();
                if old == new_style {
                    return; // 无变化
                }
                node.style = new_style.clone();
                Some(old)
            } else {
                None
            }
        };

        if let Some(old_style) = old_style {
            self.undo_stack.push(Action::EditStyle {
                id,
                old_style,
                new_style,
            });
            self.redo_stack.clear();
        self.trim_undo_stack();
        }
    }

    /// 判断 new_parent 是否是 id 的后代（防止环）
    pub fn is_descendant(&self, id: NodeId, new_parent: NodeId) -> bool {
        self.collect_subtree_ids(id).contains(&new_parent)
    }

    /// 移动节点（改变父节点）— 将 id 及其子树移到 new_parent 下
    pub fn move_node(&mut self, id: NodeId, new_parent: NodeId) {
        if id == self.root || id == new_parent {
            return;
        }
        // 禁止移到自己的后代下（防止环）
        let subtree = self.collect_subtree_ids(id);
        if subtree.contains(&new_parent) {
            return;
        }
        let old_parent = match self.get(id).and_then(|n| n.parent) {
            Some(p) => p,
            None => return,
        };
        if old_parent == new_parent {
            return;
        }
        // 从旧父节点 children 移除
        if let Some(p) = self.get_mut(old_parent) {
            p.children.retain(|&c| c != id);
        }
        // 加到新父节点 children
        if let Some(p) = self.get_mut(new_parent) {
            p.children.push(id);
        }
        // 更新 parent
        if let Some(n) = self.get_mut(id) {
            n.parent = Some(new_parent);
        }
        // 更新样式（深度/分支可能变化）
        let depth = self.depth(id);
        let branch_idx = self.branch_index(id);
        let new_style = style_for_node(depth, branch_idx);
        if let Some(n) = self.get_mut(id) {
            n.style = new_style;
        }
        self.undo_stack.push(Action::MoveNode {
            id,
            old_parent,
            new_parent,
        });
        self.redo_stack.clear();
        self.trim_undo_stack();
    }

    /// 同级排序 — 将 id 移到 sibling_id 前面或后面
    pub fn reorder_sibling(&mut self, id: NodeId, sibling_id: NodeId, before: bool) {
        let parent = match self.get(id).and_then(|n| n.parent) {
            Some(p) => p,
            None => return,
        };
        if sibling_id == id {
            return;
        }
        let old_idx = self.get(parent)
            .and_then(|p| p.children.iter().position(|&c| c == id))
            .unwrap_or(0);
        if let Some(p) = self.get_mut(parent) {
            p.children.retain(|&c| c != id);
        }
        let new_idx = if let Some(p) = self.get(parent) {
            if let Some(pos) = p.children.iter().position(|&c| c == sibling_id) {
                if before { pos } else { pos + 1 }
            } else {
                p.children.len()
            }
        } else {
            0
        };
        if let Some(p) = self.get_mut(parent) {
            p.children.insert(new_idx, id);
        }
        self.undo_stack.push(Action::ReorderNode {
            id,
            parent,
            old_idx,
            new_idx,
        });
        self.redo_stack.clear();
        self.trim_undo_stack();
    }

    /// 切换折叠状态
    pub fn toggle_collapse(&mut self, id: NodeId) {
        if let Some(node) = self.get_mut(id) {
            node.collapsed = !node.collapsed;
            self.undo_stack.push(Action::ToggleCollapse { id });
            self.redo_stack.clear();
        self.trim_undo_stack();
        }
    }

    /// 撤销
    pub fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop() {
            self.apply_undo(&action);
            self.redo_stack.push(action);
        }
    }

    /// 重做
    pub fn redo(&mut self) {
        if let Some(action) = self.redo_stack.pop() {
            self.apply_redo(&action);
            self.undo_stack.push(action);
        }
    }

    fn apply_undo(&mut self, action: &Action) {
        match action {
            Action::AddNode { id, parent, .. } => {
                // 撤销添加 = 删除
                if let Some(p) = self.get_mut(*parent) {
                    p.children.retain(|&c| c != *id);
                }
                if let Some(n) = self.get_mut(*id) {
                    n.parent = None;
                    n.children.clear();
                    n.text = String::new();
                }
            }
            Action::DeleteNode { id, parent, node } => {
                // 撤销删除 = 恢复
                if *id < self.nodes.len() {
                    self.nodes[*id] = node.clone();
                }
                if let Some(p) = self.get_mut(*parent) {
                    p.children.push(*id);
                }
            }
            Action::EditText { id, old_text, .. } => {
                if let Some(n) = self.get_mut(*id) {
                    n.text = old_text.clone();
                }
            }
            Action::EditStyle { id, old_style, .. } => {
                if let Some(n) = self.get_mut(*id) {
                    n.style = old_style.clone();
                }
            }
            Action::ToggleCollapse { id } => {
                if let Some(n) = self.get_mut(*id) {
                    n.collapsed = !n.collapsed;
                }
            }
            Action::MoveNode { id, old_parent, new_parent } => {
                // 撤销：从 new_parent children 移除，加回 old_parent children
                if let Some(p) = self.get_mut(*new_parent) {
                    p.children.retain(|&c| c != *id);
                }
                if let Some(p) = self.get_mut(*old_parent) {
                    p.children.push(*id);
                }
                if let Some(n) = self.get_mut(*id) {
                    n.parent = Some(*old_parent);
                }
                let depth = self.depth(*id);
                let branch_idx = self.branch_index(*id);
                let s = style_for_node(depth, branch_idx);
                if let Some(n) = self.get_mut(*id) {
                    n.style = s;
                }
            }
            Action::ReorderNode { id, parent, old_idx, .. } => {
                if let Some(p) = self.get_mut(*parent) {
                    p.children.retain(|&c| c != *id);
                    p.children.insert(*old_idx, *id);
                }
            }
        }
    }

    fn apply_redo(&mut self, action: &Action) {
        match action {
            Action::AddNode { id, parent, text, style } => {
                // 重做添加
                let node = MindNode {
                    id: *id,
                    text: text.clone(),
                    children: Vec::new(),
                    parent: Some(*parent),
                    style: style.clone(),
                    custom_pos: None,
                    collapsed: false,
                };
                if *id < self.nodes.len() {
                    self.nodes[*id] = node;
                } else {
                    self.nodes.push(node);
                }
                if let Some(p) = self.get_mut(*parent) {
                    p.children.push(*id);
                }
            }
            Action::DeleteNode { id, parent, .. } => {
                if let Some(p) = self.get_mut(*parent) {
                    p.children.retain(|&c| c != *id);
                }
                if let Some(n) = self.get_mut(*id) {
                    n.parent = None;
                    n.children.clear();
                    n.text = String::new();
                }
            }
            Action::EditText { id, new_text, .. } => {
                if let Some(n) = self.get_mut(*id) {
                    n.text = new_text.clone();
                }
            }
            Action::EditStyle { id, new_style, .. } => {
                if let Some(n) = self.get_mut(*id) {
                    n.style = new_style.clone();
                }
            }
            Action::ToggleCollapse { id } => {
                if let Some(n) = self.get_mut(*id) {
                    n.collapsed = !n.collapsed;
                }
            }
            Action::MoveNode { id, old_parent, new_parent } => {
                // 重做：从 old_parent children 移除，加到 new_parent children
                if let Some(p) = self.get_mut(*old_parent) {
                    p.children.retain(|&c| c != *id);
                }
                if let Some(p) = self.get_mut(*new_parent) {
                    p.children.push(*id);
                }
                if let Some(n) = self.get_mut(*id) {
                    n.parent = Some(*new_parent);
                }
                let depth = self.depth(*id);
                let branch_idx = self.branch_index(*id);
                let s = style_for_node(depth, branch_idx);
                if let Some(n) = self.get_mut(*id) {
                    n.style = s;
                }
            }
            Action::ReorderNode { id, parent, new_idx, .. } => {
                if let Some(p) = self.get_mut(*parent) {
                    p.children.retain(|&c| c != *id);
                    p.children.insert(*new_idx, *id);
                }
            }
        }
    }

    /// 获取所有可见节点（跳过已删除和折叠隐藏的）
    pub fn visible_nodes(&self) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.collect_visible(self.root, &mut result);
        result
    }

    fn collect_visible(&self, id: NodeId, result: &mut Vec<NodeId>) {
        if let Some(node) = self.get(id) {
            if node.text.is_empty() && !node.is_root() {
                return; // 已删除
            }
            result.push(id);
            if !node.collapsed {
                for &child_id in &node.children {
                    self.collect_visible(child_id, result);
                }
            }
        }
    }

    /// 获取节点的所有可见子节点
    pub fn visible_children(&self, id: NodeId) -> Vec<NodeId> {
        if let Some(node) = self.get(id) {
            if !node.collapsed {
                return node
                    .children
                    .iter()
                    .filter(|&&c| {
                        if let Some(n) = self.get(c) {
                            !n.text.is_empty() || n.is_root()
                        } else {
                            false
                        }
                    })
                    .copied()
                    .collect();
            }
        }
        Vec::new()
    }

    // === 复制 / 粘贴 ===

    /// 递归复制节点（含子树）为 ClipboardNode
    pub fn copy_node(&self, id: NodeId) -> Option<ClipboardNode> {
        let node = self.get(id)?;
        if node.text.is_empty() && !node.is_root() {
            return None;
        }
        let children: Vec<ClipboardNode> = node.children.iter()
            .filter_map(|&cid| self.copy_node(cid))
            .collect();
        Some(ClipboardNode {
            text: node.text.clone(),
            style: node.style,
            children,
        })
    }

    /// 粘贴 ClipboardNode 为 parent 的子节点（递归）
    pub fn paste_node(&mut self, parent: NodeId, clip: &ClipboardNode) -> Option<NodeId> {
        let new_id = self.add_child(parent, &clip.text);
        if let Some(node) = self.get_mut(new_id) {
            node.style = clip.style;
        }
        for child in &clip.children {
            self.paste_node(new_id, child);
        }
        Some(new_id)
    }

    /// 批量删除节点
    pub fn delete_nodes(&mut self, ids: &[NodeId]) {
        for &id in ids {
            if id != self.root {
                self.delete_node(id);
            }
        }
    }

    /// 获取所有节点ID（不含已删除）
    pub fn all_node_ids(&self) -> Vec<NodeId> {
        self.nodes.iter().enumerate()
            .filter(|(_, n)| !n.text.is_empty() || n.is_root())
            .map(|(i, _)| i)
            .collect()
    }

    /// 压缩内存：收缩 Vec/HashMap 的多余容量
    pub fn shrink_to_fit(&mut self) {
        self.nodes.shrink_to_fit();
        for node in &mut self.nodes {
            node.text.shrink_to_fit();
            node.children.shrink_to_fit();
        }
        self.undo_stack.shrink_to_fit();
        self.redo_stack.shrink_to_fit();
        self.positions.shrink_to_fit();
        self.title.shrink_to_fit();
    }

    /// 限制撤销栈大小（最多 50 条）
    pub fn trim_undo_stack(&mut self) {
        const MAX_UNDO: usize = 50;
        if self.undo_stack.len() > MAX_UNDO {
            let excess = self.undo_stack.len() - MAX_UNDO;
            self.undo_stack.drain(0..excess);
        }
    }
}

/// 剪贴板节点 — 用于复制/粘贴
#[derive(Serialize, Deserialize, Clone)]
pub struct ClipboardNode {
    pub text: String,
    pub style: NodeStyle,
    pub children: Vec<ClipboardNode>,
}
