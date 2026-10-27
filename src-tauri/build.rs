fn main() {
    tauri_build::build();
    // 注：版本资源（FileVersion/ProductName）不再在 build.rs 注入。
    // 本机工具链为 stable-x86_64-pc-windows-gnu 且无可用资源编译器
    // （.cargo\bin\windres.exe 是 shim，无 rc.exe/llvm-rc/GNU windres），
    // 改用构建后脚本 scripts/qp_fix_version.ps1 直接向 exe 写 RT_VERSION 资源。
    // 见 docs/技术陷阱记录.md。
}