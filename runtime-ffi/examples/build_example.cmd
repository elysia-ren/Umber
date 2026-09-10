@echo off
rem C host example build: Rust cdylib + MSVC C host.
rem Usage: build_example.cmd [debug^|release]
setlocal enabledelayedexpansion
set "CFG=%~1"
if "%CFG%"=="" set "CFG=debug"
set "CARGO_FLAG=%CFG%"
if /i "%CFG%"=="debug" set "CARGO_FLAG="

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
set "ROOT=%~dp0"
cd /d "%ROOT%..\.."

cargo build -p runtime-ffi %CARGO_FLAG% || exit /b 1

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath > "%TEMP%\umer_vspath.txt" 2>nul
set /p VSPATH=<"%TEMP%\umer_vspath.txt"
if not exist "!VSPATH!\VC\Auxiliary\Build\vcvars64.bat" (
    echo vcvars64.bat not found under [!VSPATH!]
    exit /b 1
)

set "BINDIR=%ROOT%bin"
if not exist "%BINDIR%" mkdir "%BINDIR%"
copy /y "target\%CFG%\runtime_ffi.dll" "%BINDIR%\" >nul || exit /b 1
copy /y "target\%CFG%\runtime_ffi.dll.lib" "%BINDIR%\" >nul || exit /b 1

call "%VSPATH%\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
rem /utf-8: umer.h 与示例的注释是中文，缺这个开关在 GBK 代码页下会报 C4819
cl /nologo /W4 /utf-8 /I "runtime-ffi\include" "runtime-ffi\examples\host_example.c" /Fe:"%BINDIR%\host_example.exe" /Fo:"%BINDIR%\host_example.obj" /link "%BINDIR%\runtime_ffi.dll.lib" || exit /b 1

set "PATH=%BINDIR%;%PATH%"
"%BINDIR%\host_example.exe"
exit /b %ERRORLEVEL%
