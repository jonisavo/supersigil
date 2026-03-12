@echo off
setlocal

for %%I in ("%~dp0..\..") do set "REPO_ROOT=%%~fI"
set "BINARY_PATH=%REPO_ROOT%\target\release\supersigil.exe"

cargo build --manifest-path "%REPO_ROOT%\Cargo.toml" -p supersigil-cli --release -q || exit /b %errorlevel%

"%BINARY_PATH%" %*
