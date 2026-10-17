# Optional: install VS Build Tools (VC toolchain + Win11 SDK) for the standard MSVC toolchain.
# Not required anymore (GNU toolchain works), but recommended before official packaging.
# Usage: double-click Install-VS-BuildTools.bat, then click Yes on UAC.
$ErrorActionPreference = 'Continue'
$resultFile = "$env:TEMP\qpp-vs-install-result.txt"
$boot = "$env:TEMP\vs_BuildTools.exe"
$bootLog = "$env:TEMP\qpp-vs-boot.log"

function Log { param($m) Add-Content $resultFile ("{0}  {1}" -f (Get-Date -Format 'HH:mm:ss'), $m) }
function Msg { param($t) try { Add-Type -AssemblyName System.Windows.Forms -ErrorAction Stop; [System.Windows.Forms.MessageBox]::Show($t, 'QuickPaste Installer') | Out-Null } catch {} }

Set-Content $resultFile "== QuickPaste VS Build Tools install =="
Log "started"

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent())
    .IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Log "not admin, self-elevating..."
    try {
        Start-Process powershell -Verb RunAs -ArgumentList ('-NoProfile -ExecutionPolicy Bypass -File "' + $PSCommandPath + '"') | Out-Null
        Log "elevation launched; wait for elevated instance"
    } catch {
        Log "ELEVATION FAILED: $($_.Exception.Message)"
        Msg 'Failed to start with administrator rights. Please run again and click Yes on the UAC prompt.'
    }
    exit
}

Log "running as admin"
Msg 'VS Build Tools will now install in the background (10-30 min). Keep the PC on and do not close the window. Done = a success box appears.'

try {
    if (-not (Test-Path $boot)) {
        Log "downloading bootstrapper..."
        try {
            Invoke-WebRequest 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile $boot -UseBasicParsing -TimeoutSec 600
            Log "bootstrapper ok, size=$((Get-Item $boot).Length)"
        } catch {
            Log "bootstrapper download FAILED, trying winget path: $($_.Exception.Message)"
            winget install --id Microsoft.VisualStudio.2022.BuildTools --silent --accept-package-agreements --accept-source-agreements `
                --override "--quiet --norestart --wait --nocache --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.22621"
            Log "winget exit=$LASTEXITCODE"
        }
    }
    if (Test-Path $boot) {
        Log "running installer..."
        $workloads = @(
            '--quiet','--norestart','--wait','--nocache',
            '--add','Microsoft.VisualStudio.Workload.VCTools','--includeRecommended',
            '--add','Microsoft.VisualStudio.Component.Windows11SDK.22621',
            '--log',$bootLog
        )
        $p = Start-Process -FilePath $boot -ArgumentList $workloads -Wait -PassThru
        Log "installer exit=$($p.ExitCode)"
        if ($p.ExitCode -ne 0) {
            Log "--- log tail ---"
            Get-Content $bootLog -Tail 15 | ForEach-Object { Log $_ }
        }
    }
} catch {
    Log "EXCEPTION: $($_.Exception.Message)"
}

$link = Get-ChildItem "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools" -Recurse -Filter link.exe -ErrorAction SilentlyContinue | Select-Object -First 1
if ($link) { Log "link.exe=$($link.FullName)" } else { Log "link.exe=NOT_FOUND" }
Log "DONE"
Msg 'VS Build Tools install finished. You may close this window.'