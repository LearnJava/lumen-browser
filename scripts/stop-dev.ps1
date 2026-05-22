<#
.SYNOPSIS
    Создаёт стоп-файл для разработчика. Текущая задача доработает,
    новая не запустится.

.PARAMETER Developer
    Номер разработчика: P1, P2, P3 или P4.

.PARAMETER All
    Остановить всех разработчиков.

.EXAMPLE
    .\stop-dev.ps1 -Developer P1
    .\stop-dev.ps1 -All
#>

param(
    [ValidateSet("P1", "P2", "P3", "P4")]
    [string]$Developer,

    [switch]$All
)

$ProjectDir = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if ($ProjectDir -match '\.claude[/\\]worktrees[/\\]') {
    $ProjectDir = ($ProjectDir -replace '\.claude[/\\]worktrees[/\\].*$', '').TrimEnd('\/')
}

if (-not $All -and -not $Developer) {
    Write-Host "Укажите -Developer P1 или -All"
    exit 1
}

$targets = if ($All) { @("P1", "P2", "P3", "P4") } else { @($Developer) }

foreach ($dev in $targets) {
    $stopFile = Join-Path $ProjectDir ".stop-$dev"
    New-Item -Path $stopFile -ItemType File -Force | Out-Null
    Write-Host "$dev будет остановлен после текущей задачи. ($stopFile)"
}
