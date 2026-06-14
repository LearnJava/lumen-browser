//! ECMA-402 `Intl` shim for `en-US` and `ru-RU` (§91 i18n).
//!
//! QuickJS (as built by `rquickjs` with default features) ships **without** the
//! ECMA-402 internationalization API, so pages that call `Intl.NumberFormat`,
//! `Intl.DateTimeFormat`, `Intl.Collator` or `Intl.PluralRules` throw
//! `ReferenceError: Intl is not defined`. This module installs a self-contained
//! pure-JS implementation covering the two locales Lumen targets first:
//! `en-US` (default fallback) and `ru-RU`.
//!
//! Scope is deliberately narrow — it is **not** a full CLDR implementation:
//!
//! * [`Intl.NumberFormat`] — `decimal` / `currency` / `percent` styles,
//!   grouping, min/max fraction digits, the common currency symbols
//!   (`USD`/`EUR`/`RUB`/`GBP`/`JPY`). `en-US` groups with `,` and a `.` decimal;
//!   `ru-RU` groups with a non-breaking space and a `,` decimal.
//! * [`Intl.DateTimeFormat`] — `year`/`month`/`day`/`weekday`/`hour`/`minute`/
//!   `second` components with locale month and weekday names; default short date
//!   (`M/D/YYYY` for `en-US`, `DD.MM.YYYY` for `ru-RU`). `hour12` defaults to
//!   `true` for `en-US`, `false` for `ru-RU`.
//! * [`Intl.Collator`] — locale-aware `compare`, placing Cyrillic `ё` after `е`
//!   for `ru-RU` and offering case-insensitive (`sensitivity: 'base'`) and
//!   numeric (`numeric: true`) collation.
//! * [`Intl.PluralRules`] — CLDR cardinal/ordinal categories for both locales
//!   (`ru-RU` resolves `one`/`few`/`many`/`other`).
//!
//! As a convenience the shim also routes `Number.prototype.toLocaleString` and
//! `Date.prototype.toLocaleString` / `toLocaleDateString` / `toLocaleTimeString`
//! through the matching `Intl` constructor so existing code paths localize too.
//!
//! Installed last in [`crate::QuickJsRuntime::install_dom`] (after the DOM and
//! `window` exist) so `window.Intl` is exposed alongside the global. If a native
//! `Intl` is ever present (e.g. a future feature-enabled QuickJS build) the shim
//! defers to it.

use rquickjs::Ctx;

/// Install the `Intl` shim into the JS context.
///
/// No-op when a native `Intl` global already exists. Must run after the DOM is
/// installed so `window` is available for re-export. Pure JS — no native
/// bindings required.
pub fn install_intl_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(INTL_SHIM)?;
    Ok(())
}

