//! Clipboard  cross-platform clipboard helpers

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClipboardError {
    #[error("clipboard error: {0}")]
    Clip(String),
}

/// 将文本复制到系统剪贴板
pub fn copy_to_clipboard(text: &str) -> Result<(), ClipboardError> {
    use copypasta::{ClipboardContext, ClipboardProvider};
    let mut ctx = ClipboardContext::new().map_err(|e| ClipboardError::Clip(e.to_string()))?;
    ctx.set_contents(text.to_string())
        .map_err(|e| ClipboardError::Clip(e.to_string()))
}

/// 从系统剪贴板获取文本（用于测试）
#[cfg(test)]
pub fn get_clipboard_contents() -> Result<String, ClipboardError> {
    use copypasta::{ClipboardContext, ClipboardProvider};
    let mut ctx = ClipboardContext::new().map_err(|e| ClipboardError::Clip(e.to_string()))?;
    ctx.get_contents()
        .map_err(|e| ClipboardError::Clip(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_copy_and_get() {
        let test_text = "测试剪贴板功能";

        // 复制到剪贴板
        let copy_result = copy_to_clipboard(test_text);
        assert!(copy_result.is_ok(), "复制到剪贴板应该成功");

        // 从剪贴板读取
        let get_result = get_clipboard_contents();
        assert!(get_result.is_ok(), "从剪贴板读取应该成功");

        let clipboard_content = get_result.unwrap();
        assert_eq!(clipboard_content, test_text, "剪贴板内容应该与复制的文本一致");
    }

    #[test]
    fn test_clipboard_empty_string() {
        let empty_text = "";

        let result = copy_to_clipboard(empty_text);
        assert!(result.is_ok(), "复制空字符串应该成功");

        let clipboard_content = get_clipboard_contents().unwrap();
        assert_eq!(clipboard_content, empty_text, "剪贴板应该包含空字符串");
    }

    #[test]
    fn test_clipboard_unicode() {
        let unicode_text = "🚀 JSON翻译工具 🎯 测试Unicode字符 ✨";

        let result = copy_to_clipboard(unicode_text);
        assert!(result.is_ok(), "复制Unicode文本应该成功");

        let clipboard_content = get_clipboard_contents().unwrap();
        assert_eq!(clipboard_content, unicode_text, "剪贴板应该正确处理Unicode字符");
    }
}

