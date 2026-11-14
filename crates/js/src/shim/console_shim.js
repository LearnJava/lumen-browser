// Console Standard `console` factory (BUG-1782): one `_lumen_make_console(emit)`
// per realm — the page shim and every worker flavour build their console from
// this text and differ only in the sink. `emit(level, text)` receives
// 'log' | 'warn' | 'error' plus one ready line and maps both onto whatever the
// scope has (the page's three `_lumen_console_*` natives, a worker's single
// prefixed native, or nothing at all in a service worker). Counters, timers and
// the group depth live in this closure, so each realm owns its state.
//
// The methods a site can tell apart from Chrome through `typeof` are functions;
// `assert` logs on a falsy condition, `time*`/`count*` keep their maps, groups
// indent. `dir`/`dirxml`/`table` log like `log` — the structured printers are a
// DevTools surface, not a page-observable one.
var _lumen_make_console = function(emit) {
  var counters = {};
  var timers = {};
  var depth = 0;
  function pad() {
    var s = '';
    for (var i = 0; i < depth; i++) s += '  ';
    return s;
  }
  function line(args) {
    return Array.prototype.map.call(args, String).join(' ');
  }
  function label(x) {
    return x === undefined ? 'default' : String(x);
  }
  return {
    log:    function() { emit('log', pad() + line(arguments)); },
    info:   function() { emit('log', pad() + line(arguments)); },
    debug:  function() { emit('log', pad() + line(arguments)); },
    warn:   function() { emit('warn', pad() + line(arguments)); },
    error:  function() { emit('error', pad() + line(arguments)); },
    dir:    function() { emit('log', pad() + line(arguments)); },
    dirxml: function() { emit('log', pad() + line(arguments)); },
    table:  function() { emit('log', pad() + line(arguments)); },
    trace:  function() {
      var stack = '';
      try { stack = new Error().stack || ''; } catch (e) {}
      emit('error', pad() + 'Trace: ' + line(arguments) + (stack ? '\n' + stack : ''));
    },
    assert: function(condition) {
      if (condition) return;
      var rest = Array.prototype.slice.call(arguments, 1);
      emit('error', 'Assertion failed' + (rest.length ? ': ' + line(rest) : ''));
    },
    clear: function() {},
    count: function(x) {
      var k = label(x);
      counters[k] = (counters[k] || 0) + 1;
      emit('log', k + ': ' + counters[k]);
    },
    countReset: function(x) {
      var k = label(x);
      if (counters[k] === undefined) {
        emit('warn', "Count for '" + k + "' does not exist");
        return;
      }
      counters[k] = 0;
    },
    group: function() { emit('log', pad() + line(arguments)); depth++; },
    groupCollapsed: function() { emit('log', pad() + line(arguments)); depth++; },
    groupEnd: function() { if (depth > 0) depth--; },
    time: function(x) { timers[label(x)] = Date.now(); },
    timeLog: function(x) {
      var k = label(x);
      if (timers[k] === undefined) {
        emit('warn', "Timer '" + k + "' does not exist");
        return;
      }
      emit('log', k + ': ' + (Date.now() - timers[k]) + ' ms');
    },
    timeEnd: function(x) {
      var k = label(x);
      if (timers[k] === undefined) {
        emit('warn', "Timer '" + k + "' does not exist");
        return;
      }
      emit('log', k + ': ' + (Date.now() - timers[k]) + ' ms');
      delete timers[k];
    },
  };
};
undefined;
