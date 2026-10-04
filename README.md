# SuperMiniMind

基于 Rust + egui 的轻量级思维导图桌面应用。零依赖原生文件对话框、中文字体内置、Glow/WGPU 双渲染后端回退、多标签页、撤销/重做、JSON 持久化、XMind 文件导入。

## 功能特性

- 🎨 **画布渲染与交互**:点击选中、双击编辑、右键菜单、键盘快捷键(Tab 子节点 / Enter 同级 / Delete 删除)
- 🖱️ **节点拖拽与框选**:左键拖拽空白触发框选,节点可拖拽移动,中心节点悬浮高亮
- 🔍 **缩放与滚动**:画布缩放(10–500% DragValue 可编辑)、惯性滚动、滚动条、"回到中心"一键居中
- 🌳 **折叠展开**:每节点 +/- 折叠按钮,树形结构清晰
- ↩️ **撤销/重做**:完整 undo/redo 栈(限制 50 条),按需 `request_repaint` 降低 CPU 占用
- 💾 **持久化**:`.mmind` JSON 格式保存,关闭时逐标签页保存提示
- 📥 **XMind 导入**:解析 `.xmind` 文件(基于 zip + serde_json)
- 🗂️ **多标签页**:per-tab 状态隔离,切换时清理缓存 + `shrink_to_fit` 内存优化
- 🎨 **XMind 风格彩虹配色**:全饱和度分支色,美化界面
- 🖼️ **零依赖原生体验**:Win32 FFI 文件对话框(替代 rfd)、程序化 RGBA 图标、中文字体内置(雅黑/黑体/宋体/等线)
- ⚡ **渲染后端回退**:WGPU DX12 → Glow OpenGL,兼容老旧显卡

## 技术栈

- Rust 2024 edition
- [egui](https://github.com/emilk/egui) 0.31 + eframe
- serde / serde_json
- zip(deflate)
- wgpu(DX12 + WGSL)

## 构建

需要 Rust 1.85+(edition 2024)。

```bash
cargo build --release
```

Release profile 体积优先:`opt-level="z"` + `lto="fat"` + `codegen-units=1` + `strip`,静态 CRT,单文件分发。

## 下载

Windows 预编译二进制见 [Releases](../../releases)。

## 测试

无头逻辑测试(12 个单元测试,覆盖布局、增删改、撤销重做等):

```bash
cargo test
```

## License

MIT
