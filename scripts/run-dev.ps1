<#
.SYNOPSIS
    Запускает цикл задач для одного разработчика Lumen.
    Каждая задача — отдельная сессия Claude Code с чистым контекстом.

.PARAMETER Developer
    Номер разработчика: P1, P2, P3 или P4.

.PARAMETER MaxTasks
    Максимальное количество задач подряд (по умолчанию — без ограничения).

.EXAMPLE
    .\run-dev.ps1 -Developer P1
    .\run-dev.ps1 -Developer P2 -MaxTasks 3
#>

param(
    [Parameter(Mandatory)]
    [ValidateSet("P1", "P2", "P3", "P4")]
    [string]$Developer,

    [int]$MaxTasks = 0
)

$ErrorActionPreference = "Stop"
$ProjectDir = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

# Если скрипт запущен из worktree — подняться до корня репо
if ($ProjectDir -match '\.claude[/\\]worktrees[/\\]') {
    $ProjectDir = ($ProjectDir -replace '\.claude[/\\]worktrees[/\\].*$', '').TrimEnd('\/')
}

$StopFile = Join-Path $ProjectDir ".stop-$Developer"
$StatusFile = Join-Path $ProjectDir "STATUS-$Developer.md"
$TaskCount = 0

function Write-Log {
    param([string]$Message)
    $ts = Get-Date -Format "HH:mm:ss"
    Write-Host "[$ts] [$Developer] $Message"
}

function Test-HasTasks {
    if (-not (Test-Path $StatusFile)) {
        Write-Log "STATUS-файл не найден: $StatusFile"
        return $false
    }
    $content = Get-Content $StatusFile -Raw
    if ($content -match 'In progress:') { return $true }
    if ($content -match 'Next:' -and $content -match '- \[') { return $true }
    return $false
}

Write-Log "Старт. Проект: $ProjectDir"
Write-Log "Стоп-файл: $StopFile"
Write-Log "Для остановки после текущей задачи: New-Item '$StopFile'"
Write-Host ""

while ($true) {
    # Проверка стоп-файла
    if (Test-Path $StopFile) {
        Write-Log "Найден стоп-файл. Останавливаюсь."
        Remove-Item $StopFile -Force
        break
    }

    # Проверка лимита задач
    if ($MaxTasks -gt 0 -and $TaskCount -ge $MaxTasks) {
        Write-Log "Достигнут лимит задач ($MaxTasks). Останавливаюсь."
        break
    }

    # Проверка наличия задач
    if (-not (Test-HasTasks)) {
        Write-Log "Нет задач в $StatusFile. Останавливаюсь."
        break
    }

    $TaskCount++
    Write-Log "=== Задача #$TaskCount ==="

    $prompt = @"
Ты разработчик $Developer.
Прочитай STATUS-$Developer.md.
Если есть "In progress" — продолжи эту задачу.
Если нет — возьми первую задачу из "Next".
Когда задача завершена — вызови /lumen-task-finish.
"@

    try {
        claude -p $prompt --dangerously-skip-permissions --cwd $ProjectDir
        $exitCode = $LASTEXITCODE
    }
    catch {
        Write-Log "Ошибка запуска claude: $_"
        break
    }

    if ($exitCode -ne 0) {
        Write-Log "Claude завершился с кодом $exitCode."
        Write-Log "Пауза 10 секунд перед повтором (возможно временная ошибка)..."
        Start-Sleep -Seconds 10
    }
    else {
        Write-Log "Задача #$TaskCount завершена."
    }

    Write-Host ""
}

Write-Log "Цикл завершён. Выполнено задач: $TaskCount."
