//! AppState：应用核心状态与JSONPath读写

use std::path::{Path, PathBuf};

use jsonpath_rust::{JsonPath, query::queryable::Queryable}; // 提供 query/query_only_path/reference_mut 等扩展
use serde_json::Value;
use thiserror::Error;

use std::collections::HashSet;

use crate::model::shadow_tree::{build_shadow_tree, NodeKind};
use crate::utils::fs::{read_json_file, write_json_file};

#[derive(Debug, Default)]
pub struct AppState {
    pub source_path: Option<PathBuf>,
    pub original_file_path: Option<PathBuf>,
    pub dom: Option<Value>,
    pub tree_flat: Vec<crate::model::shadow_tree::JsonTreeNode>,
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error("IO失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON解析失败: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("JSONPath错误: {0}")]
    JsonPath(String),
    #[error("状态错误: {0}")]
    State(String),
}

impl AppState {
    /// 加载JSON文件并构建影子树
    pub fn load_file(&mut self, p: &Path) -> Result<(), AppError> {
        let dom = read_json_file(p)?;
        self.tree_flat = build_shadow_tree(&dom);
        self.source_path = Some(p.to_path_buf());
        self.original_file_path = Some(p.to_path_buf()); // 设置原始文件路径
        self.dom = Some(dom);
        Ok(())
    }

    /// 按 JSONPath 提取第一个匹配节点的 pretty 字符串
    pub fn extract_subtree_pretty(&self, json_path: &str) -> Result<String, AppError> {
        let dom = self
            .dom
            .as_ref()
            .ok_or_else(|| AppError::State("DOM尚未加载".into()))?;
        let hits: Vec<&Value> = dom
            .query(json_path)
            .map_err(|e| AppError::JsonPath(e.to_string()))?;
        let first = hits
            .into_iter()
            .next()
            .ok_or_else(|| AppError::JsonPath("未匹配到任何节点".into()))?;
        Ok(serde_json::to_string_pretty(first)?)
    }

    /// 将 new_json 替换到第一个匹配的 json_path 节点
    pub fn update_node_from_str(&mut self, json_path: &str, new_json: &str) -> Result<(), AppError> {
        let dom = self
            .dom
            .as_mut()
            .ok_or_else(|| AppError::State("DOM尚未加载".into()))?;

        let paths: Vec<String> = dom
            .query_only_path(json_path)
            .map_err(|e| AppError::JsonPath(e.to_string()))?;
        let Some(p) = paths.into_iter().next() else {
            return Err(AppError::JsonPath("未匹配到可更新路径".into()));
        };

        // 对于字符串值，直接设置为JSON字符串值，不需要解析
        let replacement: Value = Value::String(new_json.to_string());
        // 通过 reference_mut 按路径获取可变引用（支持 root/field/index 直接访问段）
        if let Some(slot) = dom.reference_mut(&p) {
            *slot = replacement;
        } else {
            return Err(AppError::JsonPath(format!("路径不可更新: {}", p)));
        }

        // 变更后重建影子树（后续可优化为局部刷新）
        self.tree_flat = build_shadow_tree(dom);
        Ok(())
    }

    /// 将当前DOM保存到指定路径
    pub fn save_to_file(&self, path: &Path) -> Result<(), AppError> {
        let dom = self
            .dom
            .as_ref()
            .ok_or_else(|| AppError::State("DOM尚未加载".into()))?;
        write_json_file(path, dom)?;
        Ok(())
    }

    /// 将当前DOM保存到原始文件路径
    pub fn save_to_original_file(&self) -> Result<(), AppError> {
        let original_path = self
            .original_file_path
            .as_ref()
            .ok_or_else(|| AppError::State("原始文件路径未设置".into()))?;
        self.save_to_file(original_path)
    }

