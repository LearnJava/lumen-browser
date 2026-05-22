#!/usr/bin/env python3
"""
Оркестратор задач Lumen.

Автоматический запуск сессий Claude Code для разработчиков P1-P4.
Каждая задача — отдельная сессия с чистым контекстом.

Использование:
    python scripts/orchestrator.py P1              # один разработчик
    python scripts/orchestrator.py P1 P2           # два в параллель
    python scripts/orchestrator.py P1 P2 P3 P4    # все четверо
    python scripts/orchestrator.py P1 --max-tasks 3
    python scripts/orchestrator.py --stop P1       # мягкая остановка
    python scripts/orchestrator.py --stop-all      # остановить всех
    python scripts/orchestrator.py --status        # статус всех
"""

import argparse
import os
import subprocess
import sys
import re
import time
from datetime import datetime
from pathlib import Path

# Корень проекта — два уровня вверх от scripts/
PROJECT_DIR = Path(__file__).resolve().parent.parent


def log(developer: str, message: str):
    ts = datetime.now().strftime("%H:%M:%S")
    print(f"[{ts}] [{developer}] {message}", flush=True)


def has_tasks(developer: str) -> bool:
    """Проверить, есть ли задачи в STATUS-файле."""
    status_file = PROJECT_DIR / f"STATUS-{developer}.md"
    if not status_file.exists():
        log(developer, f"STATUS-файл не найден: {status_file}")
        return False

    content = status_file.read_text(encoding="utf-8")
    if re.search(r"In progress:", content):
        return True
    if re.search(r"Next:", content) and re.search(r"- \[", content):
        return True
    return False


def stop_file_path(developer: str) -> Path:
    return PROJECT_DIR / f".stop-{developer}"


def jobstatus_path(developer: str) -> Path:
    return PROJECT_DIR / f".jobstatus-{developer}"


def set_jobstatus(developer: str, status: str, detail: str = ""):
    """Записать текущий статус разработчика в файл."""
    path = jobstatus_path(developer)
    ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    lines = [f"status: {status}", f"updated: {ts}"]
    if detail:
        lines.append(f"detail: {detail}")
    path.write_text("\n".join(lines), encoding="utf-8")


def show_status():
    """Показать статус всех разработчиков."""
    print("Статус разработчиков:")
    print("-" * 50)
    for dev in ["P1", "P2", "P3", "P4"]:
        path = jobstatus_path(dev)
        if not path.exists():
            print(f"  {dev}: не запущен")
        else:
            content = path.read_text(encoding="utf-8")
            status = ""
            updated = ""
            detail = ""
            for line in content.splitlines():
                if line.startswith("status:"):
                    status = line.split(":", 1)[1].strip()
                elif line.startswith("updated:"):
                    updated = line.split(":", 1)[1].strip()
                elif line.startswith("detail:"):
                    detail = line.split(":", 1)[1].strip()
            info = f"{status}"
            if detail:
                info += f" | {detail}"
            if updated:
                info += f" (обн. {updated})"
            print(f"  {dev}: {info}")
    print("-" * 50)


