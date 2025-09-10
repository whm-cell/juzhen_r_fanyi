//! VM桥接层：连接Slint UI与AppState数据模型
//!
//! 注意：此模块的具体实现在main.rs中，因为依赖于Slint生成的类型
//! 这里只提供公共常量

// === 常量定义（消除魔法值） ===
pub const STATUS_READY: &str = "就绪";
pub const STATUS_LOADING: &str = "正在加载文件...";
pub const STATUS_LOADED: &str = "文件加载完成";
pub const STATUS_COPIED: &str = "已复制到剪贴板";
pub const STATUS_WRITE_BACK_SUCCESS: &str = "回写成功";
pub const STATUS_ERROR_PREFIX: &str = "错误: ";

