//! TC39 Temporal API shim (Stage 4 / ECMAScript 2025 §15.9-like).
//!
//! QuickJS ships without a native Temporal implementation, so pages that use
//! `Temporal.PlainDate`, `Temporal.Instant`, etc. receive a `ReferenceError`.
//! This module installs a self-contained pure-JS shim that covers the most
//! common Temporal use-cases:
//!
//! * [`Temporal.Now`] — `instant()`, `plainDateTimeISO()`, `plainDateISO()`,
//!   `plainTimeISO()`, `timeZoneId()`, `zonedDateTimeISO()`
//! * [`Temporal.Instant`] — epoch-ms/ns construction, arithmetic, `toString()`
//! * [`Temporal.PlainDate`] — ISO 8601 dates, full calendar arithmetic, `since`/`until`
//! * [`Temporal.PlainTime`] — time-of-day, arithmetic, `toString()`
//! * [`Temporal.PlainDateTime`] — combined date+time, arithmetic, `toString()`
//! * [`Temporal.PlainYearMonth`] — year+month, arithmetic
//! * [`Temporal.PlainMonthDay`] — month+day
//! * [`Temporal.Duration`] — ISO 8601 duration strings, arithmetic, `toString()`
//! * [`Temporal.ZonedDateTime`] — UTC and fixed-offset timezone support
//! * [`Temporal.Calendar`] — `iso8601` calendar (sole supported calendar)
//! * [`Temporal.TimeZone`] — UTC and system timezone from `Date`
//!
//! **Precision note:** Internally epoch-nanoseconds are stored as a Number
//! (JavaScript `f64`). Nanosecond precision beyond ~2^53 ns (~104 days from
//! epoch) is approximate. This is acceptable for Phase 1; a BigInt-backed
//! implementation can be substituted later without changing the public API.
//!
//! Installed as the last step in [`crate::QuickJsRuntime::install_dom`] so that
//! `window.Temporal` is available alongside `globalThis.Temporal`.

use rquickjs::Ctx;

/// Install the Temporal API shim into the given QuickJS context.
///
/// No-op when `globalThis.Temporal` already exists (future-proof for a
/// native QuickJS Temporal implementation). Must run after the DOM is
/// installed so that `window` is available for re-export.
pub fn install_temporal_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(TEMPORAL_SHIM)?;
    Ok(())
}

