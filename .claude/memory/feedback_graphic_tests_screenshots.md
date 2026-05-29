---
name: Graphic tests screenshot files
description: Какие файлы скриншотов сохранять при графических тестах
type: feedback
originSessionId: b55dc49a-6667-44a6-898e-8f4ae04989be
---
При работе с графическими тестами файлы `*lumen.png` (полный скриншот окна) НЕ сохранять. Сохранять только `*lumen-cropped.png` (обрезанный до клиентской области).

**Why:** полный скриншот содержит title bar и рамку ОС, они не нужны; cropped — финальный артефакт для сравнения.

**How to apply:** в скриптах pipeline и при ручной работе — коммитить/показывать только `*lumen-cropped.png`, удалять промежуточный `*lumen.png`.
