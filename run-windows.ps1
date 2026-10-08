param(
    [ValidateRange(1, 65535)][int]$Port = 19670,
    [string]$Listen = '0.0.0.0',
    [string]$PublicUrl = '',
    [switch]$Build,
    [switch]$Test,
    [switch]$Development,
    [switch]$Stop
)
$ErrorActionPreference = 'Stop'
$taskProject = $PSScriptRoot
$taskRuntime = Join-Path $taskProject 'runtime'
$taskBinary = Join-Path $taskProject 'target\release\fanqie-relay.exe'
$taskPidFile = Join-Path $taskRuntime 'windows.pid'
if (Test-Path -LiteralPath $taskPidFile) {
    $taskOldPid = [int](Get-Content -LiteralPath $taskPidFile -Raw)
    $taskOldProcess = Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue
    if ($taskOldProcess) {
        if ($taskOldProcess.Path -ne [IO.Path]::GetFullPath($taskBinary)) {
            throw 'The saved PID belongs to another program; refusing to stop it.'
        }
        if ($Stop) {
            Stop-Process -Id $taskOldPid
            Remove-Item -LiteralPath $taskPidFile
            Write-Output 'Windows relay stopped.'
            return
        }
        Write-Output "Windows relay is already running (PID $taskOldPid)."
        Write-Output (Get-Content -LiteralPath (Join-Path $taskRuntime 'windows.json') -Raw)
        return
    }
}
if ($Stop) { Write-Output 'Windows relay is not running.'; return }
if ($Build -or $Test -or -not (Test-Path -LiteralPath $taskBinary)) {
    $taskVsWhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
    $taskVs = & $taskVsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $taskVs) { throw 'Install Visual Studio C++ Build Tools first.' }
    & (Join-Path $taskVs 'Common7\Tools\Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    Push-Location $taskProject
    try {
        & cargo fmt
        if ($LASTEXITCODE -ne 0) { throw 'Rust formatting failed.' }
        if ($Test) { & cargo test; if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' } }
        & cargo build --release
        if ($LASTEXITCODE -ne 0) { throw 'Windows release build failed.' }
    } finally { Pop-Location }
}
if (-not $PublicUrl) {
    $taskAddress = Get-NetIPConfiguration | Where-Object { $_.IPv4DefaultGateway } |
        ForEach-Object { $_.IPv4Address.IPAddress } | Where-Object {
            $_ -match '^(192\.168\.|10\.|172\.(1[6-9]|2[0-9]|3[01])\.)'
        } | Select-Object -First 1
    if (-not $taskAddress) { $taskAddress = '127.0.0.1' }
    $PublicUrl = "http://${taskAddress}:$Port"
}
$env:FANQIE_RELAY_LISTEN = "${Listen}:$Port"
$env:FANQIE_RELAY_PUBLIC_URL = $PublicUrl
if ($Development) { $env:FANQIE_RELAY_WEB_DIR = Join-Path $taskProject 'web' }
else { Remove-Item Env:\FANQIE_RELAY_WEB_DIR -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Path $taskRuntime -Force | Out-Null
$taskProcess = Start-Process -FilePath $taskBinary -WorkingDirectory $taskProject -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput (Join-Path $taskRuntime 'windows.log') `
    -RedirectStandardError (Join-Path $taskRuntime 'windows-error.log')
Set-Content -LiteralPath $taskPidFile -Value $taskProcess.Id
$taskReady = $false
for ($taskAttempt = 0; $taskAttempt -lt 30; $taskAttempt++) {
    try {
        $taskHealth = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/health" -TimeoutSec 1
        if ($taskHealth.status -eq 'ok') { $taskReady = $true; break }
    } catch { Start-Sleep -Milliseconds 200 }
}
if (-not $taskReady) { throw 'Windows relay did not start; inspect runtime/windows-error.log.' }
$taskInfo = @{ pid = $taskProcess.Id; console = "http://127.0.0.1:$Port/"; phone = $PublicUrl; version = $taskHealth.version }
$taskInfo | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskRuntime 'windows.json') -Encoding utf8
Write-Output ($taskInfo | ConvertTo-Json)
