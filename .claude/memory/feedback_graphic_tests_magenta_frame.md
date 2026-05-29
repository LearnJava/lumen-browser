---
name: graphic_tests magenta frame pattern
description: Все новые тест-страницы graphic_tests должны использовать полную 1px магента-рамку, не .__m полоску
type: feedback
originSessionId: 86fa59fd-0292-4be7-8153-cc353738d7b0
---
Все новые тест-HTML (graphic_tests/NN-*.html) должны использовать полную 1px магента-рамку вокруг всего viewport (1024×720), а НЕ старую полоску `.__m` только сверху.

**Паттерн:**
```html
<style>
  body { background: #ff00ff; width: 1024px; height: 720px; }
  .__f { background: <PAGE_BG>; width: 1022px; height: 718px; margin: 1px; padding: <PADDING>; overflow: hidden; }
</style>
<body>
  <div class="__f">
    <!-- весь контент здесь -->
  </div>
</body>
```

**Why:** `.__m` давала только верхнюю полоску — на скринах нельзя было видеть левую/правую границы для проверки корректности crop. Полная рамка показывает три стороны (top/left/right) сразу.

**How to apply:** При создании нового тест-файла или при написании нового CSS-свойства теста — всегда использовать `.__f` wrapper, не `.__m` div. Существующие файлы (01–23) уже обновлены на этот паттерн (commit magenta-frame).