def run_task_loop(developer: str, max_tasks: int = 0):
    """Цикл задач для одного разработчика."""
    stop_file = stop_file_path(developer)
    task_count = 0

    log(developer, f"Старт. Проект: {PROJECT_DIR}")
    log(developer, f"Стоп-файл: {stop_file}")
    log(developer, "")
    set_jobstatus(developer, "запущен")

    while True:
        # Проверка стоп-файла
        if stop_file.exists():
            log(developer, "Найден стоп-файл. Останавливаюсь.")
            stop_file.unlink()
            break

        # Проверка лимита
        if max_tasks > 0 and task_count >= max_tasks:
            log(developer, f"Достигнут лимит задач ({max_tasks}). Останавливаюсь.")
            break

        # Проверка наличия задач
        if not has_tasks(developer):
            log(developer, "Нет задач. Останавливаюсь.")
            break

        task_count += 1
        log(developer, f"=== Задача #{task_count} ===")
        set_jobstatus(developer, "работает", f"задача #{task_count}")

        prompt = (
            f"Ты разработчик {developer}. "
            f"Прочитай STATUS-{developer}.md. "
            f"Если есть 'In progress' — продолжи эту задачу. "
            f"Если нет — возьми первую задачу из 'Next'. "
            f"Когда задача завершена — вызови /lumen-task-finish."
        )

        try:
            result = subprocess.run(
                ["claude", "-p", prompt, "--dangerously-skip-permissions"],
                cwd=PROJECT_DIR,
            )
            exit_code = result.returncode
        except FileNotFoundError:
            log(developer, "claude не найден в PATH.")
            break
        except Exception as e:
            log(developer, f"Ошибка запуска: {e}")
            break

        if exit_code != 0:
            log(developer, f"Claude завершился с кодом {exit_code}.")
            log(developer, "Пауза 10 секунд перед повтором...")
            time.sleep(10)
        else:
            log(developer, f"Задача #{task_count} завершена.")

    set_jobstatus(developer, "остановлен", f"выполнено задач: {task_count}")
    log(developer, f"Цикл завершён. Выполнено задач: {task_count}.")


def create_stop_file(developers: list[str]):
    """Создать стоп-файлы."""
    for dev in developers:
        sf = stop_file_path(dev)
        sf.touch()
        print(f"{dev} будет остановлен после текущей задачи. ({sf})")


def main():
    parser = argparse.ArgumentParser(
        description="Оркестратор задач Lumen — автозапуск сессий Claude Code."
    )
    parser.add_argument(
        "developers",
        nargs="*",
        choices=["P1", "P2", "P3", "P4"],
        metavar="DEV",
        help="Разработчики для запуска: P1 P2 P3 P4",
    )
    parser.add_argument(
        "--max-tasks",
        type=int,
        default=0,
        help="Максимум задач на разработчика (0 = без ограничения)",
    )
    parser.add_argument(
        "--stop",
        nargs="+",
        choices=["P1", "P2", "P3", "P4"],
        metavar="DEV",
        help="Остановить разработчика после текущей задачи",
    )
    parser.add_argument(
        "--stop-all",
        action="store_true",
        help="Остановить всех разработчиков",
    )
    parser.add_argument(
        "--status",
        action="store_true",
        help="Показать статус всех разработчиков",
    )

    args = parser.parse_args()

    # Режим статуса
    if args.status:
        show_status()
        return

    # Режим остановки
    if args.stop_all:
        create_stop_file(["P1", "P2", "P3", "P4"])
        return

    if args.stop:
        create_stop_file(args.stop)
        return

    # Режим запуска
    if not args.developers:
        parser.print_help()
        sys.exit(1)

    developers = args.developers

    if len(developers) == 1:
        # Один разработчик — в текущем окне
        run_task_loop(developers[0], args.max_tasks)
    else:
        # Несколько — каждый в отдельном окне консоли
        script = Path(__file__).resolve()
        max_arg = f" --max-tasks {args.max_tasks}" if args.max_tasks > 0 else ""

        for dev in developers:
            cmd = f'python "{script}" {dev}{max_arg}'
            title = f"Lumen {dev}"

            if os.name == "nt":
                # Windows: start открывает новое окно с заголовком
                subprocess.Popen(
                    f'start "{title}" cmd /k {cmd}',
                    shell=True,
                    cwd=PROJECT_DIR,
                )
            else:
                # Linux/macOS fallback
                subprocess.Popen(
                    ["bash", "-c", f"{cmd}; exec bash"],
                    cwd=PROJECT_DIR,
                )

            print(f"Запущен {dev} в отдельном окне.")

        print()
        print(f"Для остановки: python scripts/orchestrator.py --stop P1")
        print(f"Остановить всех: python scripts/orchestrator.py --stop-all")


if __name__ == "__main__":
    main()
