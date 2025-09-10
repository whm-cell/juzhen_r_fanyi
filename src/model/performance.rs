//! 性能基准测试模块
//! 
//! 用于测试大文件加载、影子树构建和回写操作的性能
//! 遵循NFR要求：50MB文件≤5秒，UI响应≤200ms，内存≤3倍文件大小

use std::time::Instant;
use serde_json::{json, Value};
use crate::model::{data_core::AppState, shadow_tree::build_shadow_tree};

/// 性能测试结果
#[derive(Debug)]
pub struct PerformanceResult {
    pub operation: String,
    pub duration_ms: u128,
    pub memory_usage_mb: Option<f64>,
    pub success: bool,
    pub details: String,
}

impl PerformanceResult {
    pub fn new(operation: &str, duration_ms: u128, success: bool, details: &str) -> Self {
        Self {
            operation: operation.to_string(),
            duration_ms,
            memory_usage_mb: None,
            success,
            details: details.to_string(),
        }
    }
}

/// 生成大型测试JSON数据
pub fn generate_large_json(depth: usize, width: usize) -> Value {
    fn create_nested_object(current_depth: usize, max_depth: usize, width: usize) -> Value {
        if current_depth >= max_depth {
            return json!("叶子节点值");
        }
        
        let mut obj = serde_json::Map::new();
        
        // 添加各种类型的字段
        for i in 0..width {
            let key = format!("field_{}", i);
            let value = match i % 5 {
                0 => json!(format!("字符串值_{}", i)),
                1 => json!(i as i64),
                2 => json!(i % 2 == 0),
                3 => json!([1, 2, 3, i]),
                4 => create_nested_object(current_depth + 1, max_depth, width / 2),
                _ => json!(null),
            };
            obj.insert(key, value);
        }
        
        Value::Object(obj)
    }
    
    let mut root = serde_json::Map::new();
    root.insert("metadata".to_string(), json!({
        "generated_at": "2025-01-09T10:00:00Z",
        "depth": depth,
        "width": width,
        "description": "性能测试用大型JSON文档"
    }));
    
    root.insert("data".to_string(), create_nested_object(0, depth, width));
    
    // 添加大型数组
    let large_array: Vec<Value> = (0..width * 10)
        .map(|i| json!({
            "id": i,
            "name": format!("项目_{}", i),
            "value": i * 2,
            "active": i % 3 == 0
        }))
        .collect();
    root.insert("items".to_string(), json!(large_array));
    
    Value::Object(root)
}

/// 测试影子树构建性能
pub fn benchmark_shadow_tree_build(json_data: &Value) -> PerformanceResult {
    let start = Instant::now();
    let tree = build_shadow_tree(json_data);
    let duration = start.elapsed();
    
    let success = !tree.is_empty();
    let details = format!("构建了 {} 个节点", tree.len());
    
    PerformanceResult::new(
        "影子树构建",
        duration.as_millis(),
        success,
        &details
    )
}

/// 测试JSON解析性能
pub fn benchmark_json_parsing(json_str: &str) -> PerformanceResult {
    let start = Instant::now();
    let parse_result = serde_json::from_str::<Value>(json_str);
    let duration = start.elapsed();
    
    match parse_result {
        Ok(_) => PerformanceResult::new(
            "JSON解析",
            duration.as_millis(),
            true,
            &format!("解析了 {} 字节的JSON", json_str.len())
        ),
        Err(e) => PerformanceResult::new(
            "JSON解析",
            duration.as_millis(),
            false,
            &format!("解析失败: {}", e)
        )
    }
}

/// 测试节点提取性能
pub fn benchmark_node_extraction(app_state: &AppState, paths: &[&str]) -> Vec<PerformanceResult> {
    let mut results = Vec::new();
    
    for path in paths {
        let start = Instant::now();
        let extract_result = app_state.extract_subtree_pretty(path);
        let duration = start.elapsed();
        
        match extract_result {
            Ok(json_str) => {
                results.push(PerformanceResult::new(
                    &format!("节点提取: {}", path),
                    duration.as_millis(),
                    true,
                    &format!("提取了 {} 字符", json_str.len())
                ));
            }
            Err(e) => {
                results.push(PerformanceResult::new(
                    &format!("节点提取: {}", path),
                    duration.as_millis(),
                    false,
                    &format!("提取失败: {}", e)
                ));
            }
        }
    }
    
    results
}

/// 运行综合性能测试
pub fn run_performance_suite() -> Vec<PerformanceResult> {
    let mut results = Vec::new();
    
    // 测试不同规模的数据
    let test_cases = [
        (3, 10),   // 小型：深度3，宽度10
        (4, 20),   // 中型：深度4，宽度20
        (5, 30),   // 大型：深度5，宽度30
    ];
    
    for (depth, width) in test_cases {
        println!("测试规模：深度{}，宽度{}", depth, width);
        
        // 生成测试数据
        let start = Instant::now();
        let json_data = generate_large_json(depth, width);
        let generation_time = start.elapsed();
        
        results.push(PerformanceResult::new(
            &format!("数据生成({}x{})", depth, width),
            generation_time.as_millis(),
            true,
            &format!("生成了深度{}宽度{}的JSON", depth, width)
        ));
        
        // 序列化测试
        let start = Instant::now();
        let json_str = serde_json::to_string(&json_data).unwrap();
        let serialization_time = start.elapsed();
        
        results.push(PerformanceResult::new(
            &format!("JSON序列化({}x{})", depth, width),
            serialization_time.as_millis(),
            true,
            &format!("序列化了 {} 字节", json_str.len())
        ));
        
        // 解析测试
        results.push(benchmark_json_parsing(&json_str));
        
        // 影子树构建测试
        results.push(benchmark_shadow_tree_build(&json_data));
        
        // AppState加载测试（使用内存数据）
        let start = Instant::now();
        let mut app_state = AppState::default();
        app_state.dom = Some(json_data.clone());
        app_state.tree_flat = build_shadow_tree(&json_data);
        let load_time = start.elapsed();
        
        results.push(PerformanceResult::new(
            &format!("AppState加载({}x{})", depth, width),
            load_time.as_millis(),
            true,
            &format!("加载了 {} 个节点", app_state.tree_flat.len())
        ));
        
        // 节点提取测试
        let test_paths = ["$", "$.metadata", "$.data", "$.items[0]"];
        let extraction_results = benchmark_node_extraction(&app_state, &test_paths);
        results.extend(extraction_results);
    }
    
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_large_json() {
        let json = generate_large_json(2, 3);
        assert!(json.is_object());
        
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("metadata"));
        assert!(obj.contains_key("data"));
        assert!(obj.contains_key("items"));
    }

    #[test]
    fn test_performance_benchmarks() {
        let json = generate_large_json(2, 5);
        
        // 测试影子树构建
        let tree_result = benchmark_shadow_tree_build(&json);
        assert!(tree_result.success);
        assert!(tree_result.duration_ms < 1000); // 应该在1秒内完成
        
        // 测试JSON序列化和解析
        let json_str = serde_json::to_string(&json).unwrap();
        let parse_result = benchmark_json_parsing(&json_str);
        assert!(parse_result.success);
        assert!(parse_result.duration_ms < 1000); // 应该在1秒内完成
    }
}
