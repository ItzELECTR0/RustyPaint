fn main() {
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("../../res/icon.ico")
        .set("ProductName", "RustyPaint")
        .set("OriginalFilename", "rustypaint.exe")
        .compile()
        .expect("Windows resources should compile");
}