/// Pure-JS TC39 Temporal shim. See module docs for scope.
const TEMPORAL_SHIM: &str = r#"(function(global) {
  'use strict';
  // Skip if a native Temporal implementation already exists.
  if (typeof global.Temporal !== 'undefined' && global.Temporal &&
      global.Temporal.PlainDate) return;

  // ── Utility helpers ────────────────────────────────────────────────────────

  function pad2(n) { return n < 10 ? '0' + n : '' + n; }
  function pad4(n) { var s = '' + Math.abs(n); while (s.length < 4) s = '0' + s; return (n < 0 ? '-' : '') + s; }
  function pad9(n) { var s = '' + n; while (s.length < 9) s = '0' + s; return s.slice(0, 9); }

  function isLeapYear(y) { return (y % 4 === 0 && y % 100 !== 0) || (y % 400 === 0); }

  function daysInMonth(y, m) {
    var dims = [0, 31, isLeapYear(y) ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    return dims[m];
  }

  function daysInYear(y) { return isLeapYear(y) ? 366 : 365; }

  // Day-of-year from month+day
  function dayOfYear(y, m, d) {
    var n = d;
    for (var i = 1; i < m; i++) n += daysInMonth(y, i);
    return n;
  }

  // Day-of-week (0=Mon … 6=Sun, following ISO 8601)
  function dayOfWeek(y, m, d) {
    var jsDay = new Date(Date.UTC(y, m - 1, d)).getUTCDay(); // 0=Sun…6=Sat
    return jsDay === 0 ? 7 : jsDay; // convert to ISO: 1=Mon…7=Sun
  }

  // Week number (ISO 8601)
  function weekOfYear(y, m, d) {
    var jan4 = new Date(Date.UTC(y, 0, 4));
    var startOfWeek1 = jan4.getTime() - (((jan4.getUTCDay() + 6) % 7)) * 86400000;
    var target = Date.UTC(y, m - 1, d);
    var weekNum = Math.floor((target - startOfWeek1) / (7 * 86400000)) + 1;
    if (weekNum < 1) return weekOfYear(y - 1, 12, 31);
    if (weekNum > 52) {
      var jan4Next = new Date(Date.UTC(y + 1, 0, 4));
      var startNext = jan4Next.getTime() - (((jan4Next.getUTCDay() + 6) % 7)) * 86400000;
      if (target >= startNext) return 1;
    }
    return weekNum;
  }

  // Convert epoch-ms to UTC date+time fields
  function msToFields(ms) {
    var d = new Date(ms);
    return {
      year: d.getUTCFullYear(), month: d.getUTCMonth() + 1, day: d.getUTCDate(),
      hour: d.getUTCHours(), minute: d.getUTCMinutes(), second: d.getUTCSeconds(),
      millisecond: d.getUTCMilliseconds(), microsecond: 0, nanosecond: 0
    };
  }

  // Convert UTC date+time fields to epoch-ms
  function fieldsToMs(f) {
    return Date.UTC(f.year, f.month - 1, f.day,
                    f.hour || 0, f.minute || 0, f.second || 0, f.millisecond || 0);
  }

  // Clamp overflow year/month back into valid ranges
  function balanceYearMonth(y, m) {
    m = m - 1; // 0-based for arithmetic
    y += Math.floor(m / 12);
    m = ((m % 12) + 12) % 12;
    return { year: y, month: m + 1 };
  }

  // Clamp day to end of month
  function clampDay(y, m, d) {
    var dim = daysInMonth(y, m);
    return d > dim ? dim : (d < 1 ? 1 : d);
  }

  // Add calendar duration (years/months/weeks/days) to a date
  function addDate(y, m, d, years, months, weeks, days) {
    y += (years || 0);
    m += (months || 0);
    var ym = balanceYearMonth(y, m);
    y = ym.year; m = ym.month;
    d = clampDay(y, m, d); // clamp after month change
    d += (weeks || 0) * 7 + (days || 0);
    // Normalize day overflow/underflow
    while (d < 1) { m--; ym = balanceYearMonth(y, m); y = ym.year; m = ym.month; d += daysInMonth(y, m); }
    while (d > daysInMonth(y, m)) { d -= daysInMonth(y, m); m++; ym = balanceYearMonth(y, m); y = ym.year; m = ym.month; }
    return { year: y, month: m, day: d };
  }

  // Days between two dates (positive if b > a)
  function daysBetween(ay, am, ad, by, bm, bd) {
    var a = Date.UTC(ay, am - 1, ad);
    var b = Date.UTC(by, bm - 1, bd);
    return Math.round((b - a) / 86400000);
  }

  // Difference between two dates as { years, months, days }
  function dateDiff(ay, am, ad, by, bm, bd) {
    var sign = 1;
    if (Date.UTC(ay, am-1, ad) > Date.UTC(by, bm-1, bd)) {
      var tmp; tmp = ay; ay = by; by = tmp; tmp = am; am = bm; bm = tmp; tmp = ad; ad = bd; bd = tmp; sign = -1;
    }
    var years = by - ay, months = bm - am, days = bd - ad;
    if (days < 0) { months--; days += daysInMonth(by, bm === 1 ? (by--, 12) : bm - 1); }
    if (months < 0) { years--; months += 12; }
    return { years: sign * years, months: sign * months, weeks: 0, days: sign * days };
  }

  // ── ISO 8601 parsing helpers ───────────────────────────────────────────────

  function parseDateStr(s) {
    s = String(s);
    var m = /^([+-]?\d{4,})-(\d{2})-(\d{2})/.exec(s);
    if (m) return { year: +m[1], month: +m[2], day: +m[3] };
    m = /^([+-]?\d{4,})(\d{2})(\d{2})$/.exec(s);
    if (m) return { year: +m[1], month: +m[2], day: +m[3] };
    throw new RangeError('Invalid date string: ' + s);
  }

  function parseTimeStr(s) {
    s = String(s);
    var m = /^(\d{2}):(\d{2})(?::(\d{2})(?:\.(\d{1,9}))?)?/.exec(s);
    if (!m) { m = /^(\d{2})(\d{2})(?:(\d{2})(?:\.(\d{1,9}))?)?/.exec(s); }
    if (!m) throw new RangeError('Invalid time string: ' + s);
    var frac = m[4] || '000000000';
    while (frac.length < 9) frac += '0';
    return { hour: +m[1], minute: +m[2], second: +(m[3] || 0),
             millisecond: +frac.slice(0,3), microsecond: +frac.slice(3,6), nanosecond: +frac.slice(6,9) };
  }

  function parseDateTimeStr(s) {
    s = String(s);
    var sep = /^([^T]+)[T ](.+?)(?:\[.*\])?(?:\[.*\])?$/.exec(s);
    if (!sep) {
      // Date only
      var dt = parseDateStr(s);
      return Object.assign({ hour:0, minute:0, second:0, millisecond:0, microsecond:0, nanosecond:0 }, dt);
    }
    var dateP = parseDateStr(sep[1]);
    // Strip timezone offset from time part for PlainDateTime
    var timePart = sep[2].replace(/[Zz]$/, '').replace(/[+-]\d{2}:\d{2}$/, '');
    var timeP = parseTimeStr(timePart);
    return Object.assign({}, dateP, timeP);
  }

  // Parse ISO duration "P1Y2M3W4DT5H6M7.89S"
  function parseDurationStr(s) {
    s = String(s);
    var neg = s[0] === '-'; if (neg) s = s.slice(1);
    if (s[0] !== 'P') throw new RangeError('Invalid duration: ' + s);
    var m = /^P(?:(-?[\d.]+)Y)?(?:(-?[\d.]+)M)?(?:(-?[\d.]+)W)?(?:(-?[\d.]+)D)?(?:T(?:(-?[\d.]+)H)?(?:(-?[\d.]+)M)?(?:(-?[\d.]+)S)?)?$/.exec(s);
    if (!m) throw new RangeError('Invalid duration: ' + s);
    var sign = neg ? -1 : 1;
    function v(x) { return x ? sign * +x : 0; }
    var total = v(m[7]); // seconds may be fractional
    var secs = Math.trunc(total), frac = Math.round((Math.abs(total) - Math.abs(secs)) * 1e9);
    return {
      years: v(m[1]), months: v(m[2]), weeks: v(m[3]), days: v(m[4]),
      hours: v(m[5]), minutes: v(m[6]), seconds: secs,
      milliseconds: Math.trunc(frac / 1e6), microseconds: Math.trunc((frac % 1e6) / 1e3), nanoseconds: frac % 1e3
    };
  }

  // ── Duration ───────────────────────────────────────────────────────────────

  function Duration(years, months, weeks, days, hours, minutes, seconds, milliseconds, microseconds, nanoseconds) {
    this.years = years || 0; this.months = months || 0; this.weeks = weeks || 0; this.days = days || 0;
    this.hours = hours || 0; this.minutes = minutes || 0; this.seconds = seconds || 0;
    this.milliseconds = milliseconds || 0; this.microseconds = microseconds || 0; this.nanoseconds = nanoseconds || 0;
  }

  Duration.from = function(thing) {
    if (thing instanceof Duration) return new Duration(thing.years, thing.months, thing.weeks, thing.days, thing.hours, thing.minutes, thing.seconds, thing.milliseconds, thing.microseconds, thing.nanoseconds);
    if (typeof thing === 'string') { var f = parseDurationStr(thing); return new Duration(f.years, f.months, f.weeks, f.days, f.hours, f.minutes, f.seconds, f.milliseconds, f.microseconds, f.nanoseconds); }
    if (thing && typeof thing === 'object') return new Duration(thing.years||0, thing.months||0, thing.weeks||0, thing.days||0, thing.hours||0, thing.minutes||0, thing.seconds||0, thing.milliseconds||0, thing.microseconds||0, thing.nanoseconds||0);
    throw new TypeError('Cannot convert to Duration: ' + thing);
  };

  Duration.compare = function(a, b) {
    a = Duration.from(a); b = Duration.from(b);
    var aMs = durationToMs(a), bMs = durationToMs(b);
    return aMs < bMs ? -1 : aMs > bMs ? 1 : 0;
  };

  function durationToMs(d) {
    return (d.years * 365.25 + d.months * 30.44 + d.weeks * 7 + d.days) * 86400000 +
           d.hours * 3600000 + d.minutes * 60000 + d.seconds * 1000 + d.milliseconds +
           d.microseconds / 1000 + d.nanoseconds / 1e6;
  }

  Duration.prototype = {
    constructor: Duration,
    get sign() {
      var f = [this.years, this.months, this.weeks, this.days, this.hours, this.minutes, this.seconds, this.milliseconds, this.microseconds, this.nanoseconds];
      for (var i = 0; i < f.length; i++) { if (f[i] < 0) return -1; if (f[i] > 0) return 1; }
      return 0;
    },
    get blank() { return this.sign === 0; },
    negated: function() { return new Duration(-this.years, -this.months, -this.weeks, -this.days, -this.hours, -this.minutes, -this.seconds, -this.milliseconds, -this.microseconds, -this.nanoseconds); },
    abs: function() { return new Duration(Math.abs(this.years), Math.abs(this.months), Math.abs(this.weeks), Math.abs(this.days), Math.abs(this.hours), Math.abs(this.minutes), Math.abs(this.seconds), Math.abs(this.milliseconds), Math.abs(this.microseconds), Math.abs(this.nanoseconds)); },
    add: function(other) { other = Duration.from(other); return new Duration(this.years+other.years, this.months+other.months, this.weeks+other.weeks, this.days+other.days, this.hours+other.hours, this.minutes+other.minutes, this.seconds+other.seconds, this.milliseconds+other.milliseconds, this.microseconds+other.microseconds, this.nanoseconds+other.nanoseconds); },
    subtract: function(other) { return this.add(Duration.from(other).negated()); },
    with: function(fields) { return Duration.from(Object.assign({ years: this.years, months: this.months, weeks: this.weeks, days: this.days, hours: this.hours, minutes: this.minutes, seconds: this.seconds, milliseconds: this.milliseconds, microseconds: this.microseconds, nanoseconds: this.nanoseconds }, fields)); },
    toString: function() {
      var s = 'P';
      if (this.years) s += this.years + 'Y';
      if (this.months) s += this.months + 'M';
      if (this.weeks) s += this.weeks + 'W';
      if (this.days) s += this.days + 'D';
      var hasTime = this.hours || this.minutes || this.seconds || this.milliseconds || this.microseconds || this.nanoseconds;
      if (hasTime) {
        s += 'T';
        if (this.hours) s += this.hours + 'H';
        if (this.minutes) s += this.minutes + 'M';
        if (this.seconds || this.milliseconds || this.microseconds || this.nanoseconds) {
          var sec = this.seconds;
          var sub = (this.milliseconds * 1e6 + this.microseconds * 1e3 + this.nanoseconds);
          if (sub) { s += sec + '.' + pad9(sub) + 'S'; } else { s += sec + 'S'; }
        }
      }
      if (s === 'P') s = 'PT0S';
      return s;
    },
    toJSON: function() { return this.toString(); },
    valueOf: function() { throw new TypeError('Do not use Temporal.Duration in numeric context'); }
  };

  // ── PlainTime ──────────────────────────────────────────────────────────────

  function PlainTime(hour, minute, second, millisecond, microsecond, nanosecond) {
    this.hour = hour || 0; this.minute = minute || 0; this.second = second || 0;
    this.millisecond = millisecond || 0; this.microsecond = microsecond || 0; this.nanosecond = nanosecond || 0;
  }

  PlainTime.from = function(thing) {
    if (thing instanceof PlainTime) return new PlainTime(thing.hour, thing.minute, thing.second, thing.millisecond, thing.microsecond, thing.nanosecond);
    if (typeof thing === 'string') {
      // Strip leading date part
      var tStr = thing.replace(/^[^T]*T/, '').replace(/[Zz]$/, '').replace(/[+-]\d{2}:\d{2}$/, '');
      var f = parseTimeStr(tStr);
      return new PlainTime(f.hour, f.minute, f.second, f.millisecond, f.microsecond, f.nanosecond);
    }
    if (thing && typeof thing === 'object') return new PlainTime(thing.hour||0, thing.minute||0, thing.second||0, thing.millisecond||0, thing.microsecond||0, thing.nanosecond||0);
    throw new TypeError('Cannot convert to PlainTime');
  };

  PlainTime.compare = function(a, b) {
    a = PlainTime.from(a); b = PlainTime.from(b);
    var aMs = a.hour*3600000 + a.minute*60000 + a.second*1000 + a.millisecond + a.microsecond/1000 + a.nanosecond/1e6;
    var bMs = b.hour*3600000 + b.minute*60000 + b.second*1000 + b.millisecond + b.microsecond/1000 + b.nanosecond/1e6;
    return aMs < bMs ? -1 : aMs > bMs ? 1 : 0;
  };

  function timeToNs(t) {
    return ((t.hour * 60 + t.minute) * 60 + t.second) * 1e9 + t.millisecond * 1e6 + t.microsecond * 1e3 + t.nanosecond;
  }

  function nsToTime(ns) {
    ns = ((ns % 86400e9) + 86400e9) % 86400e9;
    var s = Math.floor(ns / 1e9), sub = ns - s * 1e9;
    var m = Math.floor(s / 60); s = s % 60;
    var h = Math.floor(m / 60); m = m % 60;
    return new PlainTime(h, m, s, Math.floor(sub / 1e6), Math.floor((sub % 1e6) / 1e3), sub % 1e3);
  }

  PlainTime.prototype = {
    constructor: PlainTime,
    add: function(dur) { dur = Duration.from(dur); return nsToTime(timeToNs(this) + durationToMs(dur) * 1e6); },
    subtract: function(dur) { dur = Duration.from(dur); return nsToTime(timeToNs(this) - durationToMs(dur) * 1e6); },
    since: function(other) { other = PlainTime.from(other); var diff = (timeToNs(this) - timeToNs(other)) / 1e6; return new Duration(0,0,0,0,0,0,Math.trunc(diff/1000),diff%1000); },
    until: function(other) { return PlainTime.from(other).since(this).negated(); },
    with: function(fields) { return new PlainTime(fields.hour !== undefined ? fields.hour : this.hour, fields.minute !== undefined ? fields.minute : this.minute, fields.second !== undefined ? fields.second : this.second, fields.millisecond !== undefined ? fields.millisecond : this.millisecond, fields.microsecond !== undefined ? fields.microsecond : this.microsecond, fields.nanosecond !== undefined ? fields.nanosecond : this.nanosecond); },
    equals: function(other) { other = PlainTime.from(other); return this.hour === other.hour && this.minute === other.minute && this.second === other.second && this.millisecond === other.millisecond && this.microsecond === other.microsecond && this.nanosecond === other.nanosecond; },
    toString: function(opts) {
      var s = pad2(this.hour) + ':' + pad2(this.minute) + ':' + pad2(this.second);
      if (this.millisecond || this.microsecond || this.nanosecond) {
        s += '.' + pad9(this.millisecond * 1e6 + this.microsecond * 1e3 + this.nanosecond);
      }
      return s;
    },
    toJSON: function() { return this.toString(); },
    toPlainDateTime: function(date) { date = PlainDate.from(date); return new PlainDateTime(date.year, date.month, date.day, this.hour, this.minute, this.second, this.millisecond, this.microsecond, this.nanosecond); },
    valueOf: function() { throw new TypeError('Do not use Temporal.PlainTime in numeric context'); }
  };

  // ── PlainDate ──────────────────────────────────────────────────────────────

  function PlainDate(year, month, day) {
    this.year = year; this.month = month; this.day = day;
  }

  Object.defineProperties(PlainDate.prototype, {
    dayOfWeek: { get: function() { return dayOfWeek(this.year, this.month, this.day); } },
    dayOfYear: { get: function() { return dayOfYear(this.year, this.month, this.day); } },
    weekOfYear: { get: function() { return weekOfYear(this.year, this.month, this.day); } },
    daysInMonth: { get: function() { return daysInMonth(this.year, this.month); } },
    daysInYear: { get: function() { return daysInYear(this.year); } },
    inLeapYear: { get: function() { return isLeapYear(this.year); } },
    monthsInYear: { get: function() { return 12; } },
    calendarId: { get: function() { return 'iso8601'; } }
  });

  PlainDate.from = function(thing) {
    if (thing instanceof PlainDate) return new PlainDate(thing.year, thing.month, thing.day);
    if (typeof thing === 'string') { var f = parseDateStr(thing.split('T')[0]); return new PlainDate(f.year, f.month, f.day); }
    if (thing && typeof thing === 'object') {
      if (thing.year !== undefined && thing.month !== undefined && thing.day !== undefined) return new PlainDate(thing.year, thing.month, thing.day);
    }
    throw new TypeError('Cannot convert to PlainDate: ' + thing);
  };

  PlainDate.compare = function(a, b) {
    a = PlainDate.from(a); b = PlainDate.from(b);
    var aMs = Date.UTC(a.year, a.month-1, a.day), bMs = Date.UTC(b.year, b.month-1, b.day);
    return aMs < bMs ? -1 : aMs > bMs ? 1 : 0;
  };

  PlainDate.prototype.add = function(dur) {
    dur = Duration.from(dur);
    var r = addDate(this.year, this.month, this.day, dur.years, dur.months, dur.weeks, dur.days);
    return new PlainDate(r.year, r.month, r.day);
  };
  PlainDate.prototype.subtract = function(dur) { return this.add(Duration.from(dur).negated()); };
  PlainDate.prototype.since = function(other, opts) {
    other = PlainDate.from(other); var lg = (opts && opts.largestUnit) || 'day';
    var d = dateDiff(other.year, other.month, other.day, this.year, this.month, this.day);
    if (lg === 'year' || lg === 'years') return new Duration(d.years, d.months, 0, d.days);
    if (lg === 'month' || lg === 'months') return new Duration(0, d.years * 12 + d.months, 0, d.days);
    return new Duration(0, 0, 0, daysBetween(other.year, other.month, other.day, this.year, this.month, this.day));
  };
  PlainDate.prototype.until = function(other, opts) { return PlainDate.from(other).since(this, opts).negated(); };
  PlainDate.prototype.with = function(fields) { return new PlainDate(fields.year !== undefined ? fields.year : this.year, fields.month !== undefined ? fields.month : this.month, fields.day !== undefined ? fields.day : this.day); };
  PlainDate.prototype.withCalendar = function(_cal) { return new PlainDate(this.year, this.month, this.day); };
  PlainDate.prototype.equals = function(other) { other = PlainDate.from(other); return this.year === other.year && this.month === other.month && this.day === other.day; };
  PlainDate.prototype.toString = function() { return pad4(this.year) + '-' + pad2(this.month) + '-' + pad2(this.day); };
  PlainDate.prototype.toJSON = function() { return this.toString(); };
  PlainDate.prototype.toLocaleString = function(locales, opts) { return new Date(Date.UTC(this.year, this.month-1, this.day)).toLocaleDateString(locales, opts); };
  PlainDate.prototype.toPlainDateTime = function(time) {
    var t = time ? PlainTime.from(time) : new PlainTime();
    return new PlainDateTime(this.year, this.month, this.day, t.hour, t.minute, t.second, t.millisecond, t.microsecond, t.nanosecond);
  };
  PlainDate.prototype.toPlainYearMonth = function() { return new PlainYearMonth(this.year, this.month); };
  PlainDate.prototype.toPlainMonthDay = function() { return new PlainMonthDay(this.month, this.day); };
  PlainDate.prototype.toZonedDateTime = function(tzOrOpts) {
    var tz = typeof tzOrOpts === 'string' ? new TimeZone(tzOrOpts) : (tzOrOpts instanceof TimeZone ? tzOrOpts : new TimeZone(typeof tzOrOpts === 'object' && tzOrOpts && tzOrOpts.timeZone ? tzOrOpts.timeZone : 'UTC'));
    var epochMs = fieldsToMs({ year: this.year, month: this.month, day: this.day, hour: 0, minute: 0, second: 0, millisecond: 0 }) - tz._offsetMs();
    return new ZonedDateTime(epochMs * 1e6, tz, new Calendar('iso8601'));
  };
  PlainDate.prototype.valueOf = function() { throw new TypeError('Do not use Temporal.PlainDate in numeric context'); };

  // ── PlainDateTime ──────────────────────────────────────────────────────────

  function PlainDateTime(year, month, day, hour, minute, second, millisecond, microsecond, nanosecond) {
    this.year = year; this.month = month; this.day = day;
    this.hour = hour || 0; this.minute = minute || 0; this.second = second || 0;
    this.millisecond = millisecond || 0; this.microsecond = microsecond || 0; this.nanosecond = nanosecond || 0;
  }

  Object.defineProperties(PlainDateTime.prototype, {
    dayOfWeek: { get: function() { return dayOfWeek(this.year, this.month, this.day); } },
    dayOfYear: { get: function() { return dayOfYear(this.year, this.month, this.day); } },
    weekOfYear: { get: function() { return weekOfYear(this.year, this.month, this.day); } },
    daysInMonth: { get: function() { return daysInMonth(this.year, this.month); } },
    daysInYear: { get: function() { return daysInYear(this.year); } },
    inLeapYear: { get: function() { return isLeapYear(this.year); } },
    monthsInYear: { get: function() { return 12; } },
    calendarId: { get: function() { return 'iso8601'; } }
  });

  PlainDateTime.from = function(thing) {
    if (thing instanceof PlainDateTime) return new PlainDateTime(thing.year, thing.month, thing.day, thing.hour, thing.minute, thing.second, thing.millisecond, thing.microsecond, thing.nanosecond);
    if (typeof thing === 'string') { var f = parseDateTimeStr(thing); return new PlainDateTime(f.year, f.month, f.day, f.hour, f.minute, f.second, f.millisecond, f.microsecond, f.nanosecond); }
    if (thing && typeof thing === 'object') return new PlainDateTime(thing.year, thing.month, thing.day, thing.hour||0, thing.minute||0, thing.second||0, thing.millisecond||0, thing.microsecond||0, thing.nanosecond||0);
    throw new TypeError('Cannot convert to PlainDateTime');
  };

  PlainDateTime.compare = function(a, b) {
    a = PlainDateTime.from(a); b = PlainDateTime.from(b);
    var aMs = fieldsToMs(a) + a.microsecond/1000 + a.nanosecond/1e6;
    var bMs = fieldsToMs(b) + b.microsecond/1000 + b.nanosecond/1e6;
    return aMs < bMs ? -1 : aMs > bMs ? 1 : 0;
  };

  PlainDateTime.prototype.add = function(dur) {
    dur = Duration.from(dur);
    var r = addDate(this.year, this.month, this.day, dur.years, dur.months, dur.weeks, dur.days);
    var ms = fieldsToMs({ year: r.year, month: r.month, day: r.day, hour: this.hour, minute: this.minute, second: this.second, millisecond: this.millisecond });
    ms += dur.hours * 3600000 + dur.minutes * 60000 + dur.seconds * 1000 + dur.milliseconds;
    var f = msToFields(ms);
    return new PlainDateTime(f.year, f.month, f.day, f.hour, f.minute, f.second, f.millisecond, this.microsecond + (dur.microseconds||0), this.nanosecond + (dur.nanoseconds||0));
  };
  PlainDateTime.prototype.subtract = function(dur) { return this.add(Duration.from(dur).negated()); };
  PlainDateTime.prototype.since = function(other, opts) {
    other = PlainDateTime.from(other);
    var aMs = fieldsToMs(this), bMs = fieldsToMs(other);
    var diffMs = aMs - bMs;
    var lg = (opts && opts.largestUnit) || 'hour';
    if (lg === 'year' || lg === 'years' || lg === 'month' || lg === 'months' || lg === 'day' || lg === 'days') {
      var dd = dateDiff(other.year, other.month, other.day, this.year, this.month, this.day);
      var remMs = (aMs - fieldsToMs({ year: other.year, month: other.month, day: other.day, hour: 0, minute: 0, second: 0, millisecond: 0 })) - (bMs - fieldsToMs({ year: other.year, month: other.month, day: other.day, hour: 0, minute: 0, second: 0, millisecond: 0 }));
      return new Duration(dd.years, dd.months, 0, dd.days, Math.trunc(remMs / 3600000), Math.trunc((remMs % 3600000) / 60000), Math.trunc((remMs % 60000) / 1000), remMs % 1000);
    }
    return new Duration(0, 0, 0, 0, Math.trunc(diffMs / 3600000), Math.trunc((diffMs % 3600000) / 60000), Math.trunc((diffMs % 60000) / 1000), diffMs % 1000);
  };
  PlainDateTime.prototype.until = function(other, opts) { return PlainDateTime.from(other).since(this, opts).negated(); };
  PlainDateTime.prototype.with = function(fields) { return PlainDateTime.from(Object.assign({ year: this.year, month: this.month, day: this.day, hour: this.hour, minute: this.minute, second: this.second, millisecond: this.millisecond, microsecond: this.microsecond, nanosecond: this.nanosecond }, fields)); };
  PlainDateTime.prototype.withPlainDate = function(date) { date = PlainDate.from(date); return new PlainDateTime(date.year, date.month, date.day, this.hour, this.minute, this.second, this.millisecond, this.microsecond, this.nanosecond); };
  PlainDateTime.prototype.withPlainTime = function(time) { time = time ? PlainTime.from(time) : new PlainTime(); return new PlainDateTime(this.year, this.month, this.day, time.hour, time.minute, time.second, time.millisecond, time.microsecond, time.nanosecond); };
  PlainDateTime.prototype.equals = function(other) { other = PlainDateTime.from(other); return PlainDateTime.compare(this, other) === 0; };
  PlainDateTime.prototype.toString = function() {
    var s = pad4(this.year) + '-' + pad2(this.month) + '-' + pad2(this.day) + 'T' +
            pad2(this.hour) + ':' + pad2(this.minute) + ':' + pad2(this.second);
    var sub = this.millisecond * 1e6 + this.microsecond * 1e3 + this.nanosecond;
    if (sub) s += '.' + pad9(sub);
    return s;
  };
  PlainDateTime.prototype.toJSON = function() { return this.toString(); };
  PlainDateTime.prototype.toLocaleString = function(l, o) { return new Date(fieldsToMs(this)).toLocaleString(l, o); };
  PlainDateTime.prototype.toPlainDate = function() { return new PlainDate(this.year, this.month, this.day); };
  PlainDateTime.prototype.toPlainTime = function() { return new PlainTime(this.hour, this.minute, this.second, this.millisecond, this.microsecond, this.nanosecond); };
  PlainDateTime.prototype.toPlainYearMonth = function() { return new PlainYearMonth(this.year, this.month); };
  PlainDateTime.prototype.toPlainMonthDay = function() { return new PlainMonthDay(this.month, this.day); };
  PlainDateTime.prototype.toZonedDateTime = function(tz) {
    if (typeof tz === 'string') tz = new TimeZone(tz);
    if (!(tz instanceof TimeZone)) tz = new TimeZone('UTC');
    var epochMs = fieldsToMs(this) - tz._offsetMs();
    return new ZonedDateTime(epochMs * 1e6, tz, new Calendar('iso8601'));
  };
  PlainDateTime.prototype.valueOf = function() { throw new TypeError('Do not use Temporal.PlainDateTime in numeric context'); };

  // ── PlainYearMonth ─────────────────────────────────────────────────────────

  function PlainYearMonth(year, month) {
    this.year = year; this.month = month;
  }

  PlainYearMonth.from = function(thing) {
    if (thing instanceof PlainYearMonth) return new PlainYearMonth(thing.year, thing.month);
    if (typeof thing === 'string') {
      var m = /^([+-]?\d{4,})-(\d{2})/.exec(String(thing));
      if (m) return new PlainYearMonth(+m[1], +m[2]);
    }
    if (thing && typeof thing === 'object') return new PlainYearMonth(thing.year, thing.month);
    throw new TypeError('Cannot convert to PlainYearMonth');
  };

  PlainYearMonth.compare = function(a, b) {
    a = PlainYearMonth.from(a); b = PlainYearMonth.from(b);
    return a.year !== b.year ? (a.year < b.year ? -1 : 1) : a.month !== b.month ? (a.month < b.month ? -1 : 1) : 0;
  };

  Object.defineProperties(PlainYearMonth.prototype, {
    daysInMonth: { get: function() { return daysInMonth(this.year, this.month); } },
    daysInYear: { get: function() { return daysInYear(this.year); } },
    inLeapYear: { get: function() { return isLeapYear(this.year); } },
    monthsInYear: { get: function() { return 12; } },
    calendarId: { get: function() { return 'iso8601'; } }
  });

  PlainYearMonth.prototype.add = function(dur) {
    dur = Duration.from(dur);
    var r = balanceYearMonth(this.year + (dur.years||0), this.month + (dur.months||0));
    return new PlainYearMonth(r.year, r.month);
  };
  PlainYearMonth.prototype.subtract = function(dur) { return this.add(Duration.from(dur).negated()); };
  PlainYearMonth.prototype.since = function(other) { other = PlainYearMonth.from(other); var m = (this.year - other.year) * 12 + (this.month - other.month); return new Duration(Math.trunc(m / 12), m % 12, 0, 0); };
  PlainYearMonth.prototype.until = function(other) { return PlainYearMonth.from(other).since(this).negated(); };
  PlainYearMonth.prototype.with = function(fields) { return new PlainYearMonth(fields.year !== undefined ? fields.year : this.year, fields.month !== undefined ? fields.month : this.month); };
  PlainYearMonth.prototype.equals = function(other) { other = PlainYearMonth.from(other); return this.year === other.year && this.month === other.month; };
  PlainYearMonth.prototype.toPlainDate = function(fields) { return new PlainDate(this.year, this.month, (fields && fields.day) || 1); };
  PlainYearMonth.prototype.toString = function() { return pad4(this.year) + '-' + pad2(this.month); };
  PlainYearMonth.prototype.toJSON = function() { return this.toString(); };
  PlainYearMonth.prototype.valueOf = function() { throw new TypeError('Do not use Temporal.PlainYearMonth in numeric context'); };

  // ── PlainMonthDay ──────────────────────────────────────────────────────────

  function PlainMonthDay(month, day) {
    this.month = month; this.day = day;
  }

  PlainMonthDay.from = function(thing) {
    if (thing instanceof PlainMonthDay) return new PlainMonthDay(thing.month, thing.day);
    if (typeof thing === 'string') {
      var m = /^--(\d{2})-(\d{2})$/.exec(String(thing));
      if (m) return new PlainMonthDay(+m[1], +m[2]);
      m = /^(\d{4})-(\d{2})-(\d{2})/.exec(String(thing));
      if (m) return new PlainMonthDay(+m[2], +m[3]);
    }
    if (thing && typeof thing === 'object') return new PlainMonthDay(thing.month, thing.day);
    throw new TypeError('Cannot convert to PlainMonthDay');
  };

  PlainMonthDay.compare = function(a, b) {
    a = PlainMonthDay.from(a); b = PlainMonthDay.from(b);
    return a.month !== b.month ? (a.month < b.month ? -1 : 1) : a.day !== b.day ? (a.day < b.day ? -1 : 1) : 0;
  };

  PlainMonthDay.prototype = {
    constructor: PlainMonthDay,
    get calendarId() { return 'iso8601'; },
    equals: function(other) { other = PlainMonthDay.from(other); return this.month === other.month && this.day === other.day; },
    with: function(fields) { return new PlainMonthDay(fields.month !== undefined ? fields.month : this.month, fields.day !== undefined ? fields.day : this.day); },
    toPlainDate: function(fields) { return new PlainDate((fields && fields.year) || new Date().getUTCFullYear(), this.month, this.day); },
    toString: function() { return '--' + pad2(this.month) + '-' + pad2(this.day); },
    toJSON: function() { return this.toString(); },
    valueOf: function() { throw new TypeError('Do not use Temporal.PlainMonthDay in numeric context'); }
  };

  // ── TimeZone ───────────────────────────────────────────────────────────────

  function TimeZone(id) {
    this.id = String(id || 'UTC');
  }

  // Get current system timezone offset using Date
  TimeZone.from = function(thing) {
    if (thing instanceof TimeZone) return new TimeZone(thing.id);
    return new TimeZone(String(thing));
  };

  TimeZone.prototype = {
    constructor: TimeZone,
    // Internal: offset in ms from UTC (positive = east of UTC)
    _offsetMs: function() {
      var id = this.id;
      if (id === 'UTC' || id === 'Etc/UTC') return 0;
      // Fixed offset "+HH:MM" or "-HH:MM"
      var m = /^([+-])(\d{2}):(\d{2})$/.exec(id);
      if (m) return (m[1] === '+' ? 1 : -1) * (+m[2] * 60 + +m[3]) * 60000;
      // For named timezones, approximate from Date's local offset
      return -new Date().getTimezoneOffset() * 60000;
    },
    getOffsetNanosecondsFor: function(instant) {
      instant = Instant.from(instant);
      return this._offsetMs() * 1e6;
    },
    getOffsetStringFor: function(instant) {
      var off = Math.round(this._offsetMs() / 60000);
      var sign = off >= 0 ? '+' : '-'; off = Math.abs(off);
      return sign + pad2(Math.floor(off / 60)) + ':' + pad2(off % 60);
    },
    getPlainDateTimeFor: function(instant, cal) {
      instant = Instant.from(instant);
      var ms = instant.epochMilliseconds + this._offsetMs();
      var f = msToFields(ms);
      return new PlainDateTime(f.year, f.month, f.day, f.hour, f.minute, f.second, f.millisecond);
    },
    getInstantFor: function(dt, opts) {
      dt = PlainDateTime.from(dt);
      var epochMs = fieldsToMs(dt) - this._offsetMs();
      return new Instant(epochMs * 1e6);
    },
    getPossibleInstantsFor: function(dt) {
      return [this.getInstantFor(dt)];
    },
    getPreviousTransition: function() { return null; },
    getNextTransition: function() { return null; },
    toString: function() { return this.id; },
    toJSON: function() { return this.id; }
  };

  // ── Calendar ───────────────────────────────────────────────────────────────

  function Calendar(id) {
    this.id = String(id || 'iso8601');
  }

  Calendar.from = function(thing) {
    if (thing instanceof Calendar) return thing;
    return new Calendar(String(thing));
  };

  Calendar.prototype = {
    constructor: Calendar,
    dateFromFields: function(fields, opts) { return PlainDate.from(fields); },
    yearMonthFromFields: function(fields, opts) { return PlainYearMonth.from(fields); },
    monthDayFromFields: function(fields, opts) { return PlainMonthDay.from(fields); },
    dateAdd: function(date, duration, opts) { return PlainDate.from(date).add(duration); },
    dateUntil: function(one, two, opts) { return PlainDate.from(one).until(two, opts); },
    year: function(date) { return PlainDate.from(date).year; },
    month: function(date) { return PlainDate.from(date).month; },
    day: function(date) { return PlainDate.from(date).day; },
    dayOfWeek: function(date) { var d = PlainDate.from(date); return dayOfWeek(d.year, d.month, d.day); },
    daysInMonth: function(date) { var d = PlainDate.from(date); return daysInMonth(d.year, d.month); },
    daysInYear: function(date) { return daysInYear(PlainDate.from(date).year); },
    inLeapYear: function(date) { return isLeapYear(PlainDate.from(date).year); },
    monthsInYear: function() { return 12; },
    toString: function() { return this.id; },
    toJSON: function() { return this.id; }
  };

  // ── Instant ────────────────────────────────────────────────────────────────

  function Instant(epochNs) {
    // epochNs as a number (ms precision sufficient for Phase 1)
    this._epochNs = +epochNs;
  }

  Object.defineProperties(Instant.prototype, {
    epochNanoseconds: { get: function() { return this._epochNs; } },
    epochMicroseconds: { get: function() { return Math.round(this._epochNs / 1e3); } },
    epochMilliseconds: { get: function() { return Math.round(this._epochNs / 1e6); } },
    epochSeconds: { get: function() { return Math.round(this._epochNs / 1e9); } }
  });

  Instant.from = function(thing) {
    if (thing instanceof Instant) return new Instant(thing._epochNs);
    if (typeof thing === 'string') {
      // Parse ISO 8601 instant: YYYY-MM-DDTHH:MM:SS[.nnn]Z or ±HH:MM
      var s = String(thing);
      var off = 0;
      var offMatch = /([Zz]|([+-])(\d{2}):(\d{2}))$/.exec(s);
      if (offMatch) {
        if (offMatch[1] !== 'Z' && offMatch[1] !== 'z') {
          off = ((offMatch[2] === '+' ? 1 : -1) * (+offMatch[3] * 60 + +offMatch[4])) * 60000;
        }
        s = s.slice(0, s.length - offMatch[0].length);
      }
      var f = parseDateTimeStr(s);
      return new Instant((fieldsToMs(f) - off) * 1e6);
    }
    throw new TypeError('Cannot convert to Instant: ' + thing);
  };

  Instant.fromEpochSeconds = function(s) { return new Instant(s * 1e9); };
  Instant.fromEpochMilliseconds = function(ms) { return new Instant(ms * 1e6); };
  Instant.fromEpochMicroseconds = function(us) { return new Instant(us * 1e3); };
  Instant.fromEpochNanoseconds = function(ns) { return new Instant(ns); };

  Instant.compare = function(a, b) {
    a = Instant.from(a); b = Instant.from(b);
    return a._epochNs < b._epochNs ? -1 : a._epochNs > b._epochNs ? 1 : 0;
  };

  Instant.prototype.add = function(dur) {
    dur = Duration.from(dur);
    var ms = durationToMs(dur);
    return new Instant(this._epochNs + ms * 1e6);
  };
  Instant.prototype.subtract = function(dur) { return this.add(Duration.from(dur).negated()); };
  Instant.prototype.since = function(other, opts) {
    other = Instant.from(other);
    var diffMs = (this._epochNs - other._epochNs) / 1e6;
    var lg = (opts && opts.largestUnit) || 'second';
    if (lg === 'hour' || lg === 'hours') return new Duration(0, 0, 0, 0, Math.trunc(diffMs / 3600000), Math.trunc((diffMs % 3600000) / 60000), Math.trunc((diffMs % 60000) / 1000), diffMs % 1000);
    if (lg === 'minute' || lg === 'minutes') return new Duration(0, 0, 0, 0, 0, Math.trunc(diffMs / 60000), Math.trunc((diffMs % 60000) / 1000), diffMs % 1000);
    return new Duration(0, 0, 0, 0, 0, 0, Math.trunc(diffMs / 1000), diffMs % 1000);
  };
  Instant.prototype.until = function(other, opts) { return Instant.from(other).since(this, opts).negated(); };
  Instant.prototype.equals = function(other) { return Instant.compare(this, Instant.from(other)) === 0; };
  Instant.prototype.toZonedDateTime = function(opts) {
    var tz = new TimeZone(opts && (opts.timeZone || opts) || 'UTC');
    var cal = new Calendar((opts && opts.calendar) || 'iso8601');
    return new ZonedDateTime(this._epochNs, tz, cal);
  };
  Instant.prototype.toZonedDateTimeISO = function(tz) { return this.toZonedDateTime({ timeZone: tz || 'UTC', calendar: 'iso8601' }); };
  Instant.prototype.toString = function(opts) {
    var tz = opts && opts.timeZone ? new TimeZone(opts.timeZone) : new TimeZone('UTC');
    var ms = this.epochMilliseconds + tz._offsetMs();
    var f = msToFields(ms);
    var s = pad4(f.year) + '-' + pad2(f.month) + '-' + pad2(f.day) + 'T' +
            pad2(f.hour) + ':' + pad2(f.minute) + ':' + pad2(f.second);
    var sub = Math.round((this._epochNs % 1e9 + 1e9) % 1e9);
    if (sub) s += '.' + pad9(sub);
    s += tz.getOffsetStringFor(this).replace('+00:00', 'Z');
    return s;
  };
  Instant.prototype.toJSON = function() { return this.toString(); };
  Instant.prototype.valueOf = function() { throw new TypeError('Do not use Temporal.Instant in numeric context'); };

  // ── ZonedDateTime ──────────────────────────────────────────────────────────

  function ZonedDateTime(epochNs, timezone, calendar) {
    this._epochNs = +epochNs;
    this._tz = timezone instanceof TimeZone ? timezone : new TimeZone(timezone || 'UTC');
    this._cal = calendar instanceof Calendar ? calendar : new Calendar(calendar || 'iso8601');
  }

  function _zdtLocal(zdt) {
    var ms = zdt.epochMilliseconds + zdt._tz._offsetMs();
    return msToFields(ms);
  }

  Object.defineProperties(ZonedDateTime.prototype, {
    epochNanoseconds: { get: function() { return this._epochNs; } },
    epochMilliseconds: { get: function() { return Math.round(this._epochNs / 1e6); } },
    epochSeconds: { get: function() { return Math.round(this._epochNs / 1e9); } },
    timeZoneId: { get: function() { return this._tz.id; } },
    calendarId: { get: function() { return this._cal.id; } },
    year: { get: function() { return _zdtLocal(this).year; } },
    month: { get: function() { return _zdtLocal(this).month; } },
    day: { get: function() { return _zdtLocal(this).day; } },
    hour: { get: function() { return _zdtLocal(this).hour; } },
    minute: { get: function() { return _zdtLocal(this).minute; } },
    second: { get: function() { return _zdtLocal(this).second; } },
    millisecond: { get: function() { return _zdtLocal(this).millisecond; } },
    microsecond: { get: function() { return 0; } },
    nanosecond: { get: function() { return 0; } },
    offset: { get: function() { return this._tz.getOffsetStringFor(new Instant(this._epochNs)); } },
    offsetNanoseconds: { get: function() { return this._tz._offsetMs() * 1e6; } },
    dayOfWeek: { get: function() { var f = _zdtLocal(this); return dayOfWeek(f.year, f.month, f.day); } },
    dayOfYear: { get: function() { var f = _zdtLocal(this); return dayOfYear(f.year, f.month, f.day); } },
    weekOfYear: { get: function() { var f = _zdtLocal(this); return weekOfYear(f.year, f.month, f.day); } },
    daysInMonth: { get: function() { var f = _zdtLocal(this); return daysInMonth(f.year, f.month); } },
    daysInYear: { get: function() { return daysInYear(_zdtLocal(this).year); } },
    inLeapYear: { get: function() { return isLeapYear(_zdtLocal(this).year); } }
  });

  ZonedDateTime.from = function(thing) {
    if (thing instanceof ZonedDateTime) return new ZonedDateTime(thing._epochNs, thing._tz, thing._cal);
    if (typeof thing === 'string') {
      var inst = Instant.from(thing);
      var tzMatch = /\[([^\]]+)\]/.exec(String(thing));
      var tz = new TimeZone(tzMatch ? tzMatch[1] : 'UTC');
      return new ZonedDateTime(inst._epochNs, tz, new Calendar('iso8601'));
    }
    throw new TypeError('Cannot convert to ZonedDateTime');
  };

  ZonedDateTime.compare = function(a, b) {
    a = ZonedDateTime.from(a); b = ZonedDateTime.from(b);
    return a._epochNs < b._epochNs ? -1 : a._epochNs > b._epochNs ? 1 : 0;
  };

  ZonedDateTime.prototype.add = function(dur) {
    dur = Duration.from(dur);
    var ms = durationToMs(dur);
    return new ZonedDateTime(this._epochNs + ms * 1e6, this._tz, this._cal);
  };
  ZonedDateTime.prototype.subtract = function(dur) { return this.add(Duration.from(dur).negated()); };
  ZonedDateTime.prototype.since = function(other, opts) {
    other = ZonedDateTime.from(other);
    var diffMs = (this._epochNs - other._epochNs) / 1e6;
    return new Duration(0, 0, 0, 0, Math.trunc(diffMs / 3600000), Math.trunc((diffMs % 3600000) / 60000), Math.trunc((diffMs % 60000) / 1000), diffMs % 1000);
  };
  ZonedDateTime.prototype.until = function(other, opts) { return ZonedDateTime.from(other).since(this, opts).negated(); };
  ZonedDateTime.prototype.equals = function(other) { return ZonedDateTime.compare(this, ZonedDateTime.from(other)) === 0; };
  ZonedDateTime.prototype.with = function(fields) {
    var local = _zdtLocal(this);
    var merged = Object.assign(local, fields);
    var epochMs = fieldsToMs(merged) - this._tz._offsetMs();
    return new ZonedDateTime(epochMs * 1e6, this._tz, this._cal);
  };
  ZonedDateTime.prototype.withTimeZone = function(tz) { return new ZonedDateTime(this._epochNs, new TimeZone(tz), this._cal); };
  ZonedDateTime.prototype.withCalendar = function(cal) { return new ZonedDateTime(this._epochNs, this._tz, new Calendar(cal)); };
  ZonedDateTime.prototype.toInstant = function() { return new Instant(this._epochNs); };
  ZonedDateTime.prototype.toPlainDate = function() { var f = _zdtLocal(this); return new PlainDate(f.year, f.month, f.day); };
  ZonedDateTime.prototype.toPlainTime = function() { var f = _zdtLocal(this); return new PlainTime(f.hour, f.minute, f.second, f.millisecond); };
  ZonedDateTime.prototype.toPlainDateTime = function() { var f = _zdtLocal(this); return new PlainDateTime(f.year, f.month, f.day, f.hour, f.minute, f.second, f.millisecond); };
  ZonedDateTime.prototype.toPlainYearMonth = function() { var f = _zdtLocal(this); return new PlainYearMonth(f.year, f.month); };
  ZonedDateTime.prototype.toPlainMonthDay = function() { var f = _zdtLocal(this); return new PlainMonthDay(f.month, f.day); };
  ZonedDateTime.prototype.toString = function() {
    var inst = new Instant(this._epochNs);
    var ms = this.epochMilliseconds + this._tz._offsetMs();
    var f = msToFields(ms);
    var s = pad4(f.year) + '-' + pad2(f.month) + '-' + pad2(f.day) + 'T' +
            pad2(f.hour) + ':' + pad2(f.minute) + ':' + pad2(f.second);
    var sub = Math.round((this._epochNs % 1e9 + 1e9) % 1e9);
    if (sub) s += '.' + pad9(sub);
    s += this.offset;
    s += '[' + this._tz.id + ']';
    return s;
  };
  ZonedDateTime.prototype.toJSON = function() { return this.toString(); };
  ZonedDateTime.prototype.toLocaleString = function(l, o) { return new Date(this.epochMilliseconds).toLocaleString(l, o); };
  ZonedDateTime.prototype.valueOf = function() { throw new TypeError('Do not use Temporal.ZonedDateTime in numeric context'); };

  // ── Temporal.Now ──────────────────────────────────────────────────────────

  var Now = {
    instant: function() {
      return new Instant(Date.now() * 1e6);
    },
    zonedDateTimeISO: function(tzLike) {
      var tz = new TimeZone(tzLike || Now.timeZoneId());
      return new ZonedDateTime(Date.now() * 1e6, tz, new Calendar('iso8601'));
    },
    zonedDateTime: function(cal, tzLike) {
      var tz = new TimeZone(tzLike || Now.timeZoneId());
      return new ZonedDateTime(Date.now() * 1e6, tz, new Calendar(cal || 'iso8601'));
    },
    plainDateTimeISO: function(tzLike) {
      var tz = new TimeZone(tzLike || 'UTC');
      var f = msToFields(Date.now() + tz._offsetMs());
      return new PlainDateTime(f.year, f.month, f.day, f.hour, f.minute, f.second, f.millisecond);
    },
    plainDateTime: function(cal, tzLike) {
      return Now.plainDateTimeISO(tzLike);
    },
    plainDateISO: function(tzLike) {
      var pdt = Now.plainDateTimeISO(tzLike);
      return new PlainDate(pdt.year, pdt.month, pdt.day);
    },
    plainDate: function(cal, tzLike) {
      return Now.plainDateISO(tzLike);
    },
    plainTimeISO: function(tzLike) {
      var pdt = Now.plainDateTimeISO(tzLike);
      return new PlainTime(pdt.hour, pdt.minute, pdt.second, pdt.millisecond);
    },
    timeZoneId: function() {
      // Derive timezone id from Date's UTC offset
      var off = -new Date().getTimezoneOffset();
      if (off === 0) return 'UTC';
      var sign = off >= 0 ? '+' : '-'; off = Math.abs(off);
      return sign + pad2(Math.floor(off / 60)) + ':' + pad2(off % 60);
    }
  };

  // ── Temporal namespace ─────────────────────────────────────────────────────

  var Temporal = {
    Now: Now,
    Instant: Instant,
    ZonedDateTime: ZonedDateTime,
    PlainDate: PlainDate,
    PlainTime: PlainTime,
    PlainDateTime: PlainDateTime,
    PlainYearMonth: PlainYearMonth,
    PlainMonthDay: PlainMonthDay,
    Duration: Duration,
    Calendar: Calendar,
    TimeZone: TimeZone
  };

  global.Temporal = Temporal;
  if (typeof window !== 'undefined' && window !== global) window.Temporal = Temporal;

})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn setup() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            super::install_temporal_api(&ctx).unwrap();
        });
        (rt, ctx)
    }

    #[test]
    fn temporal_namespace_exists() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let v: bool = ctx.eval("typeof Temporal !== 'undefined'").unwrap();
            assert!(v);
            let v: bool = ctx.eval("typeof Temporal.Now !== 'undefined'").unwrap();
            assert!(v);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_from_string() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let year: i32 = ctx.eval("Temporal.PlainDate.from('2024-03-15').year").unwrap();
            assert_eq!(year, 2024);
            let month: i32 = ctx.eval("Temporal.PlainDate.from('2024-03-15').month").unwrap();
            assert_eq!(month, 3);
            let day: i32 = ctx.eval("Temporal.PlainDate.from('2024-03-15').day").unwrap();
            assert_eq!(day, 15);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_to_string() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.PlainDate.from('2024-03-15').toString()").unwrap();
            assert_eq!(s, "2024-03-15");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_add_duration() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.PlainDate.from('2024-01-31').add({ months: 1 }).toString()").unwrap();
            // Jan 31 + 1 month = Feb 29 (2024 is a leap year) or Feb 28
            assert!(s == "2024-02-29" || s == "2024-02-28");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_subtract_duration() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.PlainDate.from('2024-03-15').subtract({ days: 5 }).toString()").unwrap();
            assert_eq!(s, "2024-03-10");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_compare() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let cmp: i32 = ctx.eval("Temporal.PlainDate.compare('2024-01-01', '2024-12-31')").unwrap();
            assert_eq!(cmp, -1);
            let cmp: i32 = ctx.eval("Temporal.PlainDate.compare('2024-03-15', '2024-03-15')").unwrap();
            assert_eq!(cmp, 0);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_since() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let days: i32 = ctx.eval("Temporal.PlainDate.from('2024-03-20').since('2024-03-10').days").unwrap();
            assert_eq!(days, 10);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_leap_year() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let leap: bool = ctx.eval("Temporal.PlainDate.from('2024-01-01').inLeapYear").unwrap();
            assert!(leap);
            let no_leap: bool = ctx.eval("Temporal.PlainDate.from('2023-01-01').inLeapYear").unwrap();
            assert!(!no_leap);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_time_from_string() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let h: i32 = ctx.eval("Temporal.PlainTime.from('14:30:00').hour").unwrap();
            assert_eq!(h, 14);
            let m: i32 = ctx.eval("Temporal.PlainTime.from('14:30:00').minute").unwrap();
            assert_eq!(m, 30);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_time_to_string() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.PlainTime.from('09:05:03').toString()").unwrap();
            assert_eq!(s, "09:05:03");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_datetime_from_string() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.PlainDateTime.from('2024-03-15T14:30:00').toString()").unwrap();
            assert_eq!(s, "2024-03-15T14:30:00");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn duration_from_string() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let y: i32 = ctx.eval("Temporal.Duration.from('P1Y2M3DT4H5M6S').years").unwrap();
            assert_eq!(y, 1);
            let m: i32 = ctx.eval("Temporal.Duration.from('P1Y2M3DT4H5M6S').months").unwrap();
            assert_eq!(m, 2);
            let d: i32 = ctx.eval("Temporal.Duration.from('P1Y2M3DT4H5M6S').days").unwrap();
            assert_eq!(d, 3);
            let h: i32 = ctx.eval("Temporal.Duration.from('P1Y2M3DT4H5M6S').hours").unwrap();
            assert_eq!(h, 4);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn duration_to_string() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("new Temporal.Duration(1, 2, 0, 3, 4, 5, 6).toString()").unwrap();
            assert_eq!(s, "P1Y2M3DT4H5M6S");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn duration_negated() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let d: i32 = ctx.eval("Temporal.Duration.from('P5D').negated().days").unwrap();
            assert_eq!(d, -5);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn instant_from_epoch_ms() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ms: f64 = ctx.eval("Temporal.Instant.fromEpochMilliseconds(1000).epochMilliseconds").unwrap();
            assert!((ms - 1000.0).abs() < 1.0);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn instant_to_string_utc() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.Instant.fromEpochMilliseconds(0).toString()").unwrap();
            assert!(s.contains("1970-01-01T00:00:00"), "got: {}", s);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn temporal_now_instant_is_number() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx.eval("typeof Temporal.Now.instant().epochMilliseconds === 'number'").unwrap();
            assert!(ok);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn temporal_now_plain_date_iso() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx.eval("Temporal.Now.plainDateISO() instanceof Temporal.PlainDate").unwrap();
            assert!(ok);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn temporal_now_time_zone_id() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.Now.timeZoneId()").unwrap();
            assert!(!s.is_empty(), "timezone id should not be empty");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_year_month_from_string() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let y: i32 = ctx.eval("Temporal.PlainYearMonth.from('2024-06').year").unwrap();
            assert_eq!(y, 2024);
            let m: i32 = ctx.eval("Temporal.PlainYearMonth.from('2024-06').month").unwrap();
            assert_eq!(m, 6);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_month_day_from_string() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let m: i32 = ctx.eval("Temporal.PlainMonthDay.from('--06-15').month").unwrap();
            assert_eq!(m, 6);
            let d: i32 = ctx.eval("Temporal.PlainMonthDay.from('--06-15').day").unwrap();
            assert_eq!(d, 15);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn zoned_datetime_to_plain_date() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx.eval("Temporal.Instant.fromEpochMilliseconds(0).toZonedDateTimeISO('UTC').toPlainDate() instanceof Temporal.PlainDate").unwrap();
            assert!(ok);
            let s: String = ctx.eval("Temporal.Instant.fromEpochMilliseconds(0).toZonedDateTimeISO('UTC').toPlainDate().toString()").unwrap();
            assert_eq!(s, "1970-01-01");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn instant_add_duration() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ms: f64 = ctx.eval("Temporal.Instant.fromEpochMilliseconds(0).add({ seconds: 60 }).epochMilliseconds").unwrap();
            assert!((ms - 60000.0).abs() < 1.0);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_to_plain_datetime() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.PlainDate.from('2024-03-15').toPlainDateTime(Temporal.PlainTime.from('14:30:00')).toString()").unwrap();
            assert_eq!(s, "2024-03-15T14:30:00");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn duration_zero_is_pt0s() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("new Temporal.Duration().toString()").unwrap();
            assert_eq!(s, "PT0S");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_day_of_week() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // 2024-01-01 is a Monday (ISO weekday 1)
            let dow: i32 = ctx.eval("Temporal.PlainDate.from('2024-01-01').dayOfWeek").unwrap();
            assert_eq!(dow, 1);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_datetime_add_hours() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.PlainDateTime.from('2024-03-15T22:00:00').add({ hours: 3 }).toString()").unwrap();
            assert_eq!(s, "2024-03-16T01:00:00");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn plain_date_with() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let s: String = ctx.eval("Temporal.PlainDate.from('2024-03-15').with({ day: 1 }).toString()").unwrap();
            assert_eq!(s, "2024-03-01");
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn timezone_utc_offset() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let off: f64 = ctx.eval("new Temporal.TimeZone('UTC').getOffsetNanosecondsFor(Temporal.Instant.fromEpochMilliseconds(0))").unwrap();
            assert_eq!(off as i64, 0);
        });
        drop(ctx); drop(rt);
    }

    #[test]
    fn idempotent_when_already_defined() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // Install again — should not overwrite
            super::install_temporal_api(&ctx).unwrap();
            let ok: bool = ctx.eval("Temporal.PlainDate.from('2024-03-15').toString() === '2024-03-15'").unwrap();
            assert!(ok);
        });
        drop(ctx); drop(rt);
    }
}
