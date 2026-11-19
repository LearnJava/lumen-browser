// BUG-1826 срез 3: фасад `Function.prototype.toString` для функций шима.
//
// Станичный код (Cloudflare challenge VM — в его строковой таблице есть и
// 'toString', и '[native code]') проверяет «нативность» платформенных функций:
// у Chrome `fetch.toString()` отдаёт `function fetch() { [native code] }`, а
// шимы Lumen — свой JS-исходник, то есть весь API выглядит подменённым.
//
// В конце сборки шима (страничного и воркерного — обе склейки включают этот
// кусок последним) globalThis ещё содержит только платформенную поверхность:
// обходим её и каждый шимовый ключ кладём в замыкание WeakMap с фасадом в
// формате Chrome. `Function.prototype.toString` смотрит в карту; всё, чего в
// ней нет (страничные функции, V8-нативы), идёт родным путём без изменений.
//
// Границы: функции ленивых шимов, устанавливаемых ПОСЛЕ этой склейки
// (батарея, аудио и т.п.), остаются с исходником; внутренние хелперы шимов,
// не дошедшие до globalThis, тоже. `Object.prototype.toString.call` и
// `@@toStringTag` этот кусок не трогает.
(function () {
  var origToString = Function.prototype.toString;
  var facade = new WeakMap(); // функция → фасадный текст
  var seen = new Set();

  var facadeFor = function (fn) {
    return 'function ' + (fn && fn.name ? fn.name : '') + '() { [native code] }';
  };
  var mark = function (fn, text) {
    if (typeof fn === 'function') facade.set(fn, text);
  };

  // Геттеры/сеттеры Chrome печатает как `function get x() { [native code] }`.
  var visit = function (obj, depth) {
    if (!obj || seen.has(obj)) return;
    if (typeof obj !== 'object' && typeof obj !== 'function') return;
    seen.add(obj);
    var keys;
    try {
      keys = Object.getOwnPropertyNames(obj).concat(Object.getOwnPropertySymbols(obj));
    } catch (e) { return; }
    for (var i = 0; i < keys.length; i++) {
      var key = keys[i];
      var d;
      try { d = Object.getOwnPropertyDescriptor(obj, key); } catch (e) { continue; }
      if (!d) continue;
      var keyText = typeof key === 'symbol' ? '[' + String(key) + ']' : String(key);
      if (typeof d.get === 'function') {
        mark(d.get, 'function get ' + keyText + '() { [native code] }');
        // Аксессоры-объекты (document на window, location, navigator и т.п.):
        // их методы — тоже шимовые, спускаемся в результат геттера.
        if (depth > 0) {
          try { visit(d.get.call(obj), depth - 1); } catch (e) {}
        }
      }
      if (typeof d.set === 'function') mark(d.set, 'function set ' + keyText + '() { [native code] }');
      var v = d.value;
      if (typeof v === 'function') {
        mark(v, facadeFor(v));
        if (depth > 0 && v.prototype) visit(v.prototype, depth - 1);
        // статики конструкторов (URL.createObjectURL и т.п.) — тоже шимовые
        if (depth > 0) visit(v, depth - 1);
      } else if (depth > 0 && v && (typeof v === 'object' || typeof v === 'function')) {
        // вложенные неймспейсы: crypto.subtle, performance, document и т.п.
        visit(v, depth - 1);
      }
    }
  };

  visit(globalThis, 4);
  var winProto = Object.getPrototypeOf(globalThis);
  if (winProto && winProto !== Object.prototype) visit(winProto, 3);

  Function.prototype.toString = function toString() {
    var f = facade.get(this);
    if (f !== undefined) return f;
    return origToString.call(this);
  };
  facade.set(Function.prototype.toString, 'function toString() { [native code] }');

  // BUG-1826 срез 5: функции, создаваемые ПОСЛЕ установки шима, — кадровые
  // фасады (eval и прочие глобалы чужого изолята, frame_bridge.rs) — помечают
  // себя этим хуком; без него toString отдаёт исходник кловера, и проверка
  // «нативности» у challenge-VM видит подмену. Читается лениво, поэтому
  // порядок склейки не важен.
  try {
    Object.defineProperty(globalThis, '_lumen_tostring_mark', {
      value: function (fn) { mark(fn, facadeFor(fn)); },
      writable: true,
      configurable: true,
      enumerable: false,
    });
  } catch (e) { /* минимальные рантаймы без defineProperty на globalThis */ }
})();
