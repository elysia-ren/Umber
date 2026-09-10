# 启动 demo 窗口，截图，退出。用于人工核对界面观感。
# 用法: powershell -ExecutionPolicy Bypass -File screenshot.ps1 [dark|light] [width] [height]
param([string]$Theme = "dark", [int]$Width = 960, [int]$Height = 740)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
}
[StructLayout(LayoutKind.Sequential)]
public struct RECT { public int Left, Top, Right, Bottom; }
"@

$exe = Join-Path $PSScriptRoot "..\..\target\release\settings-demo.exe"
if (-not (Test-Path $exe)) {
    Write-Error "build first: cargo build -p umber-ui-egui --bin settings-demo --release"
    exit 1
}
$exe = (Resolve-Path $exe).Path
$out = Join-Path $PSScriptRoot "wizard-$Theme.png"

$proc = Start-Process -FilePath $exe -PassThru
Start-Sleep -Seconds 4

$hwnd = $proc.MainWindowHandle
[Win]::ShowWindow($hwnd, 9) | Out-Null
[Win]::MoveWindow($hwnd, 60, 40, $Width, $Height, $true) | Out-Null
[Win]::SetForegroundWindow($hwnd) | Out-Null
# 鼠标移到屏幕角落，避免 hover tooltip 挡住画面
[Win]::SetCursorPos(5, 5) | Out-Null
Start-Sleep -Milliseconds 1200

$r = New-Object RECT
[Win]::GetWindowRect($hwnd, [ref]$r) | Out-Null
$w = $r.Right - $r.Left
$h = $r.Bottom - $r.Top

$bmp = New-Object System.Drawing.Bitmap $w, $h
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size $w, $h))
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$gfx.Dispose(); $bmp.Dispose()

Stop-Process -Id $proc.Id -Force
Write-Output "saved: $out ($w x $h)"
