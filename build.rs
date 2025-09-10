fn main() {
    // 编译 Slint UI 文件
    slint_build::compile("src/app.slint").expect("compile slint ui");
}

