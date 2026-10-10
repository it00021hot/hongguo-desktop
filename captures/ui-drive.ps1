# hgplayer UI 驱动器（用户授权本会话代操作）：点击/滚轮/截图
# 用法: powershell -File ui-drive.ps1 <cmd> [args]
#   click <fx> <fy>   —— 按窗口比例点击（0-1）
#   wheel <fx> <fy> <n> —— 滚轮 n 格（负=向上）
#   shot <out.png>    —— 截窗口图
#   rect              —— 打印窗口矩形
param([string]$cmd, [double]$fx = 0, [double]$fy = 0, [int]$n = 3, [string]$out = "hg-ui.png")

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int ht, uint f);
  public struct RECT { public int L, T, R, B; }
}
"@

$proc = Get-Process 红果短剧 -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $proc) { Write-Error "窗口未找到"; exit 1 }
$h = $proc.MainWindowHandle
[W]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 300
$r = New-Object W+RECT
[W]::GetWindowRect($h, [ref]$r) | Out-Null
$wd = $r.R - $r.L; $ht = $r.B - $r.T

switch ($cmd) {
  "rect" { Write-Output "L=$($r.L) T=$($r.T) W=$wd H=$ht" }
  "front" {
    # HWND_TOPMOST(-1) + 挪到左上角：不被其他窗口遮挡，也不盖用户主工作区中心
    [W]::SetWindowPos($h, [IntPtr](-1), 0, 0, 1480, 760, 0x0040) | Out-Null
    [W]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 500
    Write-Output "fronted"
  }
  "click" {
    $x = [int]($r.L + $wd * $fx); $y = [int]($r.T + $ht * $fy)
    [W]::SetCursorPos($x, $y) | Out-Null; Start-Sleep -Milliseconds 120
    [W]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [W]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Write-Output "click @ $x,$y"
  }
  "wheel" {
    $x = [int]($r.L + $wd * $fx); $y = [int]($r.T + $ht * $fy)
    [W]::SetCursorPos($x, $y) | Out-Null; Start-Sleep -Milliseconds 150
    for ($i = 0; $i -lt [Math]::Abs($n); $i++) {
      $d = if ($n -gt 0) { [uint32]::MaxValue - 1199999 } else { 1200000 }
      [W]::mouse_event(0x0800, 0, 0, $d, [UIntPtr]::Zero)
      Start-Sleep -Milliseconds 200
    }
    Write-Output "wheel $n @ $x,$y"
  }
  "shot" {
    Add-Type -AssemblyName System.Drawing
    $b = New-Object System.Drawing.Bitmap($wd, $ht)
    $g = [System.Drawing.Graphics]::FromImage($b)
    $g.CopyFromScreen($r.L, $r.T, 0, 0, $b.Size)
    $b.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Output "saved $out ($wd x $ht)"
  }
}