    /// 应用搜索过滤，只显示匹配路径的节点
    pub fn apply_search_filter(&mut self, filter: &str) {
        if filter.trim().is_empty() {
            // 清空过滤，显示所有节点
            for node in &mut self.tree_flat {
                node.visible = true;
            }
        } else {
            // 简化的快速搜索 - 只做简单的字符串匹配
            for node in &mut self.tree_flat {
                node.visible = node.path.contains(filter) || node.name.contains(filter);
            }
        }
    }

    /// 提取搜索匹配的节点JSON内容，智能限制结果数量以优化性能
    pub fn extract_search_results(&self, filter: &str) -> Result<String, AppError> {
        if filter.trim().is_empty() {
            return Ok("".to_string());
        }

        // 确保DOM已加载
        self.dom
            .as_ref()
            .ok_or_else(|| AppError::State("DOM尚未加载".into()))?;

        // 收集所有匹配的可见节点
        let mut matched_nodes = Vec::new();
        for node in &self.tree_flat {
            if (node.path.contains(filter) || node.name.contains(filter)) && node.visible {
                matched_nodes.push(node);
            }
        }

        if matched_nodes.is_empty() {
            tracing::warn!("未找到匹配的可见节点，过滤条件: {}", filter);
            return Ok("{}".to_string());
        }

        tracing::info!("找到 {} 个匹配节点", matched_nodes.len());

        // 如果只有一个匹配节点，直接返回其完整内容
        if matched_nodes.len() == 1 {
            let node = matched_nodes[0];
            tracing::info!("单个匹配节点: {} (路径: {})", node.name, node.path);
            let result = self.extract_subtree_pretty(&node.path);
            tracing::info!("提取结果: {:?}", result.as_ref().map(|s| s.len()));
            return result;
        }

        // 多个匹配节点：复制全部场景需要完整输出（不限制数量、不截断内容）
        let display_count = matched_nodes.len();
        let mut search_results = serde_json::Map::new();

        for (index, node) in matched_nodes.iter().take(display_count).enumerate() {
            match self.extract_subtree_pretty(&node.path) {
                Ok(json_content) => {

                    // 解析JSON内容以便重新组织
                    match serde_json::from_str::<Value>(&json_content) {
                        Ok(parsed_value) => {
                            let result_key = format!("match_{}_{}", index + 1, node.name);
                            let result_entry = serde_json::json!({
                                "path": node.path,
                                "name": node.name,
                                "type": format!("{:?}", node.kind),
                                "content": parsed_value
                            });
                            search_results.insert(result_key, result_entry);
                        }
                        Err(_) => {
                            // 如果解析失败，直接存储为字符串
                            let result_key = format!("match_{}_{}", index + 1, node.name);
                            let result_entry = serde_json::json!({
                                "path": node.path,
                                "name": node.name,
                                "type": format!("{:?}", node.kind),
                                "content": json_content
                            });
                            search_results.insert(result_key, result_entry);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("提取节点 {} 失败: {}", node.path, e);
                    let result_key = format!("match_{}_{}_error", index + 1, node.name);
                    let result_entry = serde_json::json!({
                        "path": node.path,
                        "name": node.name,
                        "type": format!("{:?}", node.kind),
                        "error": e.to_string()
                    });
                    search_results.insert(result_key, result_entry);
                }
            }
        }

        let final_result = serde_json::json!({
            "search_filter": filter,
            "total_matches": matched_nodes.len(),
            "displayed_matches": display_count,
            "truncated": false,
            "results": search_results
        });

        let pretty_result = serde_json::to_string_pretty(&final_result)?;
        tracing::info!("搜索结果构建完成，显示 {}/{} 个匹配，总长度: {} 字符",
                      display_count, matched_nodes.len(), pretty_result.len());

        Ok(pretty_result)
    }

    /// 构建“中间产物 第二阶段”：按过滤条件枚举命中项，派生并提取同层级的 name 字段值，生成带连续序号的清单
    pub fn build_intermediate_stage2<F>(&self, filter: &str, mut progress_callback: F) -> Result<String, AppError>
    where
        F: FnMut(f32, &str),
    {
        self.build_intermediate_stage2_with_leaf_filter(filter, false, progress_callback)
    }

    /// 构建"中间产物 第二阶段"：支持叶子节点过滤的版本
    pub fn build_intermediate_stage2_with_leaf_filter<F>(&self, filter: &str, leaf_nodes_only: bool, mut progress_callback: F) -> Result<String, AppError>
    where
        F: FnMut(f32, &str),
    {
        if filter.trim().is_empty() {
            return Ok("".to_string());
        }

        tracing::info!("build_intermediate_stage2: 开始执行");
        progress_callback(0.1, "开始分析匹配节点...");
        tracing::info!("build_intermediate_stage2: 进度回调 0.1 调用完成");

        let dom = self
            .dom
            .as_ref()
            .ok_or_else(|| AppError::State("DOM尚未加载".into()))?;
        tracing::info!("build_intermediate_stage2: DOM获取成功");

        // 收集所有可见且匹配的节点
        let match_start = std::time::Instant::now();
        let mut matched: Vec<&crate::model::shadow_tree::JsonTreeNode> = Vec::new();
        for node in &self.tree_flat {
            // 应用叶子节点过滤逻辑
            let should_include = if leaf_nodes_only {
                // 叶子节点模式：只匹配属性名包含过滤条件的真正叶子节点（具有简单值的节点）
                node.visible && node.name.contains(filter) && matches!(node.kind, NodeKind::String | NodeKind::Number | NodeKind::Bool | NodeKind::Null)
            } else {
                // 全部节点模式：匹配路径或属性名包含过滤条件的节点
                node.visible && (node.path.contains(filter) || node.name.contains(filter))
            };

            if should_include {
                matched.push(node);
            }
        }
        let match_time = match_start.elapsed().as_millis();

        tracing::info!("build_intermediate_stage2: 找到 {} 个匹配节点，耗时: {}ms", matched.len(), match_time);
        // 优化：减少进度回调频率，直接跳到50%
        progress_callback(0.5, &format!("正在处理 {} 个匹配节点...", matched.len()));
        tracing::info!("build_intermediate_stage2: 进度回调 0.5 调用完成");

        // 派生 name 的 JSONPath
        fn derive_name_path(src: &str) -> Option<String> {
            // 寻找最后一个不在 [] 中的 '.'
            let mut depth = 0i32;
            let mut last_dot = None;
            for (i, ch) in src.chars().enumerate() {
                match ch {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    '.' if depth == 0 => last_dot = Some(i),
                    _ => {}
                }
            }
            if let Some(idx) = last_dot { Some(format!("{}{}", &src[..idx], ".name")) } else { None }
        }

        // 批量收集所有需要查询的路径，减少重复查询
        let mut path_to_value: std::collections::HashMap<String, Option<serde_json::Value>> = std::collections::HashMap::new();
        let mut paths_to_query: std::collections::HashSet<String> = std::collections::HashSet::new();

        // 收集所有需要查询的路径
        for node in &matched {
            paths_to_query.insert(node.path.clone());
            if node.name != "name" {
                if let Some(np) = derive_name_path(&node.path) {
                    paths_to_query.insert(np);
                }
            }
        }

        // 批量执行查询，缓存结果
        let query_start = std::time::Instant::now();
        progress_callback(0.5, "正在查询JSON路径...");
        for path in paths_to_query {
            let value = dom
                .query(&path)
                .map_err(|e| AppError::JsonPath(e.to_string()))?
                .into_iter()
                .next()
                .cloned();
            path_to_value.insert(path, value);
        }
        let query_time = query_start.elapsed().as_millis();
        tracing::info!("build_intermediate_stage2: JSON路径查询完成，耗时: {}ms", query_time);

        let mut items = Vec::<serde_json::Value>::new();
        let build_start = std::time::Instant::now();
        // 优化：减少进度回调，直接跳到90%
        progress_callback(0.9, "正在构建最终结果...");
        for node in matched {
            // 从缓存中获取当前节点的值
            let current_value_opt = path_to_value.get(&node.path).and_then(|v| v.clone());

            let current_value_str = match &current_value_opt {
                Some(val) => match val {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                },
                None => String::new(),
            };

            // 派生 name 字段路径和值（从缓存中获取）
            let (name_path, name_value_opt) = if node.name == "name" {
                // 本身就是 name 字段
                (node.path.clone(), current_value_opt.clone())
            } else if let Some(np) = derive_name_path(&node.path) {
                let v = path_to_value.get(&np).and_then(|v| v.clone());
                (np, v)
            } else {
                (node.path.clone(), None)
            };

            let name_value_str = match name_value_opt {
                Some(val) => match val {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                },
                None => String::new(),
            };

            items.push(serde_json::json!({
                // seq 在最终序列化时按索引补充
                "source_path": node.path,
                "name_path": name_path,
                "name": current_value_str,  // 使用查询字段的值，而不是 name 字段的值
                "field_name": node.name,    // 添加字段名信息
                "name_field_value": name_value_str,  // name 字段的值（用于参考）
            }));
        }

        // 生成带连续序号的 items（从 0 开始）
        let build_time = build_start.elapsed().as_millis();
        tracing::info!("build_intermediate_stage2: 结果项构建完成，耗时: {}ms", build_time);

        let seq_start = std::time::Instant::now();
        let items_with_seq: Vec<serde_json::Value> = items
            .into_iter()
            .enumerate()
            .map(|(i, mut obj)| {
                if let serde_json::Value::Object(ref mut map) = obj {
                    map.insert("seq".to_string(), serde_json::json!(i));
                }
                obj
            })
            .collect();
        let seq_time = seq_start.elapsed().as_millis();
        tracing::info!("build_intermediate_stage2: 序号添加完成，耗时: {}ms", seq_time);

        let format_start = std::time::Instant::now();
        // 优化：移除中间进度回调，减少UI更新频率
        let result = serde_json::json!({
            "stage": "intermediate2",
            "filter": filter,
            "count": items_with_seq.len(),
            "items": items_with_seq,
        });
        let format_time = format_start.elapsed().as_millis();
        tracing::info!("build_intermediate_stage2: JSON格式化完成，耗时: {}ms", format_time);

        progress_callback(1.0, "完成");
        tracing::info!("build_intermediate_stage2: 进度回调 1.0 调用完成");
        let result_str = serde_json::to_string_pretty(&result)?;
        tracing::info!("build_intermediate_stage2: 执行完成，返回结果");
        Ok(result_str)
    }

    /// 更新JSON中指定路径的值
    pub fn update_json_value(&mut self, path: &str, new_value: &str) -> Result<(), AppError> {
        // 直接使用现有的 update_node_from_str 方法
        self.update_node_from_str(path, new_value)
    }

    /// 保存修改后的JSON到文件
    pub fn save_modified_json(&self, path: &std::path::Path) -> Result<(), AppError> {
        let dom = self.dom.as_ref()
            .ok_or_else(|| AppError::State("DOM尚未加载".into()))?;

        let json_str = serde_json::to_string_pretty(dom)
            .map_err(|e| AppError::JsonPath(e.to_string()))?;

        std::fs::write(path, json_str)
            .map_err(|e| AppError::Io(e))?;

        tracing::info!("JSON文件已保存到: {}", path.display());
        Ok(())
    }

    /// 切换节点的展开状态
    pub fn toggle_node_expanded(&mut self, path: &str) {
        if let Some(node) = self.tree_flat.iter_mut().find(|n| n.path == path) {
            node.expanded = !node.expanded;
        }
        self.update_visibility_by_expansion();
    }

    /// 根据展开状态更新节点可见性
    pub fn update_visibility_by_expansion(&mut self) {
        // 首先标记所有节点为不可见（除了根节点）
        for (i, node) in self.tree_flat.iter_mut().enumerate() {
            if i == 0 {
                node.visible = true; // 根节点总是可见
            } else {
                node.visible = false;
            }
        }

        // 递归显示展开节点的直接子节点
        for i in 0..self.tree_flat.len() {
            if self.tree_flat[i].expanded && self.tree_flat[i].visible {
                let parent_depth = self.tree_flat[i].depth;
                // 显示直接子节点
                for j in (i + 1)..self.tree_flat.len() {
                    if self.tree_flat[j].depth == parent_depth + 1 {
                        self.tree_flat[j].visible = true;
                    } else if self.tree_flat[j].depth <= parent_depth {
                        break; // 已经超出了当前父节点的范围
                    }
                }
            }
        }
    }

    /// 智能检测JSON中的英文字段，返回纯英文的字段值列表
    pub fn detect_english_fields(&self, leaf_nodes_only: bool) -> Result<Vec<String>, AppError> {
        let dom = self
            .dom
            .as_ref()
            .ok_or_else(|| AppError::State("DOM尚未加载".into()))?;

        let mut english_fields = HashSet::new();

        // 递归遍历JSON值，提取英文属性名
        self.extract_english_keys(dom, &mut english_fields, leaf_nodes_only);

        // 转换为排序的向量，过滤掉不符合条件的字段
        let mut result: Vec<String> = english_fields
            .into_iter()
            .filter(|s| {
                let trimmed = s.trim();
                // 基本长度检查
                if trimmed.len() < 2 || trimmed.len() > 50 {
                    return false;
                }

                // 使用更精确的英文检测逻辑
                Self::is_pure_english_field(trimmed)
            })
            .collect();

        result.sort();
        result.dedup(); // 去重

        // 限制返回数量，避免UI过载
        if result.len() > 20 {
            result.truncate(20);
        }

        Ok(result)
    }

    /// 判断是否为纯英文字段名（排除时间格式、数字等）
    fn is_pure_english_field(s: &str) -> bool {
        // 必须包含至少一个英文字母
        let has_letter = s.chars().any(|c| c.is_ascii_alphabetic());
        if !has_letter {
            return false;
        }

        // 排除时间格式 (如: "2023-01-01", "12:34:56", "2023-01-01T12:34:56Z")
        if Self::is_time_format(s) {
            return false;
        }

        // 排除版本号格式 (如: "v1.2.3", "1.0.0")
        if Self::is_version_format(s) {
            return false;
        }

        // 排除主要包含数字的字符串 (如: "123abc")
        let letter_count = s.chars().filter(|c| c.is_ascii_alphabetic()).count();
        let digit_count = s.chars().filter(|c| c.is_ascii_digit()).count();
        if digit_count > letter_count {
            return false;
        }

        // 只允许英文字母、下划线、连字符（不允许数字、冒号等）
        s.chars().all(|c| c.is_ascii_alphabetic() || c == '_' || c == '-')
    }

    /// 判断是否为时间格式（优化版本，避免正则表达式性能问题）
    fn is_time_format(s: &str) -> bool {
        let len = s.len();

        // 快速长度检查
        if len < 8 || len > 30 {
            return false;
        }

        // 检查是否包含时间相关字符
        let has_time_chars = s.contains('-') || s.contains(':') || s.contains('T') || s.contains('Z');
        if !has_time_chars {
            return false;
        }

        // 简单模式匹配，避免复杂正则表达式
        // ISO 8601 格式: 2023-01-01T12:34:56
        if s.contains('T') && s.contains('-') && s.contains(':') {
            return true;
        }

        // 日期格式: 2023-01-01
        if s.matches('-').count() == 2 && len >= 8 && len <= 12 {
            let parts: Vec<&str> = s.split('-').collect();
            if parts.len() == 3 &&
               parts[0].len() == 4 && parts[0].chars().all(|c| c.is_ascii_digit()) &&
               parts[1].len() == 2 && parts[1].chars().all(|c| c.is_ascii_digit()) &&
               parts[2].len() == 2 && parts[2].chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }

        // 时间格式: 12:34:56
        if s.matches(':').count() == 2 && len >= 6 && len <= 10 {
            let parts: Vec<&str> = s.split(':').collect();
            if parts.len() == 3 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
                return true;
            }
        }

        false
    }

    /// 判断是否为版本号格式（优化版本，避免正则表达式性能问题）
    fn is_version_format(s: &str) -> bool {
        let len = s.len();

        // 快速长度检查
        if len < 3 || len > 20 {
            return false;
        }

        // 检查是否包含点号
        if !s.contains('.') {
            return false;
        }

        // 移除可能的v前缀
        let version_str = if s.starts_with('v') || s.starts_with('V') {
            &s[1..]
        } else {
            s
        };

        // 检查点号数量（1-3个点号是合理的版本号）
        let dot_count = version_str.matches('.').count();
        if dot_count < 1 || dot_count > 3 {
            return false;
        }

        // 检查是否为数字.数字格式
        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() >= 2 && parts.len() <= 4 {
            // 所有部分都应该是数字
            return parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
        }

        false
    }

    /// 判断是否为URL格式（优化版本，避免正则表达式性能问题）
    fn is_url_format(s: &str) -> bool {
        let len = s.len();

        // 快速长度检查
        if len < 7 || len > 2000 {  // 最短的URL如 http://a 至少7个字符
            return false;
        }

        // 检查是否以常见协议开头
        let lower_s = s.to_lowercase();
        if lower_s.starts_with("http://") ||
           lower_s.starts_with("https://") ||
           lower_s.starts_with("ftp://") ||
           lower_s.starts_with("ftps://") {
            // 检查协议后是否有域名部分
            let protocol_end = if lower_s.starts_with("https://") || lower_s.starts_with("ftps://") {
                8
            } else {
                7
            };

            if s.len() > protocol_end {
                let domain_part = &s[protocol_end..];
                // 域名部分应该包含至少一个点或者是localhost
                return domain_part.contains('.') || domain_part.starts_with("localhost");
            }
        }

        false
    }

    /// 判断是否为叶子节点（具有具体值的节点）
    fn is_leaf_node(value: &Value) -> bool {
        matches!(value,
            Value::String(_) |
            Value::Number(_) |
            Value::Bool(_) |
            Value::Null
        )
    }

    /// 递归提取JSON中的英文属性名（键名），只收集值为字符串且值不是时间格式的属性名
    /// 对于URL类型的属性值，直接提取URL本身而不是属性名
    fn extract_english_keys(
        &self,
        value: &Value,
        english_fields: &mut HashSet<String>,
        leaf_nodes_only: bool,
    ) {
        match value {
            Value::Array(arr) => {
                for item in arr {
                    self.extract_english_keys(item, english_fields, leaf_nodes_only);
                }
            }
            Value::Object(obj) => {
                for (key, val) in obj {
                    // 叶子节点过滤：如果开启了叶子节点模式，只处理叶子节点
                    let is_leaf = Self::is_leaf_node(val);

                    if !leaf_nodes_only || is_leaf {
                        // 只有当属性值是字符串且不是时间格式时，才收集键名或URL
                        if let Value::String(string_value) = val {
                            let trimmed_key = key.trim();
                            let trimmed_value = string_value.trim();

                            // 检查属性值是否为时间格式或版本号格式
                            if !trimmed_key.is_empty() &&
                               !Self::is_time_format(trimmed_value) &&
                               !Self::is_version_format(trimmed_value) {
                                // 如果属性值是URL，直接提取URL本身
                                if Self::is_url_format(trimmed_value) {
                                    english_fields.insert(trimmed_value.to_string());
                                } else {
                                    // 否则提取属性名
                                    english_fields.insert(trimmed_key.to_string());
                                }
                            }
                        }
                    }

                    // 递归检查子结构的键名（无论值是什么类型）
                    self.extract_english_keys(val, english_fields, leaf_nodes_only);
                }
            }
            _ => {} // 忽略其他类型（数字、布尔值、null、字符串值）
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// 创建临时JSON文件用于测试
    fn create_test_json_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("创建临时文件失败");
        file.write_all(content.as_bytes()).expect("写入临时文件失败");
        file
    }

    #[test]
    fn test_load_simple_json() {
        let json_content = r#"{"name": "test", "value": 42}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        let result = app_state.load_file(temp_file.path());

        assert!(result.is_ok(), "加载简单JSON应该成功");
        assert!(app_state.dom.is_some(), "DOM应该被加载");
        assert!(!app_state.tree_flat.is_empty(), "影子树应该被构建");
        assert_eq!(app_state.tree_flat.len(), 3, "应该有3个节点：根、name、value");
    }

    #[test]
    fn test_load_nested_json() {
        let json_content = r#"
        {
            "user": {
                "name": "张三",
                "age": 30,
                "address": {
                    "city": "北京",
                    "district": "朝阳区"
                }
            },
            "items": [1, 2, 3]
        }"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        let result = app_state.load_file(temp_file.path());

        assert!(result.is_ok(), "加载嵌套JSON应该成功");
        assert!(app_state.tree_flat.len() > 5, "嵌套结构应该产生多个节点");
    }

    #[test]
    fn test_extract_subtree() {
        let json_content = r#"{"user": {"name": "张三", "age": 30}}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        // 测试提取根节点
        let root_result = app_state.extract_subtree_pretty("$");
        assert!(root_result.is_ok(), "提取根节点应该成功");

        // 测试提取用户对象
        let user_result = app_state.extract_subtree_pretty("$.user");
        assert!(user_result.is_ok(), "提取用户对象应该成功");

        // 测试提取用户名
        let name_result = app_state.extract_subtree_pretty("$.user.name");
        assert!(name_result.is_ok(), "提取用户名应该成功");
        assert!(name_result.unwrap().contains("张三"), "结果应该包含用户名");
    }

    #[test]
    fn test_update_node() {
        let json_content = r#"{"user": {"name": "张三", "age": 30}}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        // 更新用户名
        let new_name = r#""李四""#;
        let result = app_state.update_node_from_str("$.user.name", new_name);
        assert!(result.is_ok(), "更新节点应该成功");

        // 验证更新结果
        let updated_name = app_state.extract_subtree_pretty("$.user.name").unwrap();
        assert!(updated_name.contains("李四"), "用户名应该被更新为李四");
    }

    #[test]
    fn test_invalid_json_path() {
        let json_content = r#"{"user": {"name": "张三"}}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        // 测试无效路径
        let result = app_state.extract_subtree_pretty("$.nonexistent");
        assert!(result.is_err(), "无效路径应该返回错误");
    }

    #[test]
    fn test_invalid_json_content() {
        let invalid_json = r#"{"invalid": json content}"#;
        let temp_file = create_test_json_file(invalid_json);

        let mut app_state = AppState::default();
        let result = app_state.load_file(temp_file.path());

        assert!(result.is_err(), "无效JSON应该返回错误");
    }

    #[test]
    fn test_search_results_single_match() {
        let json_content = r#"{"user": {"name": "张三", "age": 30}, "config": {"debug": true}}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        // 应用搜索过滤
        app_state.apply_search_filter("name");

        // 提取搜索结果
        let result = app_state.extract_search_results("name");
        assert!(result.is_ok(), "搜索结果提取应该成功");

        let search_result = result.unwrap();
        assert!(search_result.contains("张三"), "搜索结果应该包含匹配的内容");
        println!("单个匹配搜索结果: {}", search_result);
    }

    #[test]
    fn test_search_results_multiple_matches() {
        let json_content = r#"{"users": [{"name": "张三", "description": "用户1"}, {"name": "李四", "description": "用户2"}], "metadata": {"description": "用户数据"}}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        // 应用搜索过滤
        app_state.apply_search_filter("description");

        // 提取搜索结果
        let result = app_state.extract_search_results("description");
        assert!(result.is_ok(), "搜索结果提取应该成功");

        let search_result = result.unwrap();
        assert!(search_result.contains("search_filter"), "搜索结果应该包含搜索信息");
        assert!(search_result.contains("total_matches"), "搜索结果应该包含匹配数量");
        assert!(search_result.contains("displayed_matches"), "搜索结果应该包含显示数量");
        println!("多个匹配搜索结果: {}", search_result);
    }

    #[test]
    fn test_update_node_type_change() {
        let json_content = r#"{"data": {"value": "原始字符串"}}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        // 将字符串替换为对象
        let new_object = r#"{"name": "新对象", "id": 123}"#;
        let result = app_state.update_node_from_str("$.data.value", new_object);
        assert!(result.is_ok(), "类型变更应该成功");

        // 验证更新结果
        let updated_value = app_state.extract_subtree_pretty("$.data.value").unwrap();
        assert!(updated_value.contains("新对象"), "应该包含新对象的内容");
        assert!(updated_value.contains("123"), "应该包含新对象的ID");
    }

    #[test]
    fn test_update_array_element() {
        let json_content = r#"{"items": ["第一项", "第二项", "第三项"]}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        // 更新数组中的第二个元素
        let new_item = r#""更新的第二项""#;
        let result = app_state.update_node_from_str("$.items[1]", new_item);
        assert!(result.is_ok(), "数组元素更新应该成功");

        // 验证更新结果
        let updated_item = app_state.extract_subtree_pretty("$.items[1]").unwrap();
        assert!(updated_item.contains("更新的第二项"), "数组元素应该被更新");

        // 验证其他元素未受影响
        let first_item = app_state.extract_subtree_pretty("$.items[0]").unwrap();
        assert!(first_item.contains("第一项"), "第一项应该保持不变");
    }

    #[test]
    fn test_update_with_invalid_json() {
        let json_content = r#"{"data": "原始值"}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        // 尝试用无效JSON更新
        let invalid_json = r#"{"invalid": json content"#;
        let result = app_state.update_node_from_str("$.data", invalid_json);
        assert!(result.is_err(), "无效JSON应该导致更新失败");

        // 验证原始值未被修改
        let original_value = app_state.extract_subtree_pretty("$.data").unwrap();
        assert!(original_value.contains("原始值"), "原始值应该保持不变");
    }

    #[test]
    fn test_update_nonexistent_path() {
        let json_content = r#"{"data": "值"}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        // 尝试更新不存在的路径
        let new_value = r#""新值""#;
        let result = app_state.update_node_from_str("$.nonexistent.path", new_value);
        assert!(result.is_err(), "不存在的路径应该导致更新失败");
    }

    #[test]
    fn test_shadow_tree_rebuild_after_update() {
        let json_content = r#"{"user": {"name": "张三", "age": 30}}"#;
        let temp_file = create_test_json_file(json_content);

        let mut app_state = AppState::default();
        app_state.load_file(temp_file.path()).expect("加载文件失败");

        let original_tree_len = app_state.tree_flat.len();

        // 将简单值替换为复杂对象
        let complex_object = r#"{"profile": {"bio": "个人简介", "skills": ["Rust", "JSON"]}}"#;
        let result = app_state.update_node_from_str("$.user.name", complex_object);
        assert!(result.is_ok(), "复杂对象更新应该成功");

        // 验证影子树被重建且节点数量发生变化
        let new_tree_len = app_state.tree_flat.len();
        assert_ne!(original_tree_len, new_tree_len, "影子树应该被重建");

        // 验证新路径存在
        let bio_result = app_state.extract_subtree_pretty("$.user.name.profile.bio");
        assert!(bio_result.is_ok(), "新的嵌套路径应该可访问");
    }
}

