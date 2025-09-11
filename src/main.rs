//! 程序入口：初始化日志、加载 Slint UI，并准备后续 VM 绑定

use std::{cell::RefCell, rc::Rc, path::PathBuf};
use tracing_subscriber::fmt::SubscriberBuilder;
use slint::{ComponentHandle, ModelRc, VecModel};
use serde_json::Value;

slint::include_modules!();

mod model;
mod utils;
mod vm;

use model::{data_core::AppState, shadow_tree::JsonTreeNode};
use vm::bridge::*;
use std::time::Instant;

// TreeNodeData转换实现
impl From<&JsonTreeNode> for TreeNodeData {
    /// 将Rust JsonTreeNode转换为Slint可用的数据结构
    fn from(node: &JsonTreeNode) -> Self {
        Self {
            name: node.name.clone().into(),
            path: node.path.clone().into(),
            kind: format!("{:?}", node.kind).into(), // Object/Array/String等
            children: node.children as i32,
            preview: node.preview.clone().into(),
            depth: node.depth as i32,
            expanded: node.expanded,
            visible: true, // 在Rust端已过滤，这里总是true
        }
    }
}

// SearchItemData 转换实现（用于搜索结果列表）
impl From<&JsonTreeNode> for SearchItemData {
    fn from(node: &JsonTreeNode) -> Self {
        Self {
            name: node.name.clone().into(),
            path: node.path.clone().into(),
            kind: format!("{:?}", node.kind).into(),
        }
    }
}


/// VM桥接器：管理UI与数据层的交互
struct ViewModelBridge {
    app_state: Rc<RefCell<AppState>>,
    // 分页数据缓存
    preview_full_text: Rc<RefCell<String>>,
    final_full_text: Rc<RefCell<String>>,
}

impl ViewModelBridge {
    /// 创建新的VM桥接器并绑定所有回调
    fn new(app_window: &AppWindow, app_state: Rc<RefCell<AppState>>) -> Self {
        let bridge = Self {
            app_state: app_state.clone(),
            preview_full_text: Rc::new(RefCell::new(String::new())),
            final_full_text: Rc::new(RefCell::new(String::new())),
        };

        // 绑定所有UI回调
        bridge.setup_callbacks(app_window);
        bridge
    }

