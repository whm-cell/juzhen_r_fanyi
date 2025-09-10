//! JSON翻译工具库
//! 
//! 提供JSON文件加载、影子树构建、节点提取和回写功能
//! 遵循MVVM架构模式，支持大文件高性能处理

pub mod model;
pub mod utils;
pub mod vm;

// 重新导出主要类型
pub use model::data_core::{AppState, AppError};
pub use model::shadow_tree::{JsonTreeNode, NodeKind, build_shadow_tree};
