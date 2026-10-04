/// Headless 测试 — 验证核心逻辑（数据模型、布局、序列化）
/// 不需要 GPU，纯逻辑测试

#[cfg(test)]
mod tests {
    use mindmap::core::mindmap::MindMap;
    use mindmap::core::layout::{LayoutConfig, layout_tree, LayoutDirection};
    use mindmap::core::node::{MindNode, NodeId, NodeStyle, default_style_for_depth};
    use eframe::egui::Color32;

    #[test]
    fn test_create_mindmap() {
        let map = MindMap::new("测试脑图");
        assert_eq!(map.title, "测试脑图");
        assert_eq!(map.nodes.len(), 1);
        assert_eq!(map.root, 0);
        assert!(map.get(0).unwrap().is_root());
        assert_eq!(map.get(0).unwrap().text, "测试脑图");
    }

    #[test]
    fn test_add_children() {
        let mut map = MindMap::new("测试");
        let root = map.root;
        
        let c1 = map.add_child(root, "分支1");
        let c2 = map.add_child(root, "分支2");
        let c3 = map.add_child(root, "分支3");
        
        assert_eq!(map.get(root).unwrap().children.len(), 3);
        assert_eq!(map.get(c1).unwrap().text, "分支1");
        assert_eq!(map.get(c2).unwrap().text, "分支2");
        assert_eq!(map.get(c3).unwrap().text, "分支3");
        
        // 添加孙子节点
        let gc1 = map.add_child(c1, "子分支1-1");
        let gc2 = map.add_child(c1, "子分支1-2");
        
        assert_eq!(map.get(c1).unwrap().children.len(), 2);
        assert_eq!(map.depth(gc1), 2);
    }

    #[test]
    fn test_edit_text() {
        let mut map = MindMap::new("测试");
        let root = map.root;
        
        map.edit_text(root, "修改后的标题");
        assert_eq!(map.get(root).unwrap().text, "修改后的标题");
        
        // 验证撤销栈
        assert_eq!(map.undo_stack.len(), 1);
        
        map.undo();
        assert_eq!(map.get(root).unwrap().text, "测试");
        
        map.redo();
        assert_eq!(map.get(root).unwrap().text, "修改后的标题");
    }

    #[test]
    fn test_edit_style() {
        let mut map = MindMap::new("测试");
        let root = map.root;
        
        let new_style = NodeStyle {
            bg_color: Color32::from_rgb(255, 0, 0),
            text_color: Color32::WHITE,
            font_size: 20.0,
            bold: true,
            border: false,
            font_id: 0,
        };
        
        map.edit_style(root, new_style.clone());
        assert_eq!(map.get(root).unwrap().style, new_style);
        
        map.undo();
        assert_ne!(map.get(root).unwrap().style, new_style);
        
        map.redo();
        assert_eq!(map.get(root).unwrap().style, new_style);
    }

    #[test]
    fn test_delete_node() {
        let mut map = MindMap::new("测试");
        let root = map.root;
        
        let c1 = map.add_child(root, "分支1");
        let c2 = map.add_child(root, "分支2");
        
        assert_eq!(map.get(root).unwrap().children.len(), 2);
        
        // 删除 c1
        map.delete_node(c1);
        
        // c1 应该被标记删除
        assert!(map.get(c1).unwrap().text.is_empty());
        // root 的 children 不应包含 c1
        assert!(!map.get(root).unwrap().children.contains(&c1));
        assert!(map.get(root).unwrap().children.contains(&c2));
        
        // 不能删除根节点
        assert!(map.delete_node(root).is_none());
    }

    #[test]
    fn test_collapse() {
        let mut map = MindMap::new("测试");
        let root = map.root;
        
        let c1 = map.add_child(root, "分支1");
        map.add_child(c1, "子分支1-1");
        map.add_child(c1, "子分支1-2");
        
        // 未折叠时应可见所有节点
        let visible = map.visible_nodes();
        assert!(visible.contains(&c1));
        assert_eq!(visible.len(), 4); // root + c1 + 2 children
        
        // 折叠 c1
        map.toggle_collapse(c1);
        
        let visible = map.visible_nodes();
        assert!(visible.contains(&c1));
        assert_eq!(visible.len(), 2); // root + c1 (子节点被隐藏)
    }

