<#
.SYNOPSIS
    Запускает все 4 разработчика параллельно, каждый в своём окне PowerShell.

.PARAMETER Developers
    Список разработчиков (по умолчанию все: P1 P2 P3 P4).

.PARAMETER MaxTasks
    Лимит задач на каждого разработчика (0 = без ограничения).

.EXAMPLE
    .\run-all.ps1
    .\run-all.ps1 -Developers P1,P2
    .\run-all.ps1 -MaxTasks 5
#>

param(
    [string[]]$Developers = @("P1", "P2", "P3", "P4"),
    [int]$MaxTasks = 0
)

$ScriptDir = $PSScriptRoot
$RunDev = Join-Path $ScriptDir "run-dev.ps1"

foreach ($dev in $Developers) {
    $maxArg = if ($MaxTasks -gt 0) { "-MaxTasks $MaxTasks" } else { "" }
    $title = "Lumen $dev"

    Write-Host "Запускаю $dev в отдельном окне..."
    Start-Process powershell -ArgumentList `
        "-NoExit", `
        "-Command", `
        "`$Host.UI.RawUI.WindowTitle = '$title'; & '$RunDev' -Developer $dev $maxArg" `
        -WorkingDirectory $ScriptDir
}

Write-Host ""
Write-Host "Все разработчики запущены."
Write-Host "Для остановки используйте:  .\stop-dev.ps1 -Developer P1"
Write-Host "Или остановить всех:        .\stop-dev.ps1 -All"
