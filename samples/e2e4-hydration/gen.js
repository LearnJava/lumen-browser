// E2E-4: генератор SSR-страницы формы React 18, повторяющей разметку Next.js 14
// (маркеры Suspense `<!--$-->` / `<!--/$-->`), для локальной пробы гидрации в Lumen.
// Запускается из каталога frontend внешнего стенда (там лежит react 18.3.1),
// пишет результат в переданный первым аргументом каталог внутри репозитория Lumen.
const fs = require('fs');
const path = require('path');
const React = require('react');
const { renderToString } = require('react-dom/server');

const outDir = process.argv[2];
const e = React.createElement;

function Slow() {
  return e('p', { id: 'slow' }, 'suspense payload');
}

function App() {
  const [n, setN] = React.useState(0);
  // Парный `onClick` к client.js: обработчики не сериализуются, разметка от
  // него не меняется — держим оба дерева одинаковыми, чтобы правка одного
  // файла не выглядела рассинхроном с другим.
  return e(
    'div',
    { id: 'app', onClick: () => {} },
    e('h1', null, 'hydration probe'),
    e(React.Suspense, { fallback: e('p', null, 'loading') }, e(Slow)),
    e('button', { id: 'btn', onClick: () => setN(n + 1) }, 'clicked ' + n),
  );
}

const html = renderToString(e(App));
fs.writeFileSync(path.join(outDir, 'ssr-body.html'), html, 'utf8');

const nm = path.join(process.cwd(), 'node_modules');
for (const [src, dst] of [
  [path.join(nm, 'react/umd/react.production.min.js'), 'react.js'],
  [path.join(nm, 'react-dom/umd/react-dom.production.min.js'), 'react-dom.js'],
]) {
  fs.copyFileSync(src, path.join(outDir, dst));
}
console.log(html);
