// E2E-4, ступень 2: SSR всего документа и гидрация с корнем `document` —
// ровно то, что делает Next.js 14 App Router (`hydrateRoot(document, <RootLayout/>)`).
// Дерево тут обязано совпадать с client2.js байт в байт, включая <script>-теги:
// Next.js рендерит их как часть дерева React именно поэтому.
const fs = require('fs');
const path = require('path');
const React = require('react');
const { renderToString } = require('react-dom/server');

const outDir = process.argv[2];
const e = React.createElement;

function Doc() {
  return e(
    'html',
    { lang: 'ru' },
    e(
      'head',
      null,
      e('meta', { charSet: 'utf-8' }),
      e('title', null, 'E2E-4 document hydration'),
    ),
    e(
      'body',
      null,
      e(
        'div',
        { id: 'app' },
        e('h1', null, 'document hydration probe'),
        e(React.Suspense, { fallback: e('p', null, 'loading') }, e('p', { id: 'slow' }, 'suspense payload')),
      ),
      e('script', { src: 'react.js', async: false }),
      e('script', { src: 'react-dom.js', async: false }),
      e('script', { src: 'client2.js', async: false }),
    ),
  );
}

const html = '<!DOCTYPE html>' + renderToString(e(Doc));
fs.writeFileSync(path.join(outDir, 'doc.html'), html, 'utf8');
console.log(html);
