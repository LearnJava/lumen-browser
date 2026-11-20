// BUG-1826 / FINGERPRINT-JS-REGISTRY: единый реестр «нативных» функций реала.
//
// До унификации действовали два механизма с разными отсечками: фасад
// `Function.prototype.toString` в конце склейки шима (обход глубины 4) и
// Set-снапшот в конце `install_dom` (BFS с вызовом геттеров окна). Покрытие
// зависело от того, где оказалась отсечка: ленивые шимы и функции,
// созданные после второго прохода, утекали исходником; в воркерах Set-маски
// вообще не было. Здесь — один реестр, одна точка обхода на реалм.
//
// Жизненный цикл:
//  * склейка ставит фасад РАНО (в конец страничной/воркерной программы),
//    но обхода не делает — к этому моменту ещё не все `install_*` отработали;
//  * после всех установок Rust дёргает `_lumen_native_sweep()` один раз
//    (page: `v8_runtime.rs` перед BUG-1811 preload и BUG-378 seal; worker:
//    конец install_*_globals_v8 каждого flavour);
//  * функции, созданные ПОСЛЕ обхода (кадровые фасады моста, ленивые шимы),
//    регистрируются хуком `_lumen_native_register(fn, kind, key)` —
//    неэнумерируемые свойства внутреннего контейнера `__lumen_C` (как у всех
//    `_lumen_*` имён после BUG-753), на globalThis — только в реалмах без
//    контейнера.
//
// Границы: `Object.prototype.toString`/`@@toStringTag` реестр не трогает —
// это отдельный чеклист probe-native-api.
(function () {
  if (typeof Function === 'undefined' || typeof Function.prototype === 'undefined') return;
  var origToString = Function.prototype.toString;
  if (typeof origToString !== 'function') return;
  // Хост хуков — внутренний контейнер (`__lumen_C`), как у всех `_lumen_*`
  // имён после BUG-753 срез 2: internal eval разрешает их через
  // `with (container)`, а страница до контейнера не достаёт — хуки невидимы
  // для `Object.getOwnPropertyNames(globalThis)` и не нуждаются в BUG-378
  // seal. В реалмах без контейнера (минимальные рантаймы) — fallback на
  // globalThis, там внутренние eval не обёрнуты и имя резолвится глобально.
  var host = globalThis;
  try {
    if (typeof __lumen_C !== 'undefined' && __lumen_C !== null &&
        typeof __lumen_C === 'object') host = __lumen_C;
  } catch (e) { /* __lumen_C недоступен — остаётся globalThis */ }
  // Гард повторной установки: в контейнере уже лежит живая функция sweep.
  if (typeof host._lumen_native_sweep === 'function') return;

  var texts = new WeakMap(); // функция → текст в формате Chrome
  var seen = new Set();
  var budget = 1000000; // жёсткий потолок на новые объекты; DOM-граф ~10^4
  var queue = [];

  var fnText = function (fn) {
    var name = '';
    try { name = fn.name || ''; } catch (e) { name = ''; }
    return 'function ' + (name ? name : '') + '() { [native code] }';
  };
  var accessorText = function (kind, key) {
    return 'function ' + kind + ' ' + key + '() { [native code] }';
  };
  var keyText = function (key) {
    return typeof key === 'symbol' ? '[' + String(key) + ']' : String(key);
  };
  var register = function (fn, text) {
    if (typeof fn === 'function' && texts.get(fn) === undefined) texts.set(fn, text);
  };

  // BFS по достижимости: data-свойства (конструкторы, прототипы, неймспейсы,
  // статики), акцессоры (геттеры/сеттеры — с текстом `get x`/`set x`),
  // прототипная цепочка каждого объекта. Геттеры вызываем один раз на
  // владельце (this = сам объект) — синглтоны вроде document/navigator
  // отдаются уже установленными, как в обоих прежних проходах; бросающие
  // и не-object результаты пропускаем. На объектах, попавших в очередь по
  // ПРОТОТИПНОЙ цепочке, геттеры не вызываем: бренд-чекнутые акцессоры
  // прототипов (`Illegal invocation` на this=prototype) — не наши данные,
  // а их вызов ещё и сыплет unhandled rejection в reporter страницы.
  var harvest = function (obj, depth, callGetters) {
    if (obj === null || (typeof obj !== 'object' && typeof obj !== 'function')) return;
    if (seen.has(obj)) return;
    if (budget-- <= 0) return;
    seen.add(obj);

    var names, syms;
    try { names = Object.getOwnPropertyNames(obj); } catch (e) { names = []; }
    try { syms = Object.getOwnPropertySymbols(obj); } catch (e) { syms = []; }
    for (var pass = 0; pass < 2; pass++) {
      var keys = pass === 0 ? names : syms;
      for (var i = 0; i < keys.length; i++) {
        var key = keys[i], d;
        try { d = Object.getOwnPropertyDescriptor(obj, key); } catch (e) { continue; }
        if (!d) continue;
        var kt = keyText(key);
        if (typeof d.get === 'function') {
          register(d.get, accessorText('get', kt));
          if (callGetters && depth < 3) {
            try {
              var gv = d.get.call(obj);
              // A probe getter may answer with a promise that rejects
              // asynchronously (brand-checked accessors such as the
              // ViewTransition `ready`/`finished`/`updateCallbackDone`
              // getters reject when `this` is not a real instance). The
              // synchronous try/catch above cannot see that rejection —
              // attach a noop handler so the sweep does not surface it as
              // the page's own unhandledrejection.
              if (gv !== null && (typeof gv === 'object' || typeof gv === 'function')) {
                try { Promise.resolve(gv).then(undefined, function () {}); } catch (e2) {}
                queue.push([gv, depth + 1, true]);
              }
            } catch (e) {}
          }
        }
        if (typeof d.set === 'function') register(d.set, accessorText('set', kt));
        var v = d.value;
        if (typeof v === 'function') {
          register(v, fnText(v));
          // Сама функция в очереди: её `.prototype` и статики — data-свойства.
          queue.push([v, depth, callGetters]);
        } else if (v !== null && typeof v === 'object') {
          queue.push([v, depth, callGetters]);
        }
      }
    }

    var proto;
    try { proto = Object.getPrototypeOf(obj); } catch (e) { proto = null; }
    if (proto !== null) queue.push([proto, depth, false]);
  };

  var sweep = function () {
    queue.push([globalThis, 0, true]);
    while (queue.length > 0) {
      var item = queue.pop();
      harvest(item[0], item[1], item[2]);
    }
  };

  var maskedToString = function toString() {
    var t = texts.get(this);
    if (t !== undefined) return t;
    return origToString.call(this);
  };
  register(maskedToString, fnText(maskedToString));
  Function.prototype.toString = maskedToString;

  var publish = function (name, value) {
    try {
      Object.defineProperty(host, name, {
        value: value,
        writable: true,
        configurable: true,
        enumerable: false,
      });
    } catch (e) { /* минимальные рантаймы без defineProperty на хосте */ }
  };

  // Хук для функций, созданных ПОСЛЕ обхода: кадровые фасады моста
  // (frame_bridge.rs) и ленивые шимы. `kind`: 'function' | 'getter' |
  // 'setter'; для акцессоров `key` — имя свойства (текст `get x`/`set x`).
  publish('_lumen_native_register', function (fn, kind, key) {
    if (typeof fn !== 'function') return;
    if (kind === 'getter' || kind === 'setter') {
      // Имя акцессора в JS уже 'get x'/'set x' — без ключа срезаем префикс.
      var kt = key === undefined
        ? String(fn.name || '').replace(/^(get|set) /, '')
        : keyText(key);
      register(fn, accessorText(kind === 'getter' ? 'get' : 'set', kt));
    } else {
      register(fn, fnText(fn));
    }
  });
  publish('_lumen_native_sweep', sweep);
})();
