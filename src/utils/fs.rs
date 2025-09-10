//! IO helper: safe file read/write for JSON

use std::{fs::File, io::BufReader, path::Path};

use serde_json::Value;
use crate::model::data_core::AppError;

/// 从文件读取JSON数据
pub fn read_json_file(p: &Path) -> Result<Value, AppError> {
    let f = File::open(p)?;
    let rdr = BufReader::new(f);
    let v: Value = serde_json::from_reader(rdr)?;
    Ok(v)
}

/// 将JSON数据保存到文件（格式化输出）
pub fn write_json_file(p: &Path, value: &Value) -> Result<(), AppError> {
    let f = File::create(p)?;
    serde_json::to_writer_pretty(f, value)?;
    Ok(())
}