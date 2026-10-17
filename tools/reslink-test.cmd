@echo off
rem reslink-test.cmd - regenerate libresource.a via windres-shim and relink the minimal exe.
rem Usage: reslink-test.cmd [extra env WINDRES_* vars are inherited]
setlocal
set PATH=E:\2Monkcode\toolchains\mingw-shims;%USERPROFILE%\.cargo\bin;%PATH%
set RC=E:\2Monkcode\1QuickPaste\src-tauri\target\debug\build\quickpaste-aecd7f15bdd2c7bf\out\resource.rc
set RES=E:\2Monkcode\1QuickPaste\src-tauri\target\debug\build\quickpaste-aecd7f15bdd2c7bf\out\libresource.a
windres --input %RC% --output %RES% --include-dir %RC%\.. --output-format=coff
if errorlevel 1 ( echo WINDRES_FAILED & exit /b 1 )
cd /d E:\2Monkcode\1QuickPaste\tools
rustup run stable-x86_64-pc-windows-gnu rustc -C target-feature=+crt-static --target x86_64-pc-windows-gnu -C linker=rust-lld -C link-arg=%RES% reslink-test.rs -o %TEMP%\reslink-test.exe
if errorlevel 1 ( echo LINK_FAILED & exit /b 1 )
echo LINK_OK
