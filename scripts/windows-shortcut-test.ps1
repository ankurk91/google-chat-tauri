<#
.SYNOPSIS
Drive the built app through the Windows keyboard paths, with real keystrokes.

.DESCRIPTION
Run from the repo root, after `cargo build`:
    powershell -ExecutionPolicy Bypass -File scripts\windows-shortcut-test.ps1

Windows is the platform where a shortcut can be present in the menu, correct in
the accelerator table, and still never fire -- so none of this is assertable
from a unit test, and the menu's own labels do not prove it either. Two things
are checked, and they are separate claims:

  * the View and File menus say what they should. The menu is read back out of
    Win32 with `GetMenuStringW`, so this sees the string Windows will draw,
    including the tab-separated accelerator text -- not what muda was asked for.
  * Ctrl+W and Ctrl+Q actually do something. They are injected with
    `keybd_event` against the real desktop and the outcome is observed from
    outside the app: is the window still visible, is the process still alive.

Both keys reach the app through `features::accelerators`, which asks WebView2
for them directly, because the menu accelerator never fires while the webview
has focus. See "Windows menu accelerators are decoration" in docs/Notes.md.

**This drives the real desktop.** Keystrokes go wherever focus is, so typing
during a run lands them in your window instead and every check fails in a way
that reads exactly like a broken app -- the same trap the X11 harnesses
document. Every keystroke here is bracketed by a foreground check against the
app's own window and the run aborts loudly rather than reporting a false
failure. Leave the machine alone for the ~30 seconds it takes.

Unlike the Python harnesses there is no sandbox profile: Windows has no
XDG_* equivalent to point at a throwaway directory, so this runs against your
real profile. It changes nothing in it -- no preference is written, and both
keys under test are ones you can undo by relaunching.
#>

$ErrorActionPreference = 'Stop'

$Bin = Join-Path $PSScriptRoot '..\src-tauri\target\debug\google-chat-tauri.exe'
$Bin = [System.IO.Path]::GetFullPath($Bin)
if (-not (Test-Path $Bin)) {
    Write-Output "no binary at $Bin -- run: cargo build --manifest-path src-tauri/Cargo.toml"
    exit 1
}

Add-Type -Namespace WT -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
[DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr h);
[DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
[DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr m, int p);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetMenuStringW(IntPtr m, uint item, System.Text.StringBuilder s, int max, uint flag);
'@

$script:Failures = @()
function Check([string]$Name, [bool]$Ok, [string]$Detail = '') {
    $note = if ($Detail -and -not $Ok) { "  -- $Detail" } else { '' }
    $mark = if ($Ok) { 'PASS' } else { 'FAIL' }
    Write-Output "  $mark  $Name$note"
    if (-not $Ok) { $script:Failures += $Name }
}

function Stop-App {
    Get-Process google-chat-tauri -ErrorAction SilentlyContinue | ForEach-Object {
        try { $_.Kill() } catch { }
    }
    Start-Sleep -Milliseconds 800
}

# The app closes to the tray, so a leftover copy answers instead of ours through
# the single-instance plugin. Same reason the Python harnesses want the slot.
function Start-App {
    Stop-App
    Start-Process -FilePath $Bin | Out-Null
    # Wait for the *menu* rather than the window. `MainWindowHandle` goes
    # non-zero as soon as something is mapped, which is before `setup` has run
    # `set_menu` -- ask too early and the menu bar is genuinely not there yet,
    # which reads like the app having lost its menu.
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 500
        $p = Get-Process google-chat-tauri -ErrorAction SilentlyContinue |
             Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
        if ($p -and [WT.Win]::GetMenu($p.MainWindowHandle) -ne [IntPtr]::Zero) { return $p }
    }
    throw "app window never appeared"
}