/// Pure-JS ECMA-402 shim (`en-US` + `ru-RU`). See module docs for scope.
const INTL_SHIM: &str = r#"(function(global) {
  // Defer to a native Intl if the host build provides one.
  if (typeof global.Intl !== 'undefined' && global.Intl && global.Intl.NumberFormat) {
    return;
  }

  // ── Locale resolution ───────────────────────────────────────────────────────
  // Only two locales are modelled; everything that is not Russian collapses to
  // en-US (the spec-mandated "look up" fallback for this minimal data set).
  function resolveLocale(locales) {
    var list = [];
    if (typeof locales === 'string') list = [locales];
    else if (Array.isArray(locales)) list = locales;
    for (var i = 0; i < list.length; i++) {
      var l = String(list[i] || '').toLowerCase();
      if (l === 'ru' || l.indexOf('ru-') === 0) return 'ru-RU';
      if (l === 'en' || l.indexOf('en-') === 0) return 'en-US';
    }
    return 'en-US';
  }

  var SEP = {
    'en-US': { group: ',', decimal: '.' },
    'ru-RU': { group: ' ', decimal: ',' }
  };

  var CURRENCY = {
    USD: { symbol: '$', frac: 2 }, EUR: { symbol: '€', frac: 2 },
    RUB: { symbol: '₽', frac: 2 }, GBP: { symbol: '£', frac: 2 },
    JPY: { symbol: '¥', frac: 0 }, KRW: { symbol: '₩', frac: 0 },
    CNY: { symbol: '¥', frac: 2 }
  };

  // ── Number formatting ───────────────────────────────────────────────────────
  function groupInteger(intStr, sep) {
    if (sep.group === '') return intStr;
    var out = '';
    var n = intStr.length;
    for (var i = 0; i < n; i++) {
      if (i > 0 && (n - i) % 3 === 0) out += sep.group;
      out += intStr.charAt(i);
    }
    return out;
  }

  function NumberFormat(locales, options) {
    if (!(this instanceof NumberFormat)) return new NumberFormat(locales, options);
    options = options || {};
    this._locale = resolveLocale(locales);
    this._style = options.style || 'decimal';
    this._currency = options.currency ? String(options.currency).toUpperCase() : undefined;
    this._currencyDisplay = options.currencyDisplay || 'symbol';
    this._useGrouping = options.useGrouping !== false;
    var defMax = this._style === 'currency'
      ? (CURRENCY[this._currency] ? CURRENCY[this._currency].frac : 2)
      : (this._style === 'percent' ? 0 : 3);
    var defMin = this._style === 'currency'
      ? (CURRENCY[this._currency] ? CURRENCY[this._currency].frac : 2)
      : 0;
    this._minFrac = options.minimumFractionDigits != null
      ? (options.minimumFractionDigits | 0) : defMin;
    this._maxFrac = options.maximumFractionDigits != null
      ? (options.maximumFractionDigits | 0) : Math.max(defMax, this._minFrac);
    if (this._maxFrac < this._minFrac) this._maxFrac = this._minFrac;
  }
  NumberFormat.prototype.format = function(value) {
    var num = Number(value);
    if (!isFinite(num)) return (num !== num) ? 'NaN' : (num > 0 ? '∞' : '-∞');
    var sep = SEP[this._locale];
    var negative = num < 0 || (num === 0 && 1 / num < 0);
    var v = Math.abs(num);
    if (this._style === 'percent') v = v * 100;

    var fixed = v.toFixed(this._maxFrac);
    var dot = fixed.indexOf('.');
    var intPart = dot < 0 ? fixed : fixed.slice(0, dot);
    var fracPart = dot < 0 ? '' : fixed.slice(dot + 1);
    // Trim to between min and max fraction digits.
    while (fracPart.length > this._minFrac && fracPart.charAt(fracPart.length - 1) === '0') {
      fracPart = fracPart.slice(0, -1);
    }
    var grouped = this._useGrouping ? groupInteger(intPart, sep) : intPart;
    var body = grouped + (fracPart.length ? sep.decimal + fracPart : '');
    if (negative) body = '-' + body;

    if (this._style === 'currency') {
      var info = CURRENCY[this._currency];
      var sym = (this._currencyDisplay === 'code' || !info)
        ? (this._currency || '') : info.symbol;
      if (this._locale === 'ru-RU') return body + ' ' + sym;
      // en-US: sign stays outside the symbol ("-$5.00").
      return negative ? '-' + sym + body.slice(1) : sym + body;
    }
    if (this._style === 'percent') {
      return this._locale === 'ru-RU' ? body + ' %' : body + '%';
    }
    return body;
  };
  NumberFormat.prototype.formatToParts = function(value) {
    return [{ type: 'literal', value: this.format(value) }];
  };
  NumberFormat.prototype.resolvedOptions = function() {
    return {
      locale: this._locale, numberingSystem: 'latn', style: this._style,
      currency: this._currency, useGrouping: this._useGrouping,
      minimumFractionDigits: this._minFrac, maximumFractionDigits: this._maxFrac
    };
  };

  // ── Date/time formatting ────────────────────────────────────────────────────
  var MONTHS = {
    'en-US': {
      long: ['January','February','March','April','May','June','July','August',
             'September','October','November','December'],
      short: ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'],
      gen: null
    },
    'ru-RU': {
      // Nominative (standalone month) vs genitive (used with a day number).
      long: ['январь','февраль',
             'март','апрель','май',
             'июнь','июль','август',
             'сентябрь','октябрь',
             'ноябрь','декабрь'],
      short: ['янв.','февр.','март',
              'апр.','май','июнь',
              'июль','авг.','сент.',
              'окт.','нояб.','дек.'],
      gen: ['января','февраля',
            'марта','апреля','мая',
            'июня','июля','августа',
            'сентября','октября',
            'ноября','декабря']
    }
  };
  var WEEKDAYS = {
    'en-US': {
      long: ['Sunday','Monday','Tuesday','Wednesday','Thursday','Friday','Saturday'],
      short: ['Sun','Mon','Tue','Wed','Thu','Fri','Sat']
    },
    'ru-RU': {
      long: ['воскресенье',
             'понедельник',
             'вторник','среда',
             'четверг','пятница',
             'суббота'],
      short: ['вс','пн','вт','ср','чт',
              'пт','сб']
    }
  };

  function pad2(n) { return n < 10 ? '0' + n : '' + n; }

  function DateTimeFormat(locales, options) {
    if (!(this instanceof DateTimeFormat)) return new DateTimeFormat(locales, options);
    this._locale = resolveLocale(locales);
    this._opt = options || null;
  }
  DateTimeFormat.prototype.format = function(date) {
    var d = (date == null) ? new Date() : (date instanceof Date ? date : new Date(date));
    var loc = this._locale;
    var o = this._opt;
    if (!o) {
      // Default short date.
      if (loc === 'ru-RU') return pad2(d.getDate()) + '.' + pad2(d.getMonth() + 1) + '.' + d.getFullYear();
      return (d.getMonth() + 1) + '/' + d.getDate() + '/' + d.getFullYear();
    }
    var dateParts = [];
    if (o.weekday) {
      dateParts.push(WEEKDAYS[loc][o.weekday === 'long' ? 'long' : 'short'][d.getDay()]);
    }
    var ymd = [];
    var hasDay = !!o.day;
    if (o.month) {
      var m = d.getMonth();
      if (o.month === 'numeric') ymd.push({ k: 'm', v: '' + (m + 1) });
      else if (o.month === '2-digit') ymd.push({ k: 'm', v: pad2(m + 1) });
      else {
        var names = MONTHS[loc];
        var arr = (loc === 'ru-RU' && hasDay && names.gen)
          ? names.gen : names[o.month === 'short' ? 'short' : 'long'];
        ymd.push({ k: 'm', v: arr[m] });
      }
    }
    if (o.day) ymd.push({ k: 'd', v: o.day === '2-digit' ? pad2(d.getDate()) : '' + d.getDate() });
    if (o.year) ymd.push({ k: 'y', v: o.year === '2-digit' ? pad2(d.getFullYear() % 100) : '' + d.getFullYear() });

    var dateStr = '';
    if (ymd.length) {
      if (loc === 'ru-RU') {
        // Russian order: day month year ("2 июня 2026 г.").
        var order = { d: 0, m: 1, y: 2 };
        ymd.sort(function(a, b) { return order[a.k] - order[b.k]; });
        var parts = ymd.map(function(p) { return p.v; });
        dateStr = parts.join(' ');
        if (o.year && o.month && typeof o.month !== 'undefined' && o.month !== 'numeric' && o.month !== '2-digit') {
          dateStr += ' г.';
        }
      } else {
        // en-US order: month day, year.
        var byk = {};
        ymd.forEach(function(p) { byk[p.k] = p.v; });
        var seg = [];
        if (byk.m != null) seg.push(byk.m);
        if (byk.d != null) seg.push(byk.d);
        var md = seg.join(o.month === 'long' || o.month === 'short' ? ' ' : '/');
        dateStr = md;
        if (byk.y != null) dateStr += (o.month === 'long' || o.month === 'short' ? ', ' : '/') + byk.y;
      }
    }
    if (dateParts.length) {
      dateStr = dateParts.join(', ') + (dateStr ? (loc === 'ru-RU' ? ', ' : ', ') + dateStr : '');
    }

    // Time component.
    var timeStr = '';
    if (o.hour || o.minute || o.second) {
      var h24 = d.getHours();
      var hour12 = (o.hour12 != null) ? o.hour12 : (loc === 'en-US');
      var hourNum = h24, suffix = '';
      if (hour12) {
        suffix = h24 < 12 ? ' AM' : ' PM';
        hourNum = h24 % 12; if (hourNum === 0) hourNum = 12;
      }
      var tparts = [];
      if (o.hour) tparts.push(o.hour === '2-digit' ? pad2(hourNum) : '' + hourNum);
      if (o.minute) tparts.push(o.minute === '2-digit' ? pad2(d.getMinutes()) : '' + d.getMinutes());
      if (o.second) tparts.push(o.second === '2-digit' ? pad2(d.getSeconds()) : '' + d.getSeconds());
      timeStr = tparts.join(':') + (hour12 ? suffix : '');
    }

    if (dateStr && timeStr) return dateStr + (loc === 'ru-RU' ? ', ' : ', ') + timeStr;
    return dateStr || timeStr;
  };
  DateTimeFormat.prototype.formatToParts = function(date) {
    return [{ type: 'literal', value: this.format(date) }];
  };
  DateTimeFormat.prototype.resolvedOptions = function() {
    var r = { locale: this._locale, calendar: 'gregory', numberingSystem: 'latn',
              timeZone: 'UTC' };
    if (this._opt) for (var k in this._opt) r[k] = this._opt[k];
    return r;
  };

  // ── Collation ───────────────────────────────────────────────────────────────
  function Collator(locales, options) {
    if (!(this instanceof Collator)) return new Collator(locales, options);
    options = options || {};
    this._locale = resolveLocale(locales);
    this._sensitivity = options.sensitivity || 'variant';
    this._numeric = !!options.numeric;
    this._caseFirst = options.caseFirst || 'false';
  }
  // Map a Russian char to a sortable weight so that 'ё' collates right after 'е'.
  function ruWeight(ch) {
    var c = ch.charCodeAt(0);
    if (c === 0x0451) return 0x0435 + 0.5;       // ё after е (lowercase)
    if (c === 0x0401) return 0x0415 + 0.5;       // Ё after Е (uppercase)
    return c;
  }
  Collator.prototype.compare = function(a, b) {
    a = String(a); b = String(b);
    if (this._sensitivity === 'base' || this._sensitivity === 'accent') {
      a = a.toLowerCase(); b = b.toLowerCase();
    }
    if (this._numeric) {
      var re = /(\d+|\D+)/g;
      var ta = a.match(re) || [], tb = b.match(re) || [];
      var n = Math.min(ta.length, tb.length);
      for (var i = 0; i < n; i++) {
        var pa = ta[i], pb = tb[i];
        if (/^\d/.test(pa) && /^\d/.test(pb)) {
          var na = parseInt(pa, 10), nb = parseInt(pb, 10);
          if (na !== nb) return na < nb ? -1 : 1;
        } else if (pa !== pb) {
          return cmpWeighted(pa, pb);
        }
      }
      return ta.length - tb.length;
    }
    return cmpWeighted(a, b);
  };
  function cmpWeighted(a, b) {
    var n = Math.min(a.length, b.length);
    for (var i = 0; i < n; i++) {
      var wa = ruWeight(a.charAt(i)), wb = ruWeight(b.charAt(i));
      if (wa !== wb) return wa < wb ? -1 : 1;
    }
    return a.length === b.length ? 0 : (a.length < b.length ? -1 : 1);
  }
  Collator.prototype.resolvedOptions = function() {
    return { locale: this._locale, sensitivity: this._sensitivity,
             numeric: this._numeric, caseFirst: this._caseFirst,
             collation: 'default', usage: 'sort' };
  };

  // ── Plural rules ────────────────────────────────────────────────────────────
  function PluralRules(locales, options) {
    if (!(this instanceof PluralRules)) return new PluralRules(locales, options);
    options = options || {};
    this._locale = resolveLocale(locales);
    this._type = options.type || 'cardinal';
  }
  PluralRules.prototype.select = function(value) {
    var num = Number(value);
    var s = Math.abs(num);
    var str = '' + s;
    var dot = str.indexOf('.');
    var v = dot < 0 ? 0 : str.length - dot - 1;   // visible fraction digit count
    var i = Math.floor(s);                          // integer part
    if (this._locale === 'ru-RU') {
      if (this._type === 'ordinal') return 'other';
      if (v !== 0) return 'other';
      var mod10 = i % 10, mod100 = i % 100;
      if (mod10 === 1 && mod100 !== 11) return 'one';
      if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return 'few';
      return 'many';
    }
    // en-US
    if (this._type === 'ordinal') {
      var m10 = i % 10, m100 = i % 100;
      if (m10 === 1 && m100 !== 11) return 'one';
      if (m10 === 2 && m100 !== 12) return 'two';
      if (m10 === 3 && m100 !== 13) return 'few';
      return 'other';
    }
    return (i === 1 && v === 0) ? 'one' : 'other';
  };
  PluralRules.prototype.resolvedOptions = function() {
    return { locale: this._locale, type: this._type, pluralCategories: ['other'] };
  };

  function getCanonicalLocales(locales) {
    var list = (typeof locales === 'string') ? [locales]
      : (Array.isArray(locales) ? locales : []);
    return list.map(function(l) { return String(l); });
  }

  var Intl = {
    NumberFormat: NumberFormat,
    DateTimeFormat: DateTimeFormat,
    Collator: Collator,
    PluralRules: PluralRules,
    getCanonicalLocales: getCanonicalLocales
  };
  // ECMA-402 marks these as supporting locale negotiation.
  [NumberFormat, DateTimeFormat, Collator, PluralRules].forEach(function(C) {
    C.supportedLocalesOf = function(locales) {
      var list = (typeof locales === 'string') ? [locales]
        : (Array.isArray(locales) ? locales : []);
      return list.filter(function(l) {
        var s = String(l).toLowerCase();
        return s === 'en' || s === 'ru' || s.indexOf('en-') === 0 || s.indexOf('ru-') === 0;
      });
    };
  });

  global.Intl = Intl;
  if (typeof window !== 'undefined') window.Intl = Intl;

  // ── Localized prototype methods that delegate to Intl ───────────────────────
  try {
    Number.prototype.toLocaleString = function(locales, options) {
      return new NumberFormat(locales, options).format(this);
    };
    Date.prototype.toLocaleString = function(locales, options) {
      return new DateTimeFormat(locales, options || {
        year: 'numeric', month: 'numeric', day: 'numeric',
        hour: 'numeric', minute: 'numeric', second: 'numeric'
      }).format(this);
    };
    Date.prototype.toLocaleDateString = function(locales, options) {
      return new DateTimeFormat(locales, options).format(this);
    };
    Date.prototype.toLocaleTimeString = function(locales, options) {
      return new DateTimeFormat(locales, options || {
        hour: 'numeric', minute: 'numeric', second: 'numeric'
      }).format(this);
    };
  } catch (_) {}
})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

