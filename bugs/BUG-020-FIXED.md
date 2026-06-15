# BUG-020

**Статус:** FIXED 2026-05-26
**Компонент:** layout
**Файл:** (нет)

## Описание

overflow axis coercion: visible+hidden combo не клипало ось; CSS Overflow L3 §2.1 visible→auto в compute_style; TEST-14: 1.70%→0.03% PASS

## Детали

overflow: scroll/auto/hidden не реализован

TEST-14: все варианты overflow ведут себя как `visible`. В Edge видны scrollbar-ы и клиппинг.

Fix: overflow axis coercion (FIXED 2026-05-26); overflow visible+hidden combo не клипало ось; CSS Overflow L3 §2.1 visible→auto в compute_style; TEST-14: 1.70%→0.03% PASS.