# Focus has to be *inside* the webview, which is where it is in real use and
# also the only case where the menu accelerator is known to fail. Activating the
# window is not enough on its own, so click into the page as well.
function Focus-Page($p) {
    [void][WT.Win]::SetForegroundWindow($p.MainWindowHandle)
    Start-Sleep -Milliseconds 700
    if ([WT.Win]::GetForegroundWindow() -ne $p.MainWindowHandle) {
        throw "the app is not foreground -- something else took focus; rerun on an idle machine"
    }
    [void][WT.Win]::SetCursorPos(950, 600)
    Start-Sleep -Milliseconds 150
    [WT.Win]::mouse_event(0x02, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [WT.Win]::mouse_event(0x04, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 500
}

function Send-Ctrl([byte]$Vk, $p) {
    [WT.Win]::keybd_event(0x11, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 30
    [WT.Win]::keybd_event($Vk, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 60
    [WT.Win]::keybd_event($Vk, 0, 2, [IntPtr]::Zero); Start-Sleep -Milliseconds 30
    [WT.Win]::keybd_event(0x11, 0, 2, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 1500
    # The window may legitimately have gone by now (that is the point of both
    # keys), so only complain if some *other* window took focus while it lived.
    $fg = [WT.Win]::GetForegroundWindow()
    if ([WT.Win]::IsWindowVisible($p.MainWindowHandle) -and $fg -ne $p.MainWindowHandle) {
        throw "focus left the app mid-keystroke -- rerun on an idle machine"
    }
}

# Every menu item as "Top > Item", accelerator text and all. Windows separates
# the label from the accelerator with a tab; keep it, it is what is being
# asserted.
function Read-Menu($hwnd) {
    $items = @{}
    $bar = [WT.Win]::GetMenu($hwnd)
    if ($bar -eq [IntPtr]::Zero) { throw "the window has no menu bar" }
    $topCount = [WT.Win]::GetMenuItemCount($bar)
    for ($t = 0; $t -lt $topCount; $t++) {
        $sb = New-Object System.Text.StringBuilder 512
        [void][WT.Win]::GetMenuStringW($bar, [uint32]$t, $sb, 512, 0x400)
        $top = $sb.ToString()
        $sub = [WT.Win]::GetSubMenu($bar, $t)
        if ($sub -eq [IntPtr]::Zero) { continue }
        $n = [WT.Win]::GetMenuItemCount($sub)
        for ($i = 0; $i -lt $n; $i++) {
            $sb2 = New-Object System.Text.StringBuilder 512
            [void][WT.Win]::GetMenuStringW($sub, [uint32]$i, $sb2, 512, 0x400)
            $text = $sb2.ToString()
            if ($text -eq '') { continue }
            $items["$top > " + ($text -split "`t")[0]] = $text
        }
    }
    return $items
}

$app = $null
try {
    Write-Output "[1/3] the menu says what it should"
    $app = Start-App
    $menu = Read-Menu $app.MainWindowHandle

    Check "Zoom In carries its shortcut" `
        ($menu['View > Zoom In'] -eq "Zoom In`tCtrl+=") "got '$($menu['View > Zoom In'])'"
    Check "Zoom Out carries its shortcut" `
        ($menu['View > Zoom Out'] -eq "Zoom Out`tCtrl+-") "got '$($menu['View > Zoom Out'])'"
    Check "Close to Tray carries its shortcut" `
        ($menu['File > Close to Tray'] -eq "Close to Tray`tCtrl+W") "got '$($menu['File > Close to Tray'])'"
    Check "Quit carries its shortcut" `
        ($menu['File > Quit'] -eq "Quit`tCtrl+Q") "got '$($menu['File > Quit'])'"
    # muda draws a Fullscreen item on Windows and then does nothing when it is
    # clicked, so it is asked for on macOS only. Catch it coming back.
    Check "no Toggle Full Screen" `
        (-not ($menu.Keys | Where-Object { $_ -match 'Full Screen' })) "muda's macOS-only item is being drawn again"

    Write-Output "[2/3] Ctrl+W closes to tray"
    Focus-Page $app
    Send-Ctrl 0x57 $app
    Check "window hidden" (-not [WT.Win]::IsWindowVisible($app.MainWindowHandle))
    Check "process still running" ([bool](Get-Process -Id $app.Id -ErrorAction SilentlyContinue))

    Write-Output "[3/3] Ctrl+Q quits"
    $app = Start-App
    Focus-Page $app
    $id = $app.Id
    Send-Ctrl 0x51 $app
    Check "process gone" (-not (Get-Process -Id $id -ErrorAction SilentlyContinue))
}
finally {
    Stop-App
    Write-Output "app stopped"
}

if ($script:Failures.Count -gt 0) {
    Write-Output ""
    Write-Output "$($script:Failures.Count) FAILED: $($script:Failures -join ', ')"
    exit 1
}
Write-Output ""
Write-Output "all checks passed"
