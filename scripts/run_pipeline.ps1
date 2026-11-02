# scripts/run_pipeline.ps1
# Pipeline automatizado de Portix OS para Windows (PowerShell).
# Uso: .\scripts\run_pipeline.ps1 [-Mode build|suite|recert|all] [-TimeoutSec 1800]
# Permite continuar la produccion sin el agente: build limpio + suite + recertificacion.
param(
    [ValidateSet("build","suite","recert","all")]
    [string]$Mode = "all",
    [int]$TimeoutSec = 1800
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$python = "python"
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
$logDir = Join-Path $root "build\logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

function Step($msg) { Write-Host "`n=== $msg ===" -ForegroundColor Cyan }

function Run-Build {
    Step "BUILD (limpio, sin ISO)"
    $log = Join-Path $logDir "build.log"
    $p = Start-Process -FilePath $python -ArgumentList "scripts/build.py","--no-run","--no-vm","--no-iso" -Wait -PassThru -NoNewWindow -RedirectStandardOutput $log -RedirectStandardError "$log.err"
    if ($p.ExitCode -ne 0) { Write-Host "BUILD FAIL (exit $($p.ExitCode))" -ForegroundColor Red; Get-Content $log -Tail 30; throw "build failed" }
    Write-Host "BUILD OK" -ForegroundColor Green
}

function Run-Suite {
    Step "SUITE HEADLESS"
    $log = Join-Path $logDir "headless.log"
    $p = Start-Process -FilePath $python -ArgumentList "scripts/headless_runner.py","--timeout",$TimeoutSec -Wait -PassThru -NoNewWindow -RedirectStandardOutput $log -RedirectStandardError "$log.err"
    Write-Host "suite exit=$($p.ExitCode)" -ForegroundColor Yellow
    Select-String -Path $log -Pattern "TOTAL:|PASSED:|FAILED:|MASTER-PASS|FAIL" | Select-Object -Last 20
}

function Run-Recert {
    Step "RECERTIFICACION (--no-build)"
    $log = Join-Path $logDir "recert.log"
    $p = Start-Process -FilePath $python -ArgumentList "scripts/recertify.py","--no-build" -Wait -PassThru -NoNewWindow -RedirectStandardOutput $log -RedirectStandardError "$log.err"
    Write-Host "recert exit=$($p.ExitCode)" -ForegroundColor Yellow
    Select-String -Path $log -Pattern "MASTER-PASS|FAIL|PASS" | Select-Object -Last 20
}

switch ($Mode) {
    "build"  { Run-Build }
    "suite"  { Run-Suite }
    "recert" { Run-Recert }
    "all"    { Run-Build; Run-Suite; Run-Recert }
}
Write-Host "`nPipeline '$Mode' terminado." -ForegroundColor Green