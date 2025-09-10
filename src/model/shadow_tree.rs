//! 影子树（Shadow Tree）：仅存结构与路径，不复制大值，助于大文件性能导航

use serde_json::Value;

/// JSON 节点类型（与 UI 展示解耦）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

#[derive(Debug, Clone)]
pub struct JsonTreeNode {
    /// 节点在父级中的键名或索引的字符串形式
    pub name: String,
    /// RFC 9535 JSONPath（用于精确寻址与回写）
    pub path: String,
    /// 节点类型
    pub kind: NodeKind,
    /// 子元素数量（对象字段数 / 数组长度），便于 UI 懒加载展示
    pub children: u32,
    /// 轻量预览（字符串截断、数字/布尔/空的简短描述）
    pub preview: String,
    /// 节点深度（用于UI缩进显示）
    pub depth: u32,
    /// 是否展开（用于折叠/展开功能）
    pub expanded: bool,
    /// 是否可见（用于搜索过滤）
    pub visible: bool,
}

/// 从根 Value 构建全树影子索引（可后续做懒加载/分页）
pub fn build_shadow_tree(root: &Value) -> Vec<JsonTreeNode> {
    let mut out = Vec::with_capacity(1024);
    fn kind_of(v: &Value) -> NodeKind {
        match v {
            Value::Object(_) => NodeKind::Object,
            Value::Array(_) => NodeKind::Array,
            Value::String(_) => NodeKind::String,
            Value::Number(_) => NodeKind::Number,
            Value::Bool(_) => NodeKind::Bool,
            Value::Null => NodeKind::Null,
        }
    }
    fn preview_of(v: &Value) -> String {
        match v {
            Value::String(s) => {
                let s = s.trim();
                if s.chars().count() > 32 {
                    let truncated: String = s.chars().take(32).collect();
                    format!("\"{}...\"", truncated)
                } else {
                    format!("\"{}\"", s)
                }
            }
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_string(),
            Value::Object(m) => format!("{{..}} ({} keys)", m.len()),
            Value::Array(a) => format!("[..] ({} items)", a.len()),
        }
    }
    fn push_node(out: &mut Vec<JsonTreeNode>, name: String, path: String, v: &Value, depth: u32) {
        let children = match v {
            Value::Object(m) => m.len() as u32,
            Value::Array(a) => a.len() as u32,
            _ => 0,
        };
        out.push(JsonTreeNode {
            name,
            path,
            kind: kind_of(v),
            children,
            preview: preview_of(v),
            depth,
            expanded: false,  // 默认折叠
            visible: true,    // 默认可见
        });
    }
    fn walk(out: &mut Vec<JsonTreeNode>, v: &Value, path: &str, name: &str, depth: u32) {
        push_node(out, name.to_string(), path.to_string(), v, depth);
        match v {
            Value::Object(map) => {
                for (k, child) in map {
                    // JSONPath 字段含特殊字符时使用 bracket-notation
                    let field_path = if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' ) {
                        format!("{}.{}", path, k)
                    } else {
                        format!("{}['{}']", path, k.replace('\'', "\\'"))
                    };
                    walk(out, child, &field_path, k, depth + 1);
                }
            }
            Value::Array(arr) => {
                for (idx, child) in arr.iter().enumerate() {
                    let item_path = format!("{}[{}]", path, idx);
                    walk(out, child, &item_path, &format!("[{}]", idx), depth + 1);
                }
            }
            _ => {}
        }
    }

    walk(&mut out, root, "$", "$", 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_object_shadow_tree() {
        let json = json!({
            "name": "测试",
            "age": 30
        });

        let tree = build_shadow_tree(&json);

        // 应该有3个节点：根、name、age
        assert_eq!(tree.len(), 3);

        // 检查根节点
        assert_eq!(tree[0].name, "$");
        assert_eq!(tree[0].path, "$");
        assert_eq!(tree[0].kind, NodeKind::Object);
        assert_eq!(tree[0].children, 2);

        // 检查字段节点（顺序可能不确定）
        let field_nodes: Vec<_> = tree.iter().skip(1).collect();
        assert_eq!(field_nodes.len(), 2);

        // 检查是否包含name和age字段
        let names: Vec<&str> = field_nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"age"));

        // 检查路径
        let paths: Vec<&str> = field_nodes.iter().map(|n| n.path.as_str()).collect();
        assert!(paths.contains(&"$.name"));
        assert!(paths.contains(&"$.age"));
    }

    #[test]
    fn test_nested_object_shadow_tree() {
        let json = json!({
            "user": {
                "profile": {
                    "name": "张三"
                }
            }
        });

        let tree = build_shadow_tree(&json);

        // 应该有4个节点：根、user、profile、name
        assert_eq!(tree.len(), 4);

        // 检查路径生成
        assert_eq!(tree[0].path, "$");
        assert_eq!(tree[1].path, "$.user");
        assert_eq!(tree[2].path, "$.user.profile");
        assert_eq!(tree[3].path, "$.user.profile.name");
    }

    #[test]
    fn test_array_shadow_tree() {
        let json = json!({
            "items": [
                "第一项",
                {"id": 1},
                [1, 2, 3]
            ]
        });

        let tree = build_shadow_tree(&json);

        // 检查数组路径生成
        let paths: Vec<&str> = tree.iter().map(|n| n.path.as_str()).collect();
        assert!(paths.contains(&"$"));
        assert!(paths.contains(&"$.items"));
        assert!(paths.contains(&"$.items[0]"));
        assert!(paths.contains(&"$.items[1]"));
        assert!(paths.contains(&"$.items[1].id"));
        assert!(paths.contains(&"$.items[2]"));
        assert!(paths.contains(&"$.items[2][0]"));
        assert!(paths.contains(&"$.items[2][1]"));
        assert!(paths.contains(&"$.items[2][2]"));
    }

    #[test]
    fn test_special_characters_in_keys() {
        let json = json!({
            "normal_key": "value1",
            "key with spaces": "value2",
            "key-with-dashes": "value3",
            "key.with.dots": "value4",
            "key'with'quotes": "value5"
        });

        let tree = build_shadow_tree(&json);

        // 检查特殊字符的路径处理
        let paths: Vec<&str> = tree.iter().map(|n| n.path.as_str()).collect();
        assert!(paths.contains(&"$.normal_key"));
        assert!(paths.contains(&"$['key with spaces']"));
        assert!(paths.contains(&"$['key-with-dashes']"));
        assert!(paths.contains(&"$['key.with.dots']"));
        assert!(paths.contains(&"$['key\\'with\\'quotes']"));
    }

    #[test]
    fn test_node_preview_generation() {
        let json = json!({
            "short_string": "短文本",
            "long_string": "这是一个非常长的字符串，应该被截断以便在预览中显示，不应该显示完整内容",
            "number": 42,
            "boolean": true,
            "null_value": null,
            "object": {"nested": "value"},
            "array": [1, 2, 3, 4, 5]
        });

        let tree = build_shadow_tree(&json);

        // 检查预览文本生成
        for node in &tree {
            match node.name.as_str() {
                "short_string" => assert_eq!(node.preview, "\"短文本\""),
                "long_string" => assert!(node.preview.contains("...")),
                "number" => assert_eq!(node.preview, "42"),
                "boolean" => assert_eq!(node.preview, "true"),
                "null_value" => assert_eq!(node.preview, "null"),
                "object" => assert_eq!(node.preview, "{..} (1 keys)"),
                "array" => assert_eq!(node.preview, "[..] (5 items)"),
                _ => {}
            }
        }
    }
}

