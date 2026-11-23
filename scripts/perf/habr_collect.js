
(() => {
  const nav = performance.getEntriesByType('navigation')[0] || {};
  const paints = performance.getEntriesByType('paint').map(p => [p.name, Math.round(p.startTime)]);
  const res = performance.getEntriesByType('resource').map(r => ({
    n: r.name, i: r.initiatorType || '', s: Math.round(r.startTime),
    d: Math.round(r.duration), rs: Math.round(r.responseEnd - r.responseStart || 0),
    t: r.transferSize || 0, enc: r.encodedBodySize || 0
  }));
  const lt = (performance.getEntriesByType('longtask')||[]).map(l=>Math.round(l.duration));
  return JSON.stringify({
    rs: document.readyState,
    nav: {
      fetchStart: Math.round(nav.fetchStart||0), domContentLoaded: Math.round(nav.domContentLoadedEventEnd||0),
      domInteractive: Math.round(nav.domInteractive||0), loadEvent: Math.round(nav.loadEventEnd||0),
      responseEnd: Math.round(nav.responseEnd||0), ttfb: Math.round((nav.responseStart-nav.fetchStart)||0),
      domComplete: Math.round(nav.domComplete||0)
    },
    paints, longtasks: lt, count: res.length, res
  });
})()