    #[test]
    fn test_undo_redo() {
        let mut map = MindMap::new("测试");
        let root = map.root;
        
        let c1 = map.add_child(root, "分支1");
        assert_eq!(map.get(root).unwrap().children.len(), 1);
        
        // 撤销添加
        map.undo();
        assert_eq!(map.get(root).unwrap().children.len(), 0);
        
        // 重做
        map.redo();
        assert_eq!(map.get(root).unwrap().children.len(), 1);
    }

    #[test]
    fn test_depth_calculation() {
        let mut map = MindMap::new("测试");
        let root = map.root;
        assert_eq!(map.depth(root), 0);
        
        let c1 = map.add_child(root, "分支1");
        assert_eq!(map.depth(c1), 1);
        
        let gc1 = map.add_child(c1, "子分支1");
        assert_eq!(map.depth(gc1), 2);
        
        let ggc1 = map.add_child(gc1, "孙分支1");
        assert_eq!(map.depth(ggc1), 3);
    }

    #[test]
    fn test_serialization() {
        let mut map = MindMap::new("序列化测试");
        let root = map.root;
        
        map.add_child(root, "分支A");
        let c2 = map.add_child(root, "分支B");
        map.add_child(c2, "子分支B-1");
        
        // 序列化
        let json = serde_json::to_string(&map).unwrap();
        assert!(!json.is_empty());
        
        // 反序列化
        let loaded: MindMap = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.title, "序列化测试");
        assert_eq!(loaded.nodes.len(), map.nodes.len());
        assert_eq!(loaded.get(root).unwrap().text, "序列化测试");
        assert_eq!(loaded.get(root).unwrap().children.len(), 2);
    }

    #[test]
    fn test_style_for_depth() {
        let root_style = default_style_for_depth(0);
        assert!(root_style.bold);
        assert_eq!(root_style.font_size, 20.0);
        assert_ne!(root_style.bg_color, Color32::from_rgb(240, 240, 245));
        
        let branch_style = default_style_for_depth(1);
        assert!(branch_style.bold);
        assert_eq!(branch_style.font_size, 16.0);
        
        let leaf_style = default_style_for_depth(2);
        assert!(!leaf_style.bold);
        assert_eq!(leaf_style.font_size, 14.0);
    }

    #[test]
    fn test_visible_children() {
        let mut map = MindMap::new("测试");
        let root = map.root;
        
        let c1 = map.add_child(root, "分支1");
        let c2 = map.add_child(root, "分支2");
        map.add_child(c1, "子1-1");
        map.add_child(c1, "子1-2");
        
        let children = map.visible_children(root);
        assert_eq!(children.len(), 2);
        assert!(children.contains(&c1));
        assert!(children.contains(&c2));
        
        let c1_children = map.visible_children(c1);
        assert_eq!(c1_children.len(), 2);
    }

    #[test]
    fn test_large_map_performance() {
        // 测试大规模节点性能
        let mut map = MindMap::new("大规模测试");
        let root = map.root;
        
        // 创建 100 个一级分支，每个 10 个子节点 = 1000+ 节点
        let start = std::time::Instant::now();
        for i in 0..100 {
            let child = map.add_child(root, &format!("分支{}", i));
            for j in 0..10 {
                map.add_child(child, &format!("子{}-{}", i, j));
            }
        }
        let elapsed = start.elapsed();
        
        assert_eq!(map.nodes.len(), 1101); // root + 100 + 1000
        println!("创建 1100 个节点耗时: {:?}", elapsed);
        assert!(elapsed.as_millis() < 100); // 应该非常快
        
        // 测试可见节点遍历
        let start = std::time::Instant::now();
        let visible = map.visible_nodes();
        let elapsed = start.elapsed();
        println!("遍历 {} 个可见节点耗时: {:?}", visible.len(), elapsed);
        assert_eq!(visible.len(), 1101);
        assert!(elapsed.as_millis() < 10);
        
        // 测试序列化性能
        let start = std::time::Instant::now();
        let json = serde_json::to_string(&map).unwrap();
        let elapsed = start.elapsed();
        println!("序列化 1100 个节点耗时: {:?}, JSON 大小: {} bytes", elapsed, json.len());
        assert!(elapsed.as_millis() < 50);
        
        // 测试反序列化性能
        let start = std::time::Instant::now();
        let _loaded: MindMap = serde_json::from_str(&json).unwrap();
        let elapsed = start.elapsed();
        println!("反序列化 1100 个节点耗时: {:?}", elapsed);
        assert!(elapsed.as_millis() < 50);
    }
}
