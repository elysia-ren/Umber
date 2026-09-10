@echo off
rem ABI spike build: Rust cdylib + MSVC C host.
rem Usage: build_spike.cmd [debug^|release]
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
cl /nologo /W4 /I "runtime-ffi\include" "runtime-ffi\examples\spike.c" /Fe:"%BINDIR%\spike.exe" /Fo:"%BINDIR%\spike.obj" /link "%BINDIR%\runtime_ffi.dll.lib" || exit /b 1

set "PATH=%BINDIR%;%PATH%"
"%BINDIR%\spike.exe"
exit /b %ERRORLEVEL%