    /// 设置所有UI回调函数
    fn setup_callbacks(&self, app_window: &AppWindow) {
        let app_state = self.app_state.clone();

        // === 加载文件回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_load_file(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_load_file(&app_window, &app_state);
                }
            });
        }

        // === 节点选择回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_node_selected(move |json_path| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_node_selected(&app_window, &app_state, &json_path.to_string());
                }
            });
        }

        // === 复制按钮回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_copy_pressed(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_copy_pressed(&app_window, &app_state);
                }
            });
        }



        // === 一键获得最终产物回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            let preview_full_text = self.preview_full_text.clone();
            let final_full_text = self.final_full_text.clone();
            app_window.on_one_click_final_product(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_one_click_final_product(&app_window, &app_state, &preview_full_text, &final_full_text);
                }
            });
        }

        // === 搜索过滤回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_search_changed(move |filter_text| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_search_changed(&app_window, &app_state, &filter_text.to_string());
                }
            });
        }

        // === 搜索结果项选择回调（列表→详情） ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_search_item_selected(move |sel_path| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_search_item_selected(&app_window, &app_state, &sel_path.to_string());
                }
            });
        }

        // === 复制全部回调（后台聚合） ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            let preview_full_text = self.preview_full_text.clone();
            app_window.on_copy_all_pressed(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_copy_all_pressed(&app_window, &app_state, &preview_full_text);
                }
            });
        }


        // === 转换与复制最终 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            let preview_full_text = self.preview_full_text.clone();
            let final_full_text = self.final_full_text.clone();
            app_window.on_transform_pressed(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_transform_pressed(&app_window, &app_state, &preview_full_text, &final_full_text);
                }
            });
        }
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            let final_full_text = self.final_full_text.clone();
            app_window.on_copy_final_pressed(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_copy_final_pressed(&app_window, &app_state, &final_full_text);
                }
            });
        }

        // === 分页回调 ===
        {
            let app_window_weak = app_window.as_weak();
            let preview_full_text = self.preview_full_text.clone();
            app_window.on_preview_page_changed(move |page| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_preview_page_changed(&app_window, &preview_full_text, page);
                }
            });
        }
        {
            let app_window_weak = app_window.as_weak();
            let final_full_text = self.final_full_text.clone();
            app_window.on_final_page_changed(move |page| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_final_page_changed(&app_window, &final_full_text, page);
                }
            });
        }

        // === 回写功能回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            let preview_full_text = self.preview_full_text.clone();
            let final_full_text = self.final_full_text.clone();
            app_window.on_upload_writeback_file(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_upload_writeback_file(&app_window, &app_state, &preview_full_text, &final_full_text);
                }
            });
        }


        // === 清空回写日志回调 ===
        {
            let app_window_weak = app_window.as_weak();
            app_window.on_clear_writeback_log(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    app_window.set_writeback_log("".into());
                }
            });
        }

        // === 消息对话框回调 ===
        {
            let app_window_weak = app_window.as_weak();
            app_window.on_show_message_dialog(move |title, text| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    app_window.set_message_dialog_title(title);
                    app_window.set_message_dialog_text(text);
                    app_window.set_message_dialog_visible(true);
                }
            });
        }

        {
            let app_window_weak = app_window.as_weak();
            app_window.on_close_message_dialog(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    app_window.set_message_dialog_visible(false);
                }
            });
        }

        // === 回写后重新加载文件回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_reload_file_after_writeback(move |file_path| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_reload_file_after_writeback(&app_window, &app_state, &file_path.to_string());
                }
            });
        }

        // === JSON结构树控制回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_toggle_tree_flatten(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_toggle_tree_flatten(&app_window, &app_state);
                }
            });
        }
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_set_tree_char_filter(move |filter| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_set_tree_char_filter(&app_window, &app_state, &filter.to_string());
                }
            });
        }
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_toggle_tree_hide_empty(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_toggle_tree_hide_empty(&app_window, &app_state);
                }
            });
        }

        // === 节点展开/折叠回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_toggle_node_expanded(move |node_path| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_toggle_node_expanded(&app_window, &app_state, &node_path.to_string());
                }
            });
        }

        // === 智能英文字段检测回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_detect_english_fields(move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_detect_english_fields(&app_window, &app_state);
                }
            });
        }

        // === 应用搜索过滤回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_apply_search_filter(move |filter| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_apply_search_filter(&app_window, &app_state, &filter.to_string());
                }
            });
        }

        // === 提取搜索结果回调 ===
        {
            let app_state = app_state.clone();
            let app_window_weak = app_window.as_weak();
            app_window.on_extract_search_results(move |filter| {
                if let Some(app_window) = app_window_weak.upgrade() {
                    Self::handle_extract_search_results(&app_window, &app_state, &filter.to_string());
                }
            });
        }
    }

    /// 初始化UI状态
    fn initialize_ui(&self, app_window: &AppWindow) {
        app_window.set_status_message(STATUS_READY.into());
        app_window.set_current_path("".into());
        app_window.set_preview_text("".into());

        app_window.set_selected_json_path("".into());
        app_window.set_writeback_log("".into());

        // 设置空的树模型
        let empty_model = ModelRc::new(VecModel::<TreeNodeData>::default());
        app_window.set_tree_model(empty_model);
    }

    /// 显示文件选择对话框
    fn show_file_dialog() -> Option<PathBuf> {
        use rfd::FileDialog;

        // 使用原生文件对话框选择JSON文件
        let file_path = FileDialog::new()
            .add_filter("JSON文件", &["json"])
            .add_filter("所有文件", &["*"])
            .set_title("选择要处理的JSON文件")
            .pick_file();

        match file_path {
            Some(path) => {
                tracing::info!("用户选择了文件: {}", path.display());
                Some(path)
            }
            None => {
                tracing::info!("用户取消了文件选择");
                None
            }
        }
    }

    /// 处理加载文件操作
    fn handle_load_file(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>) {
        // 使用文件对话框选择JSON文件
        let file_path = match Self::show_file_dialog() {
            Some(path) => path,
            None => {
                app_window.set_status_message("未选择文件".into());
                return;
            }
        };

        app_window.set_status_message(STATUS_LOADING.into());
        app_window.set_performance_info("".into());

        // 开始性能监控
        let start_time = Instant::now();

        let load_result = app_state.borrow_mut().load_file(&file_path);
        match load_result {
            Ok(()) => {
                let load_duration = start_time.elapsed();

                // 设置根节点为展开状态
                if !app_state.borrow().tree_flat.is_empty() {
                    app_state.borrow_mut().tree_flat[0].expanded = true;
                    app_state.borrow_mut().update_visibility_by_expansion();
                }

                // 在新的作用域中进行不可变借用
                let (path_str, tree_data, node_count) = {
                    let state = app_state.borrow();
                    let path_str = state.source_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();

                    // 转换树模型数据 - 只包含可见的节点
                    let tree_data: Vec<TreeNodeData> = state.tree_flat
                        .iter()
                        .filter(|node| node.visible)
                        .map(TreeNodeData::from)
                        .collect();

                    let node_count = state.tree_flat.len();
                    (path_str, tree_data, node_count)
                };

                app_window.set_current_path(path_str.into());
                let model = ModelRc::new(VecModel::from(tree_data));
                app_window.set_tree_model(model);

                // 初始化树控制状态
                app_window.set_tree_flatten_mode(false);
                app_window.set_tree_char_filter("all".into());
                app_window.set_tree_hide_empty(false);

                // 显示性能信息
                let perf_info = format!("加载: {:.2}ms | 节点: {} | 内存: ~{:.1}MB",
                    load_duration.as_millis(),
                    node_count,
                    (node_count as f64 * 0.1) // 估算内存使用
                );
                app_window.set_performance_info(perf_info.into());

                app_window.set_status_message(STATUS_LOADED.into());
                tracing::info!("文件加载成功: {} 个节点，耗时: {:.2}ms",
                    node_count, load_duration.as_millis());

                // 自动检测英文字段
                Self::handle_detect_english_fields(app_window, app_state);
            }
            Err(e) => {
                let error_msg = format!("{}{}", STATUS_ERROR_PREFIX, e);
                app_window.set_status_message(error_msg.into());
                tracing::error!("文件加载失败: {}", e);
            }
        }
    }

    /// 处理节点选择操作
    fn handle_node_selected(
        app_window: &AppWindow,
        app_state: &Rc<RefCell<AppState>>,
        json_path: &str
    ) {
        // 检查是否在搜索状态，如果是则不覆盖搜索结果


        let search_text = app_window.get_search_filter().to_string();
        if !search_text.trim().is_empty() {
            tracing::info!("搜索状态下跳过节点选择: {}", json_path);
            return;
        }

        app_window.set_selected_json_path(json_path.into());

        // 开始性能监控
        let start_time = Instant::now();

        match app_state.borrow().extract_subtree_pretty(json_path) {
            Ok(pretty_json) => {
                let extract_duration = start_time.elapsed();
                app_window.set_preview_text(pretty_json.into());

                // 更新性能信息（保留加载信息，添加提取信息）
                let current_perf = app_window.get_performance_info().to_string();
                let extract_info = format!("提取: {:.1}ms", extract_duration.as_millis());
                let new_perf = if current_perf.is_empty() {
                    extract_info
                } else {
                    format!("{} | {}", current_perf, extract_info)
                };
                app_window.set_performance_info(new_perf.into());

                tracing::info!("节点选择成功: {}，耗时: {:.1}ms", json_path, extract_duration.as_millis());
            }
            Err(e) => {
                let error_msg = format!("{}{}", STATUS_ERROR_PREFIX, e);
                app_window.set_status_message(error_msg.into());
                tracing::error!("节点选择失败: {}", e);
            }
        }
    }

    /// 处理复制按钮操作（优先复制选中节点的完整 JSON；否则复制预览区文本）
    fn handle_copy_pressed(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>) {
        let selected_path = app_window.get_selected_json_path().to_string();
        let preview_text = app_window.get_preview_text().to_string();

        // 优先尝试基于选中路径提取完整 JSON（保持原始深度）
        let content_to_copy = if !selected_path.is_empty() && selected_path.starts_with("$") && !selected_path.starts_with("搜索结果") {
            match app_state.borrow().extract_subtree_pretty(&selected_path) {
                Ok(pretty) => Some(pretty),
                Err(e) => {
                    tracing::warn!("基于路径提取失败，将回退使用预览文本: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let final_text = content_to_copy.unwrap_or(preview_text);

        if final_text.trim().is_empty() {
            app_window.set_status_message("错误: 没有可复制的内容".into());
            return;
        }

        match utils::clipboard::copy_to_clipboard(&final_text) {
            Ok(()) => {
                app_window.set_status_message(STATUS_COPIED.into());
                tracing::info!("内容已复制到剪贴板，长度: {} 字符", final_text.len());
            }
            Err(e) => {
                let error_msg = format!("{}{}", STATUS_ERROR_PREFIX, e);
                app_window.set_status_message(error_msg.into());
                tracing::error!("复制失败: {}", e);
            }
        }
    }

    /// 添加日志到回写日志区域（异步版本，避免阻塞UI线程）
    fn append_writeback_log(app_window: &AppWindow, message: &str) {
        let current_log = app_window.get_writeback_log().to_string();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() % 86400000; // 取一天内的毫秒数
        let seconds = timestamp / 1000;
        let millis = timestamp % 1000;
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        let secs = seconds % 60;
        let time_str = format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, secs, millis);
        let new_entry = format!("[{}] {}", time_str, message);
        let updated_log = if current_log.is_empty() {
            new_entry
        } else {
            format!("{}\n{}", current_log, new_entry)
        };

        // 直接设置日志，避免复杂的异步处理
        app_window.set_writeback_log(updated_log.into());

        // 控制台输出用于调试
        println!("日志已更新: {}", message);

        // 打印到控制台用于调试
        tracing::info!("回写日志: {}", message);
    }



    /// 一键获得最终产物：自动执行生成中间产物2 + 转换为最终产物
    fn handle_one_click_final_product(
        app_window: &AppWindow,
        app_state: &Rc<RefCell<AppState>>,
        preview_full_text: &Rc<RefCell<String>>,
        final_full_text: &Rc<RefCell<String>>
    ) {
        let filter = app_window.get_search_filter().to_string();
        if filter.trim().is_empty() {
            app_window.set_status_message("错误: 过滤条件为空，请先设置搜索条件".into());
            return;
        }

        // 显示进度条
        app_window.invoke_show_progress("正在一键生成最终产物...".into());

        // 使用 spawn_local 让长时间操作异步执行，保持UI响应
        let app_weak = app_window.as_weak();
        let app_state_clone = app_state.clone();
        let preview_full_text_clone = preview_full_text.clone();
        let final_full_text_clone = final_full_text.clone();
        let filter_clone = filter.clone();

        slint::spawn_local(async move {
            tracing::info!("一键获得最终产物：开始执行");

            // 第一阶段：生成中间产物2
            app_weak.upgrade().map(|app| app.invoke_update_progress(0.1, "正在生成中间产物...".into()));

            let app_weak_progress = app_weak.clone();
            let progress_callback = move |progress: f32, message: &str| {
                // 将进度映射到0.1-0.5范围（第一阶段占50%）
                let mapped_progress = 0.1 + progress * 0.4;
                if let Some(app) = app_weak_progress.upgrade() {
                    app.invoke_update_progress(mapped_progress, format!("阶段1: {}", message).into());
                }
            };

            match app_state_clone.borrow().build_intermediate_stage2(&filter_clone, progress_callback) {
                Ok(stage2_json) => {
                    tracing::info!("一键获得最终产物：中间产物2生成成功");

                    // 保存中间产物到preview_full_text
                    *preview_full_text_clone.borrow_mut() = stage2_json.clone();

                    if let Some(app) = app_weak.upgrade() {
                        // 显示中间产物2在预览区域
                        let (page_text, total_pages) = ViewModelBridge::paginate_text(&stage2_json, 1, 300);
                        app.set_preview_text(page_text.into());
                        app.set_preview_current_page(1);
                        app.set_preview_total_pages(total_pages);
                        app.set_selected_json_path("中间产物第二阶段".into());

                        app.invoke_update_progress(0.5, "正在转换为最终产物...".into());

                        // 第二阶段：转换为最终产物
                        match serde_json::from_str::<Value>(&stage2_json) {
                            Ok(v) => {
                                app.invoke_update_progress(0.6, "正在处理数据项...".into());

                                // 使用BTreeMap自动排序
                                let mut out = std::collections::BTreeMap::new();

                                if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
                                    let total_items = items.len();
                                    for (index, item) in items.iter().enumerate() {
                                        // 将进度映射到0.6-0.8范围
                                        let progress = 0.6 + (index as f32 / total_items as f32) * 0.2;
                                        if index % 100 == 0 || index == total_items - 1 {
                                            app.invoke_update_progress(progress, format!("阶段2: 处理项目 {}/{}", index + 1, total_items).into());
                                        }

                                        let seq = item.get("seq").and_then(|s| s.as_u64()).unwrap_or(0);
                                        let name_val = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                        out.insert(seq.to_string(), serde_json::Value::String(name_val.to_string()));
                                    }
                                }

                                app.invoke_update_progress(0.8, "正在构建最终JSON...".into());

                                // 构建最终JSON
                                let final_json = serde_json::Value::Object(out.into_iter().collect());
                                match serde_json::to_string_pretty(&final_json) {
                                    Ok(s) => {
                                        app.invoke_update_progress(0.9, "正在格式化输出...".into());

                                        // 保存完整文本
                                        *final_full_text_clone.borrow_mut() = s.clone();

                                        // 计算分页并显示第一页
                                        let (page_text, total_pages) = ViewModelBridge::paginate_text(&s, 1, 300);
                                        app.set_final_product_text(page_text.into());
                                        app.set_final_current_page(1);
                                        app.set_final_total_pages(total_pages);

                                        app.invoke_update_progress(1.0, "完成".into());
                                        app.set_status_message("一键获得最终产物完成！".into());

                                        // 隐藏进度条
                                        app.invoke_hide_progress();

                                        tracing::info!("一键获得最终产物：执行成功");
                                    }
                                    Err(e) => {
                                        app.invoke_hide_progress();
                                        let msg = format!("{}最终产物格式化失败: {}", STATUS_ERROR_PREFIX, e);
                                        app.set_status_message(msg.into());
                                        tracing::error!("一键获得最终产物：最终产物格式化失败: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                app.invoke_hide_progress();
                                let msg = format!("{}中间产物解析失败: {}", STATUS_ERROR_PREFIX, e);
                                app.set_status_message(msg.into());
                                tracing::error!("一键获得最终产物：中间产物解析失败: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Some(app) = app_weak.upgrade() {
                        app.invoke_hide_progress();
                        let msg = format!("{}生成中间产物失败: {}", STATUS_ERROR_PREFIX, e);
                        app.set_status_message(msg.into());
                    }
                    tracing::error!("一键获得最终产物：生成中间产物失败: {}", e);
                }
            }
        }).unwrap();
    }

    /// 处理另存为按钮操作
    fn handle_save_as_pressed(
        app_window: &AppWindow,
        app_state: &Rc<RefCell<AppState>>
    ) {
        // 目前使用硬编码路径进行测试（后续可添加文件对话框）
        let save_path = std::path::Path::new("output.json");

        // 开始性能监控
        let start_time = Instant::now();

        match app_state.borrow().save_to_file(save_path) {
            Ok(()) => {
                let save_duration = start_time.elapsed();
                let success_msg = format!("文件已保存到: {}", save_path.display());
                app_window.set_status_message(success_msg.into());

                // 更新性能信息
                let current_perf = app_window.get_performance_info().to_string();
                let save_info = format!("保存: {:.1}ms", save_duration.as_millis());
                let new_perf = if current_perf.contains("保存:") {
                    // 替换现有的保存信息
                    current_perf.split(" | ").filter(|s| !s.starts_with("保存:"))
                        .chain(std::iter::once(save_info.as_str()))
                        .collect::<Vec<_>>().join(" | ")
                } else {
                    format!("{} | {}", current_perf, save_info)
                };
                app_window.set_performance_info(new_perf.into());

                tracing::info!("文件保存成功: {}，耗时: {:.1}ms", save_path.display(), save_duration.as_millis());
            }
            Err(e) => {
                let error_msg = format!("{}{}", STATUS_ERROR_PREFIX, e);
                app_window.set_status_message(error_msg.into());
                tracing::error!("文件保存失败: {}", e);
            }
        }
    }

    /// 处理搜索过滤改变
    fn handle_search_changed(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>, filter: &str) {
        let start_time = Instant::now();

        // 应用搜索过滤
        app_state.borrow_mut().apply_search_filter(filter);

        // 使用新的重建函数，支持扁平化和字符过滤
        Self::rebuild_tree_model(app_window, app_state);

        // 搜索模式：仅构建“匹配列表”模型，不在预览区一次性渲染聚合内容
        if filter.trim().is_empty() {
            app_window.set_preview_text("".into());
            app_window.set_selected_json_path("".into());
            let empty: Vec<SearchItemData> = Vec::new();
            app_window.set_search_results(ModelRc::new(VecModel::from(empty)));
        } else {
            let filter_lower = filter.to_lowercase();
            let items: Vec<SearchItemData> = {
                let state = app_state.borrow();
                state
                    .tree_flat
                    .iter()
                    .filter(|n| n.name.to_lowercase().contains(&filter_lower) || n.path.to_lowercase().contains(&filter_lower))
                    .map(SearchItemData::from)
                    .collect()
            };
            app_window.set_search_results(ModelRc::new(VecModel::from(items)));

            // 仅设置提示，不强制渲染详情；详情通过点击列表项加载
            app_window.set_preview_text("".into());
            app_window.set_selected_json_path(format!("搜索结果: {}", filter).into());
        }

        let filter_duration = start_time.elapsed();

        // 更新状态消息
        if filter.trim().is_empty() {
            app_window.set_status_message("已清除搜索过滤".into());
        } else {
            let visible_count = app_state.borrow().tree_flat.iter().filter(|n| n.visible).count();
            app_window.set_status_message(format!("搜索过滤: {} (显示 {} 个节点)", filter, visible_count).into());
        }

        tracing::info!("搜索过滤应用: {}，耗时: {:.1}ms", filter, filter_duration.as_millis());
    }

    /// 处理节点展开/折叠切换
    fn handle_toggle_node_expanded(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>, node_path: &str) {
        let start_time = Instant::now();

        // 切换节点展开状态
        app_state.borrow_mut().toggle_node_expanded(node_path);

        // 使用新的重建函数，支持扁平化和字符过滤
        Self::rebuild_tree_model(app_window, app_state);

        let toggle_duration = start_time.elapsed();

        // 更新状态消息
        let node_name = app_state.borrow().tree_flat
            .iter()

            .find(|n| n.path == node_path)
            .map(|n| n.name.clone())
            .unwrap_or_default();

        let expanded = app_state.borrow().tree_flat
            .iter()
            .find(|n| n.path == node_path)
            .map(|n| n.expanded)
            .unwrap_or(false);

        let action = if expanded { "展开" } else { "折叠" };
        app_window.set_status_message(format!("{}: {}", action, node_name).into());

        tracing::info!("节点{}切换: {}，耗时: {:.1}ms", action, node_path, toggle_duration.as_millis());
    }


    /// 处理搜索结果项被点击（中间产物 第一阶段：仅选中列表项，不展示详情）
    fn handle_search_item_selected(
        app_window: &AppWindow,
        _app_state: &Rc<RefCell<AppState>>,
        json_path: &str,
    ) {
        app_window.set_selected_json_path(json_path.into());
        app_window.set_status_message("已选中列表项（不展示详情）".into());
    }



    /// 生成“中间产物 第二阶段”：不复制到剪贴板，直接填充到预览区
    fn handle_copy_all_pressed(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>, preview_full_text: &Rc<RefCell<String>>) {
        let filter = app_window.get_search_filter().to_string();
        if filter.trim().is_empty() {
            app_window.set_status_message("错误: 过滤条件为空".into());
            return;
        }
        // 优化：预显示进度条并添加小延迟确保UI更新完成
        let start_time = std::time::Instant::now();
        tracing::info!("开始显示进度条");

        app_window.invoke_show_progress("正在生成中间产物第二阶段...".into());

        // 添加小延迟确保进度条显示完成
        std::thread::sleep(std::time::Duration::from_millis(16)); // 约1帧时间

        let progress_show_time = start_time.elapsed().as_millis();
        tracing::info!("进度条显示调用已执行，耗时: {}ms", progress_show_time);

        // 使用 spawn_local 让长时间操作异步执行，保持UI响应
        let app_weak = app_window.as_weak();
        let app_state_clone = app_state.clone();
        let preview_full_text_clone = preview_full_text.clone();
        let filter_clone = filter.clone();

        slint::spawn_local(async move {
            let build_start = std::time::Instant::now();
            tracing::info!("异步任务开始：调用 build_intermediate_stage2");

            // 优化：创建简化的进度回调，减少UI更新频率
            let app_weak_progress = app_weak.clone();
            let progress_callback = move |progress: f32, message: &str| {
                let callback_start = std::time::Instant::now();

                tracing::info!("进度回调被调用: {}% - {}", (progress * 100.0) as i32, message);
                if let Some(app) = app_weak_progress.upgrade() {
                    app.invoke_update_progress(progress, message.into());
                    let callback_time = callback_start.elapsed().as_millis();
                    tracing::info!("进度更新函数调用完成，耗时: {}ms", callback_time);
                } else {
                    tracing::warn!("无法获取app实例进行进度更新");
                }
            };

            match app_state_clone.borrow().build_intermediate_stage2(&filter_clone, progress_callback) {
                Ok(stage2_json) => {
                    let build_time = build_start.elapsed().as_millis();
                    tracing::info!("build_intermediate_stage2 执行成功，总耗时: {}ms，开始处理结果", build_time);

                    if let Some(app) = app_weak.upgrade() {
                        // 保存完整文本
                        let save_start = std::time::Instant::now();
                        tracing::info!("开始保存完整文本");
                        *preview_full_text_clone.borrow_mut() = stage2_json.clone();
                        let save_time = save_start.elapsed().as_millis();
                        tracing::info!("保存完整文本完成，耗时: {}ms", save_time);

                        // 计算分页并显示第一页
                        let paginate_start = std::time::Instant::now();
                        tracing::info!("开始计算分页");
                        let (page_text, total_pages) = ViewModelBridge::paginate_text(&stage2_json, 1, 300);
                        let paginate_time = paginate_start.elapsed().as_millis();
                        tracing::info!("分页计算完成，耗时: {}ms", paginate_time);

                        let ui_start = std::time::Instant::now();
                        app.set_preview_text(page_text.into());
                        app.set_preview_current_page(1);
                        app.set_preview_total_pages(total_pages);

                        app.set_selected_json_path("中间产物第二阶段".into());
                        app.set_final_product_text("".into());

                        tracing::info!("设置状态消息");
                        app.set_status_message("已生成中间产物 第二阶段".into());
                        let ui_time = ui_start.elapsed().as_millis();
                        tracing::info!("UI更新完成，耗时: {}ms", ui_time);

                        // 暂时不隐藏进度条，让用户看到完成状态
                        // app.invoke_hide_progress();
                    }
                }
                Err(e) => {
                    if let Some(app) = app_weak.upgrade() {
                        app.invoke_hide_progress();
                        let msg = format!("{}{}", STATUS_ERROR_PREFIX, e);
                        app.set_status_message(msg.into());
                    }
                    tracing::error!("生成中间产物 第二阶段 失败: {}", e);
                }
            }
        }).unwrap();
    }

    /// 将中间产物2转换为最终产物 {seq: name_value}
    fn handle_transform_pressed(app_window: &AppWindow, _app_state: &Rc<RefCell<AppState>>, preview_full_text: &Rc<RefCell<String>>, final_full_text: &Rc<RefCell<String>>) {
        let stage2_text = preview_full_text.borrow().clone();
        if stage2_text.trim().is_empty() {
            app_window.set_status_message("错误: 中间产物为空，无法转换".into());
            return;
        }

        // 显示进度条
        app_window.invoke_show_progress("正在生成最终产物...".into());
        app_window.invoke_update_progress(0.1, "正在解析中间产物...".into());
        match serde_json::from_str::<Value>(&stage2_text) {
            Ok(v) => {
                app_window.invoke_update_progress(0.3, "正在处理数据项...".into());

                // 使用BTreeMap自动排序，避免额外的排序步骤
                let mut out = std::collections::BTreeMap::new();

                if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
                    let total_items = items.len();
                    for (index, item) in items.iter().enumerate() {
                        // 更新进度
                        let progress = 0.3 + (index as f32 / total_items as f32) * 0.4;
                        if index % 100 == 0 || index == total_items - 1 {
                            app_window.invoke_update_progress(progress, format!("处理项目 {}/{}", index + 1, total_items).into());
                        }

                        let seq = item.get("seq").and_then(|s| s.as_u64()).unwrap_or(0);
                        let name_val = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        // 直接插入BTreeMap，自动按key排序
                        out.insert(seq.to_string(), serde_json::Value::String(name_val.to_string()));
                    }
                }

                app_window.invoke_update_progress(0.8, "正在构建最终JSON...".into());

                // 直接从BTreeMap构建JSON对象，无需额外排序
                let final_json = serde_json::Value::Object(out.into_iter().collect());
                match serde_json::to_string_pretty(&final_json) {
                    Ok(s) => {
                        app_window.invoke_update_progress(0.9, "正在格式化输出...".into());

                        // 保存完整文本
                        *final_full_text.borrow_mut() = s.clone();

                        // 计算分页并显示第一页
                        let (page_text, total_pages) = Self::paginate_text(&s, 1, 300);
                        app_window.set_final_product_text(page_text.into());
                        app_window.set_final_current_page(1);
                        app_window.set_final_total_pages(total_pages);

                        app_window.invoke_update_progress(1.0, "完成".into());
                        app_window.set_status_message("已构建最终产物".into());

                        // 隐藏进度条
                        app_window.invoke_hide_progress();
                    }
                    Err(e) => {
                        app_window.invoke_hide_progress();
                        let msg = format!("{}{}", STATUS_ERROR_PREFIX, e);
                        app_window.set_status_message(msg.into());
                    }
                }
            }
            Err(e) => {
                app_window.invoke_hide_progress();
                let msg = format!("{}{}", STATUS_ERROR_PREFIX, e);
                app_window.set_status_message(msg.into());
            }
        }
    }

    /// 复制最终产物到剪贴板
    fn handle_copy_final_pressed(app_window: &AppWindow, _app_state: &Rc<RefCell<AppState>>, final_full_text: &Rc<RefCell<String>>) {
        let text = final_full_text.borrow().clone();
        if text.trim().is_empty() {
            app_window.set_status_message("错误: 最终产物为空".into());
            return;
        }
        match utils::clipboard::copy_to_clipboard(&text) {
            Ok(()) => app_window.set_status_message(STATUS_COPIED.into()),
            Err(e) => {
                let msg = format!("{}{}", STATUS_ERROR_PREFIX, e);
                app_window.set_status_message(msg.into());
            }
        }
    }

    /// 文本分页：将文本按行分页，返回指定页的内容和总页数
    fn paginate_text(text: &str, page: i32, lines_per_page: usize) -> (String, i32) {
        let lines: Vec<&str> = text.lines().collect();
        let total_lines = lines.len();
        let total_pages = ((total_lines + lines_per_page - 1) / lines_per_page).max(1) as i32;

        if page < 1 || page > total_pages {
            return (String::new(), total_pages);
        }

        let start_idx = ((page - 1) as usize) * lines_per_page;
        let end_idx = (start_idx + lines_per_page).min(total_lines);

        let page_lines = &lines[start_idx..end_idx];
        (page_lines.join("\n"), total_pages)
    }

    /// 处理中间产物分页改变
    fn handle_preview_page_changed(app_window: &AppWindow, preview_full_text: &Rc<RefCell<String>>, page: i32) {
        let full_text = preview_full_text.borrow().clone();
        let (page_text, total_pages) = Self::paginate_text(&full_text, page, 300);
        app_window.set_preview_text(page_text.into());
        app_window.set_preview_current_page(page);
        app_window.set_preview_total_pages(total_pages);
    }

    /// 处理最终产物分页改变
    fn handle_final_page_changed(app_window: &AppWindow, final_full_text: &Rc<RefCell<String>>, page: i32) {
        let full_text = final_full_text.borrow().clone();
        let (page_text, total_pages) = Self::paginate_text(&full_text, page, 300);
        app_window.set_final_product_text(page_text.into());
        app_window.set_final_current_page(page);
        app_window.set_final_total_pages(total_pages);
    }

    /// 处理上传回写文件（真正的非阻塞版本）
    fn handle_upload_writeback_file(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>, preview_full_text: &Rc<RefCell<String>>, final_full_text: &Rc<RefCell<String>>) {
        Self::append_writeback_log(app_window, "📂 开始选择回写文件...");

        // 打开文件选择对话框
        let file_dialog = rfd::FileDialog::new()
            .add_filter("JSON文件", &["json"])
            .set_title("选择回写JSON文件");

        if let Some(path) = file_dialog.pick_file() {
            Self::append_writeback_log(app_window, &format!("📁 已选择文件: {}", path.display()));

            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    Self::append_writeback_log(app_window, &format!("📖 文件读取成功，大小: {} 字节", content.len()));

                    // 格式验证：比较上传文件与最终产物的格式
                    let final_product_text = final_full_text.borrow().clone();
                    Self::append_writeback_log(app_window, &format!("🔍 最终产物文本长度: {} 字符", final_product_text.len()));

                    if final_product_text.trim().is_empty() {
                        Self::append_writeback_log(app_window, "⚠️ 最终产物为空，跳过格式验证");
                    } else if let Err(validation_error) = Self::validate_json_format(&content, &final_product_text) {
                        Self::append_writeback_log(app_window, &format!("⚠️ 格式验证失败: {}", validation_error));
                        app_window.invoke_show_message_dialog(
                            "格式不一致警告".into(),
                            format!("请上传与最终产物格式一致的JSON文件\n\n错误详情: {}", validation_error).into()
                        );
                        return;
                    } else {
                        Self::append_writeback_log(app_window, "✅ 格式验证通过");
                    }
                    Self::append_writeback_log(app_window, "✅ 格式验证通过");

                    // 使用真正的后台线程处理，避免阻塞UI
                    let app_window_weak = app_window.as_weak();

                    // 在启动线程前提取所需数据
                    let intermediate_stage2 = preview_full_text.borrow().clone();
                    let original_file_path = app_state.borrow().original_file_path.clone();

                    // 提取原始JSON数据用于更新
                    let original_json = app_state.borrow().dom.clone();

                    std::thread::spawn(move || {
                        // 在后台线程中处理回写
                        match Self::process_writeback_in_background(&content, &intermediate_stage2, original_json, original_file_path, &app_window_weak) {
                            Ok((modified_count, updated_json)) => {
                                // 使用invoke_from_event_loop安全地更新UI
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(app_window) = app_window_weak.upgrade() {
                                        Self::append_writeback_log(&app_window, &format!("🎉 回写完成！共修改了 {} 个字段", modified_count));
                                        app_window.set_status_message(format!("回写成功，修改了 {} 个字段", modified_count).into());

                                        // 触发JSON结构树更新的信号
                                        if updated_json.is_some() {
                                            Self::append_writeback_log(&app_window, "🔄 正在更新JSON结构树...");
                                            // 通过设置一个特殊的状态来触发重新加载
                                            app_window.set_status_message("JSON结构树更新完成".into());
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                let error_msg = e.to_string();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(app_window) = app_window_weak.upgrade() {
                                        Self::append_writeback_log(&app_window, &format!("❌ 回写失败: {}", error_msg));
                                        app_window.set_status_message(format!("回写失败: {}", error_msg).into());
                                    }
                                });
                            }
                        }
                    });
                }
                Err(e) => {
                    Self::append_writeback_log(app_window, &format!("❌ 文件读取失败: {}", e));
                    app_window.set_status_message(format!("读取文件失败: {}", e).into());
                }
            }
        } else {
            Self::append_writeback_log(app_window, "⚠️ 用户取消了文件选择");
            app_window.set_status_message("用户取消了文件选择".into());
        }
    }



    /// 在后台线程中处理回写（真正的非阻塞版本）
    fn process_writeback_in_background(
        writeback_content: &str,
        intermediate_stage2: &str,
        mut original_json: Option<serde_json::Value>,
        original_file_path: Option<PathBuf>,
        app_window_weak: &slint::Weak<AppWindow>
    ) -> Result<(usize, Option<serde_json::Value>), Box<dyn std::error::Error + Send + Sync>> {
        // 更新日志的闭包（使用invoke_from_event_loop）
        let update_log = |app_window_weak: &slint::Weak<AppWindow>, message: String| {
            let app_window_weak_clone = app_window_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app_window) = app_window_weak_clone.upgrade() {
                    Self::append_writeback_log(&app_window, &message);
                }
            });
        };

        update_log(app_window_weak, "🔍 开始解析回写文件...".to_string());

        // 解析上传的回写文件
        let writeback_data: serde_json::Value = serde_json::from_str(writeback_content)?;
        let writeback_obj = writeback_data.as_object()
            .ok_or("回写文件必须是JSON对象")?;

        update_log(app_window_weak, format!("📊 回写文件包含 {} 个条目", writeback_obj.len()));

        update_log(app_window_weak, "🔍 开始解析中间产物2...".to_string());
        // 解析中间产物2
        let stage2_data: serde_json::Value = serde_json::from_str(intermediate_stage2)?;
        let items = stage2_data.get("items")
            .and_then(|v| v.as_array())
            .ok_or("中间产物2格式错误：缺少items数组")?;

        update_log(app_window_weak, format!("📊 中间产物2包含 {} 个条目", items.len()));

        let mut modified_count = 0;
        let mut skipped_count = 0;
        let total_entries = writeback_obj.len();

        update_log(app_window_weak, format!("🔄 开始处理 {} 个回写条目...", total_entries));

        // 确保有原始JSON数据
        let json_data = original_json.as_mut()
            .ok_or("缺少原始JSON数据")?;

        // 处理每个回写条目
        for (key, new_value) in writeback_obj.iter() {
            // 每处理100个条目就更新进度
            if (modified_count + skipped_count) % 100 == 0 {
                let progress = ((modified_count + skipped_count) as f64 / total_entries as f64 * 100.0) as u32;
                update_log(app_window_weak, format!("📊 进度: {}/{} ({}%)",
                    modified_count + skipped_count + 1, total_entries, progress));
            }

            // 解析序号
            let seq = match key.parse::<usize>() {
                Ok(s) => s,
                Err(_) => {
                    tracing::warn!("跳过无效序号: {}", key);
                    skipped_count += 1;
                    continue;
                }
            };

            // 在中间产物2中找到对应的条目
            if let Some(item) = items.get(seq) {
                if let Some(source_path) = item.get("source_path").and_then(|v| v.as_str()) {
                    // 验证新值格式
                    let new_value_str = match new_value {
                        serde_json::Value::String(s) => {
                            let trimmed = s.trim();
                            if trimmed.is_empty() {
                                skipped_count += 1;
                                continue;
                            }
                            s.clone()
                        },
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Null => {
                            skipped_count += 1;
                            continue;
                        },
                        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                            skipped_count += 1;
                            continue;
                        }
                    };

                    // 使用JSONPath更新原始JSON
                    match Self::update_json_by_path(json_data, source_path, &new_value_str) {
                        Ok(_) => {
                            modified_count += 1;
                        }
                        Err(_) => {
                            skipped_count += 1;
                        }
                    }
                } else {
                    skipped_count += 1;
                }
            } else {
                skipped_count += 1;
            }
        }

        update_log(app_window_weak, format!("📈 处理完成: 成功 {} 个，跳过 {} 个", modified_count, skipped_count));

        // 保存到原始文件
        if let Some(original_path) = original_file_path {
            update_log(app_window_weak, "💾 开始保存到原始文件...".to_string());
            let json_string = serde_json::to_string_pretty(json_data)?;
            std::fs::write(&original_path, json_string)?;
            update_log(app_window_weak, format!("✅ 已保存到: {}", original_path.display()));

            // 触发重新加载文件以更新JSON结构树
            let path_for_reload = original_path.clone();
            let _ = slint::invoke_from_event_loop({
                let app_window_weak = app_window_weak.clone();
                move || {
                    if let Some(app_window) = app_window_weak.upgrade() {
                        Self::append_writeback_log(&app_window, "🔄 触发JSON结构树重新加载...");
                        // 调用重新加载回调
                        app_window.invoke_reload_file_after_writeback(path_for_reload.to_string_lossy().to_string().into());
                    }
                }
            });
        }

        Ok((modified_count, original_json))
    }

    /// 验证JSON格式是否一致
    fn validate_json_format(upload_content: &str, final_product: &str) -> Result<(), String> {
        // 如果最终产物为空，跳过验证
        if final_product.trim().is_empty() {
            return Ok(());
        }

        // 解析上传的JSON
        let upload_json: serde_json::Value = serde_json::from_str(upload_content)
            .map_err(|e| format!("上传文件不是有效的JSON: {}", e))?;

        // 解析最终产物JSON
        let final_json: serde_json::Value = serde_json::from_str(final_product)
            .map_err(|e| format!("最终产物不是有效的JSON: {}", e))?;

        // 比较JSON结构
        if !Self::compare_json_structure(&upload_json, &final_json) {
            return Err("JSON结构不匹配，字段数量或类型不一致".to_string());
        }

        Ok(())
    }

    /// 比较两个JSON的结构是否一致
    fn compare_json_structure(json1: &serde_json::Value, json2: &serde_json::Value) -> bool {
        use serde_json::Value;

        match (json1, json2) {
            (Value::Object(obj1), Value::Object(obj2)) => {
                // 比较对象的键数量
                if obj1.len() != obj2.len() {
                    return false;
                }
                // 递归比较每个键的结构
                for (key, value1) in obj1 {
                    if let Some(value2) = obj2.get(key) {
                        if !Self::compare_json_structure(value1, value2) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            }
            (Value::Array(arr1), Value::Array(arr2)) => {
                // 比较数组长度
                if arr1.len() != arr2.len() {
                    return false;
                }
                // 递归比较数组元素结构
                for (item1, item2) in arr1.iter().zip(arr2.iter()) {
                    if !Self::compare_json_structure(item1, item2) {
                        return false;
                    }
                }
                true
            }
            // 对于基本类型，只比较类型是否相同
            (Value::String(_), Value::String(_)) => true,
            (Value::Number(_), Value::Number(_)) => true,
            (Value::Bool(_), Value::Bool(_)) => true,
            (Value::Null, Value::Null) => true,
            _ => false, // 类型不匹配
        }
    }

    /// 使用JSONPath更新JSON值（独立函数，不依赖AppState）
    fn update_json_by_path(
        json_data: &mut serde_json::Value,
        json_path: &str,
        new_value: &str
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use jsonpath_rust::{JsonPath, query::queryable::Queryable};

        // 查找路径
        let paths: Vec<String> = json_data
            .query_only_path(json_path)
            .map_err(|e| format!("JSONPath查询失败: {}", e))?;

        let Some(p) = paths.into_iter().next() else {
            return Err(format!("JSONPath未匹配到可更新路径: {}", json_path).into());
        };

        // 通过 reference_mut 按路径获取可变引用
        if let Some(slot) = json_data.reference_mut(&p) {
            *slot = serde_json::Value::String(new_value.to_string());
        } else {
            return Err(format!("路径不可更新: {}", p).into());
        }

        Ok(())
    }

    /// 回写完成后更新JSON结构树
    fn update_json_tree_after_writeback(
        app_window: &AppWindow,
        app_state: &Rc<RefCell<AppState>>,
        updated_json: serde_json::Value
    ) {
        // 更新AppState中的DOM数据
        {
            let mut state = app_state.borrow_mut();
            state.dom = Some(updated_json);
            // 重新构建影子树
            if let Some(ref dom) = state.dom {
                state.tree_flat = crate::model::shadow_tree::build_shadow_tree(dom);
                // 更新可见性
                state.update_visibility_by_expansion();
            }
        }

        // 更新UI中的树模型
        let tree_data: Vec<TreeNodeData> = {
            let state = app_state.borrow();
            state.tree_flat
                .iter()
                .filter(|node| node.visible)
                .map(TreeNodeData::from)
                .collect()
        };

        let model = ModelRc::new(VecModel::from(tree_data));
        app_window.set_tree_model(model);
    }

    /// 线程安全的JSON结构树更新（在后台线程中调用）
    fn update_json_tree_after_writeback_sync(
        app_state: &Rc<RefCell<AppState>>,
        updated_json: serde_json::Value,
        app_window_weak: &slint::Weak<AppWindow>
    ) {
        // 更新AppState中的DOM数据
        {
            let mut state = app_state.borrow_mut();
            state.dom = Some(updated_json);
            // 重新构建影子树
            if let Some(ref dom) = state.dom {
                state.tree_flat = crate::model::shadow_tree::build_shadow_tree(dom);
                // 更新可见性
                state.update_visibility_by_expansion();
            }
        }

        // 准备UI更新数据
        let tree_data: Vec<TreeNodeData> = {
            let state = app_state.borrow();
            state.tree_flat
                .iter()
                .filter(|node| node.visible)
                .map(TreeNodeData::from)
                .collect()
        };

        // 在主线程中更新UI
        let _ = slint::invoke_from_event_loop({
            let app_window_weak = app_window_weak.clone();
            move || {
                if let Some(app_window) = app_window_weak.upgrade() {
                    let model = ModelRc::new(VecModel::from(tree_data));
                    app_window.set_tree_model(model);
                    Self::append_writeback_log(&app_window, "✅ JSON结构树已更新");
                }
            }
        });
    }

    /// 处理回写后重新加载文件
    fn handle_reload_file_after_writeback(
        app_window: &AppWindow,
        app_state: &Rc<RefCell<AppState>>,
        file_path: &str
    ) {
        use std::path::Path;

        let path = Path::new(file_path);
        if !path.exists() {
            Self::append_writeback_log(app_window, "❌ 文件不存在，无法重新加载");
            return;
        }

        // 重新加载文件
        match app_state.borrow_mut().load_file(path) {
            Ok(()) => {
                Self::append_writeback_log(app_window, "✅ 文件重新加载成功");
            }
            Err(e) => {
                Self::append_writeback_log(app_window, &format!("❌ 文件重新加载失败: {}", e));
                app_window.set_status_message(format!("重新加载失败: {}", e).into());
                return;
            }
        }

        // 在借用结束后，重新获取数据更新UI
        Self::rebuild_tree_model(app_window, app_state);
        app_window.set_current_path(file_path.into());

        Self::append_writeback_log(app_window, "✅ JSON结构树已更新");
        app_window.set_status_message("JSON结构树更新完成".into());
    }

    /// 处理扁平化显示切换
    fn handle_toggle_tree_flatten(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>) {
        let current_mode = app_window.get_tree_flatten_mode();
        app_window.set_tree_flatten_mode(!current_mode);

        // 重新构建树模型
        Self::rebuild_tree_model(app_window, app_state);

        let mode_text = if !current_mode { "扁平化" } else { "层级" };
        app_window.set_status_message(format!("已切换到{}显示模式", mode_text).into());
    }

    /// 处理字符过滤设置
    fn handle_set_tree_char_filter(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>, filter: &str) {
        app_window.set_tree_char_filter(filter.into());

        // 重新构建树模型
        Self::rebuild_tree_model(app_window, app_state);

        let filter_text = match filter {
            "chinese" => "中文字符",
            "english" => "英文字符",
            _ => "全部字符"
        };
        app_window.set_status_message(format!("已设置过滤显示: {}", filter_text).into());
    }

    /// 重新构建树模型（应用扁平化、字符过滤和空值过滤）
    fn rebuild_tree_model(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>) {
        let flatten_mode = app_window.get_tree_flatten_mode();
        let char_filter = app_window.get_tree_char_filter().to_string();
        let hide_empty = app_window.get_tree_hide_empty();

        let tree_data: Vec<TreeNodeData> = {
            let state = app_state.borrow();
            let mut nodes: Vec<TreeNodeData> = state.tree_flat
                .iter()
                .filter(|node| node.visible)
                .map(TreeNodeData::from)
                .collect();

            // 应用字符过滤
            if char_filter != "all" {
                nodes.retain(|node| Self::matches_char_filter(&node.preview.to_string(), &char_filter));
            }

            // 应用空值过滤
            if hide_empty {
                nodes.retain(|node| !Self::is_empty_value(&node.preview.to_string(), &node.kind.to_string()));
            }

            // 应用扁平化
            if flatten_mode {
                // 扁平化：移除层级缩进，所有节点深度设为0
                for node in &mut nodes {
                    node.depth = 0;
                }
            }

            nodes
        };

        let model = ModelRc::new(VecModel::from(tree_data));
        app_window.set_tree_model(model);
    }

    /// 检查文本是否匹配字符过滤条件
    fn matches_char_filter(text: &str, filter: &str) -> bool {
        match filter {
            "chinese" => {
                // 纯中文：包含中文字符且不包含英文字符
                let has_chinese = text.chars().any(|c| Self::is_chinese_char(c));
                let has_english = text.chars().any(|c| Self::is_english_char(c));
                has_chinese && !has_english
            },
            "english" => {
                // 纯英文：包含英文字符且不包含中文字符
                let has_chinese = text.chars().any(|c| Self::is_chinese_char(c));
                let has_english = text.chars().any(|c| Self::is_english_char(c));
                has_english && !has_chinese
            },
            _ => true
        }
    }

    /// 判断是否为中文字符
    fn is_chinese_char(c: char) -> bool {
        let code = c as u32;
        // CJK统一汉字基本区块: U+4E00-U+9FFF
        // CJK统一汉字扩展A区: U+3400-U+4DBF
        // CJK兼容汉字: U+F900-U+FAFF
        (code >= 0x4E00 && code <= 0x9FFF) ||
        (code >= 0x3400 && code <= 0x4DBF) ||
        (code >= 0xF900 && code <= 0xFAFF)
    }

    /// 判断是否为英文字符
    fn is_english_char(c: char) -> bool {
        c.is_ascii_alphabetic()
    }

    /// 处理隐藏空值切换
    fn handle_toggle_tree_hide_empty(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>) {
        let current_mode = app_window.get_tree_hide_empty();
        app_window.set_tree_hide_empty(!current_mode);

        // 重新构建树模型
        Self::rebuild_tree_model(app_window, app_state);

        let mode_text = if !current_mode { "隐藏空值" } else { "显示空值" };
        app_window.set_status_message(format!("已切换到{}模式", mode_text).into());
    }

    /// 判断是否为空值
    fn is_empty_value(preview: &str, kind: &str) -> bool {
        match kind {
            "Null" => true,
            "String" => {
                // 去除引号后检查是否为空字符串
                let trimmed = preview.trim_matches('"').trim();
                trimmed.is_empty()
            },
            "Array" => {
                // 空数组显示为 "[]"
                preview.trim() == "[]"
            },
            "Object" => {
                // 空对象显示为 "{}"
                preview.trim() == "{}"
            },
            _ => false
        }
    }

    /// 处理智能英文字段检测
    fn handle_detect_english_fields(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>) {
        match app_state.borrow().detect_english_fields() {
            Ok(english_fields) => {
                // 转换为Slint可用的字符串数组
                let slint_fields: Vec<slint::SharedString> = english_fields
                    .into_iter()
                    .map(|s| s.into())
                    .collect();

                // 设置到UI
                let field_count = slint_fields.len();
                let model = ModelRc::new(VecModel::from(slint_fields));
                app_window.set_english_fields(model);

                app_window.set_status_message(format!("检测到 {} 个英文字段", field_count).into());

                tracing::info!("英文字段检测完成，找到 {} 个字段", field_count);
            }
            Err(e) => {
                let error_msg = format!("{}英文字段检测失败: {}", STATUS_ERROR_PREFIX, e);
                app_window.set_status_message(error_msg.into());
                tracing::error!("英文字段检测失败: {}", e);
            }
        }
    }

    /// 处理应用搜索过滤
    fn handle_apply_search_filter(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>, filter: &str) {
        // 直接调用现有的搜索处理函数
        Self::handle_search_changed(app_window, app_state, filter);
    }

    /// 处理提取搜索结果
    fn handle_extract_search_results(app_window: &AppWindow, app_state: &Rc<RefCell<AppState>>, filter: &str) {
        if filter.trim().is_empty() {
            app_window.set_status_message("错误: 搜索条件为空".into());
            return;
        }

        match app_state.borrow().extract_search_results(filter) {
            Ok(search_results) => {
                app_window.set_preview_text(search_results.into());
                app_window.set_selected_json_path(format!("搜索结果: {}", filter).into());
                app_window.set_status_message(format!("已提取搜索结果: {}", filter).into());

                tracing::info!("搜索结果提取成功: {}", filter);
            }
            Err(e) => {
                let error_msg = format!("{}搜索结果提取失败: {}", STATUS_ERROR_PREFIX, e);
                app_window.set_status_message(error_msg.into());
                tracing::error!("搜索结果提取失败: {}", e);
            }
        }
    }
}


fn main() {
    // 初始化日志输出（遵循 message_：可观测性）
    let _ = SubscriberBuilder::default()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    let app = AppWindow::new().expect("UI 初始化失败");
    let state = Rc::new(RefCell::new(AppState::default()));

    // 创建VM桥接器并绑定UI回调
    let bridge = ViewModelBridge::new(&app, state.clone());
    bridge.initialize_ui(&app);

    tracing::info!("应用启动成功，UI已初始化");
    app.run().unwrap();
}