#[cfg(test)]
mod tests {
    use crate::QuickJsRuntime;
    use lumen_core::{JsRuntime, JsValue};
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    fn runtime() -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "", None, None, None, None, None, None, None).unwrap();
        rt
    }

    fn s(rt: &QuickJsRuntime, code: &str) -> String {
        match rt.eval(code).unwrap() {
            JsValue::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn intl_is_defined() {
        let rt = runtime();
        assert_eq!(
            rt.eval("typeof Intl").unwrap(),
            JsValue::String("object".into())
        );
        assert_eq!(
            rt.eval("typeof Intl.NumberFormat").unwrap(),
            JsValue::String("function".into())
        );
        assert_eq!(
            rt.eval("typeof window.Intl").unwrap(),
            JsValue::String("object".into())
        );
    }

    #[test]
    fn number_grouping_en_us() {
        let rt = runtime();
        assert_eq!(
            s(&rt, "new Intl.NumberFormat('en-US').format(1234567.89)"),
            "1,234,567.89"
        );
    }

    #[test]
    fn number_grouping_ru_ru() {
        let rt = runtime();
        // ru-RU uses a non-breaking space group separator and a comma decimal.
        assert_eq!(
            s(&rt, "new Intl.NumberFormat('ru-RU').format(1234567.89)"),
            "1\u{00A0}234\u{00A0}567,89"
        );
    }

    #[test]
    fn currency_en_and_ru() {
        let rt = runtime();
        assert_eq!(
            s(
                &rt,
                "new Intl.NumberFormat('en-US',{style:'currency',currency:'USD'}).format(1234.5)"
            ),
            "$1,234.50"
        );
        assert_eq!(
            s(
                &rt,
                "new Intl.NumberFormat('ru-RU',{style:'currency',currency:'RUB'}).format(1234.5)"
            ),
            "1\u{00A0}234,50\u{00A0}\u{20BD}"
        );
    }

    #[test]
    fn currency_negative_en_keeps_symbol_after_sign() {
        let rt = runtime();
        assert_eq!(
            s(
                &rt,
                "new Intl.NumberFormat('en-US',{style:'currency',currency:'USD'}).format(-5)"
            ),
            "-$5.00"
        );
    }

    #[test]
    fn percent_style() {
        let rt = runtime();
        assert_eq!(
            s(&rt, "new Intl.NumberFormat('en-US',{style:'percent'}).format(0.25)"),
            "25%"
        );
        assert_eq!(
            s(&rt, "new Intl.NumberFormat('ru-RU',{style:'percent'}).format(0.25)"),
            "25\u{00A0}%"
        );
    }

    #[test]
    fn fraction_digit_limits() {
        let rt = runtime();
        assert_eq!(
            s(
                &rt,
                "new Intl.NumberFormat('en-US',{minimumFractionDigits:2,maximumFractionDigits:2}).format(3)"
            ),
            "3.00"
        );
        assert_eq!(
            s(
                &rt,
                "new Intl.NumberFormat('en-US',{maximumFractionDigits:0}).format(2.7)"
            ),
            "3"
        );
    }

    #[test]
    fn datetime_default_short_date() {
        let rt = runtime();
        // 2026-06-02 (month is 0-based in the Date constructor).
        assert_eq!(
            s(&rt, "new Intl.DateTimeFormat('en-US').format(new Date(2026,5,2))"),
            "6/2/2026"
        );
        assert_eq!(
            s(&rt, "new Intl.DateTimeFormat('ru-RU').format(new Date(2026,5,2))"),
            "02.06.2026"
        );
    }

    #[test]
    fn datetime_long_month_ru_genitive() {
        let rt = runtime();
        // With a day present ru-RU uses the genitive month form and a "г." suffix.
        assert_eq!(
            s(
                &rt,
                "new Intl.DateTimeFormat('ru-RU',{year:'numeric',month:'long',day:'numeric'}).format(new Date(2026,5,2))"
            ),
            "2 \u{0438}\u{044E}\u{043D}\u{044F} 2026 \u{0433}."
        );
    }

    #[test]
    fn datetime_long_month_en() {
        let rt = runtime();
        assert_eq!(
            s(
                &rt,
                "new Intl.DateTimeFormat('en-US',{year:'numeric',month:'long',day:'numeric'}).format(new Date(2026,5,2))"
            ),
            "June 2, 2026"
        );
    }

    #[test]
    fn datetime_time_hour12_vs_24() {
        let rt = runtime();
        // en-US defaults to 12-hour with AM/PM.
        assert_eq!(
            s(
                &rt,
                "new Intl.DateTimeFormat('en-US',{hour:'numeric',minute:'2-digit'}).format(new Date(2026,5,2,14,5))"
            ),
            "2:05 PM"
        );
        // ru-RU defaults to 24-hour.
        assert_eq!(
            s(
                &rt,
                "new Intl.DateTimeFormat('ru-RU',{hour:'2-digit',minute:'2-digit'}).format(new Date(2026,5,2,14,5))"
            ),
            "14:05"
        );
    }

    #[test]
    fn collator_orders_cyrillic_yo_after_ye() {
        let rt = runtime();
        // ё (U+0451) must sort after е (U+0435) and before ж (U+0436).
        assert_eq!(
            rt.eval(
                "new Intl.Collator('ru-RU').compare('е','ё') < 0"
            )
            .unwrap(),
            JsValue::Bool(true)
        );
        assert_eq!(
            rt.eval(
                "new Intl.Collator('ru-RU').compare('ё','ж') < 0"
            )
            .unwrap(),
            JsValue::Bool(true)
        );
    }

    #[test]
    fn collator_numeric() {
        let rt = runtime();
        assert_eq!(
            rt.eval(
                "new Intl.Collator('en-US',{numeric:true}).compare('item2','item10') < 0"
            )
            .unwrap(),
            JsValue::Bool(true)
        );
    }

    #[test]
    fn collator_base_sensitivity_ignores_case() {
        let rt = runtime();
        assert_eq!(
            rt.eval(
                "new Intl.Collator('en-US',{sensitivity:'base'}).compare('a','A')"
            )
            .unwrap(),
            JsValue::Number(0.0)
        );
    }

    #[test]
    fn plural_rules_ru_cardinal() {
        let rt = runtime();
        let pr = "var pr = new Intl.PluralRules('ru-RU');";
        rt.eval(pr).unwrap();
        assert_eq!(s(&rt, "pr.select(1)"), "one");
        assert_eq!(s(&rt, "pr.select(2)"), "few");
        assert_eq!(s(&rt, "pr.select(5)"), "many");
        assert_eq!(s(&rt, "pr.select(11)"), "many");
        assert_eq!(s(&rt, "pr.select(21)"), "one");
        assert_eq!(s(&rt, "pr.select(22)"), "few");
        assert_eq!(s(&rt, "pr.select(1.5)"), "other");
    }

    #[test]
    fn plural_rules_en_cardinal_and_ordinal() {
        let rt = runtime();
        assert_eq!(s(&rt, "new Intl.PluralRules('en-US').select(1)"), "one");
        assert_eq!(s(&rt, "new Intl.PluralRules('en-US').select(2)"), "other");
        assert_eq!(
            s(&rt, "new Intl.PluralRules('en-US',{type:'ordinal'}).select(1)"),
            "one"
        );
        assert_eq!(
            s(&rt, "new Intl.PluralRules('en-US',{type:'ordinal'}).select(2)"),
            "two"
        );
        assert_eq!(
            s(&rt, "new Intl.PluralRules('en-US',{type:'ordinal'}).select(3)"),
            "few"
        );
        assert_eq!(
            s(&rt, "new Intl.PluralRules('en-US',{type:'ordinal'}).select(4)"),
            "other"
        );
        assert_eq!(
            s(&rt, "new Intl.PluralRules('en-US',{type:'ordinal'}).select(11)"),
            "other"
        );
    }

    #[test]
    fn number_to_locale_string_delegates() {
        let rt = runtime();
        assert_eq!(s(&rt, "(1234.5).toLocaleString('en-US')"), "1,234.5");
        assert_eq!(
            s(&rt, "(1234.5).toLocaleString('ru-RU')"),
            "1\u{00A0}234,5"
        );
    }

    #[test]
    fn resolved_options_locale_fallback() {
        let rt = runtime();
        // Unknown locale falls back to en-US.
        assert_eq!(
            s(&rt, "new Intl.NumberFormat('xx-YY').resolvedOptions().locale"),
            "en-US"
        );
        assert_eq!(
            s(&rt, "new Intl.NumberFormat(['de','ru-RU']).resolvedOptions().locale"),
            "ru-RU"
        );
    }

    #[test]
    fn supported_locales_of() {
        let rt = runtime();
        assert_eq!(
            rt.eval(
                "Intl.NumberFormat.supportedLocalesOf(['en-US','fr-FR','ru-RU']).length"
            )
            .unwrap(),
            JsValue::Number(2.0)
        );
    }
}
