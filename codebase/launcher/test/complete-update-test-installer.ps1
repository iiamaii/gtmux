$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class TestInstallerControl {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
}
'@
for ($attempt=0; $attempt -lt 120; $attempt++) {
  $process=Get-Process -Name 'gtmux-update-test-0.1.1' -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if ($process) {
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    # NSIS exposes its themed buttons as Panes, not UIA Buttons. Use the native
    # default-button ID and BM_CLICK, scoped to this test installer's window.
    $elements=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
    $currentUser=$elements | Where-Object { $_.Current.AutomationId -eq '1202' -and $_.Current.IsEnabled } | Select-Object -First 1
    if($currentUser){[void][TestInstallerControl]::PostMessage([IntPtr]$currentUser.Current.NativeWindowHandle,0x00F5,[IntPtr]::Zero,[IntPtr]::Zero)}
    $button=$elements | Where-Object { $_.Current.AutomationId -eq '1' -and $_.Current.IsEnabled -and -not $_.Current.IsOffscreen } | Select-Object -First 1
    if($button){
      Write-Output ('Test installer: '+$button.Current.Name)
      [void][TestInstallerControl]::PostMessage([IntPtr]$button.Current.NativeWindowHandle,0x00F5,[IntPtr]::Zero,[IntPtr]::Zero)
      if($process.WaitForExit(2000)){exit 0}
    }
  }
  Start-Sleep -Milliseconds 750
}
throw 'Test installer did not finish in time.'
