//! BUG-646 — `new PaymentRequest()` ran none of the Payment Request API §3.1
//! checks: an empty `methodData`, a negative or non-decimal total and a
//! malformed currency code all constructed silently. The cases below are the
//! WPT `payment-request/payment-request-constructor`, `-ctor-pmi-handling`
//! and `-ctor-currency-code-checks` lists — those files are `.https.` and
//! never reach the constructor through the runner (TLS gap), so they are
//! pinned here on the real runtime (a real `URL` is needed for URL-based
//! payment method identifiers).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn runtime() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "https://example.com/", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt.eval(
        r#"
        globalThis.M = [{ supportedMethods: 'https://pay.example/pr' }];
        globalThis.D = { total: { label: 'T', amount: { currency: 'USD', value: '1.0' } } };
        globalThis.failures = [];
        // Records `label` unless `f` throws exactly `Ctor` (or throws nothing when `Ctor` is null).
        globalThis.expect = function(label, Ctor, f) {
          try { f(); if (Ctor) failures.push(label + ': no throw'); }
          catch (e) {
            if (!Ctor) failures.push(label + ': threw ' + e);
            else if (!(e instanceof Ctor)) failures.push(label + ': wrong error ' + e);
          }
        };
        globalThis.amt = function(currency, value) {
          return { total: { label: 'T', amount: { currency: currency, value: value } } };
        };
        "#,
    )
    .unwrap();
    rt
}

fn failures(rt: &V8JsRuntime, script: &str) -> String {
    rt.eval(script).unwrap();
    match rt.eval("failures.join('\\n')").unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn bug646_the_four_reported_cases_throw() {
    let rt = runtime();
    let f = failures(
        &rt,
        r#"
        expect('empty methodData', TypeError, () => new PaymentRequest([], D));
        expect('negative total', TypeError, () => new PaymentRequest(M, amt('USD', '-5.00')));
        expect('non-decimal', TypeError, () => new PaymentRequest(M, amt('USD', 'not-a-number')));
        expect('2-letter currency', RangeError, () => new PaymentRequest(M, amt('US', '1.00')));
        expect('valid request', null, () => new PaymentRequest(M, D));
        "#,
    );
    assert_eq!(f, "");
}

#[test]
fn bug646_decimal_monetary_values() {
    let rt = runtime();
    let f = failures(
        &rt,
        r#"
        const bad = ['-', 'notdigits', '10.', '.99', '-10.', '-.99', '10-', '1-0', '1.0.0',
          '1/3', '', null, ' 1.0  ', '1.0 ', 'USD$1.0', '$1.0', { toString() { return ' 1.0'; } }];
        for (const v of bad.concat(['-1', '-1.0', '-1000.000', -10])) {
          expect('total ' + v, TypeError, () => new PaymentRequest(M, amt('USD', v)));
          expect('modifier total ' + v, TypeError, () => new PaymentRequest(M,
            { total: D.total, modifiers: [{ supportedMethods: M[0].supportedMethods,
              total: { label: '', amount: { currency: 'USD', value: v } } }] }));
        }
        for (const v of bad) {
          const item = [{ label: '', amount: { currency: 'USD', value: v } }];
          expect('displayItem ' + v, TypeError, () => new PaymentRequest(M,
            { total: D.total, displayItems: item }));
          expect('additionalDisplayItem ' + v, TypeError, () => new PaymentRequest(M,
            { total: D.total, modifiers: [{ supportedMethods: M[0].supportedMethods,
              additionalDisplayItems: item }] }));
        }
        expect('number total', null, () => new PaymentRequest(M, amt('USD', 1.0)));
        expect('high precision', null, () => new PaymentRequest(M, amt('USD', '1.00000000000000000001')));
        expect('negative displayItem', null, () => new PaymentRequest(M,
          { total: D.total, displayItems: [{ label: '', amount: { currency: 'USD', value: '-5' } }] }));
        "#,
    );
    assert_eq!(f, "");
}

#[test]
fn bug646_currency_codes() {
    let rt = runtime();
    let f = failures(
        &rt,
        r#"
        for (const c of ['BOB', 'EUR', 'usd', 'XdR', 'xTs'])
          expect('valid ' + c, null, () => new PaymentRequest(M, amt(c, '1.00')));
        for (const c of ['', '€', '$', 'SFr.', 'DM', 'KR₩', '702', 'ßP', 'ınr', '¡INVALID!']) {
          expect('total ' + c, RangeError, () => new PaymentRequest(M, amt(c, '1.00')));
          expect('displayItem ' + c, RangeError, () => new PaymentRequest(M,
            { total: D.total, displayItems: [{ label: '', amount: { currency: c, value: '1' } }] }));
          expect('shippingOption ' + c, RangeError, () => new PaymentRequest(M,
            { total: D.total, shippingOptions: [{ id: 'a', label: '',
              amount: { currency: c, value: '5.00' } }] }, { requestShipping: true }));
        }
        "#,
    );
    assert_eq!(f, "");
}

#[test]
fn bug646_payment_method_identifiers() {
    let rt = runtime();
    let f = failures(
        &rt,
        r#"
        const pr = (pmi) => new PaymentRequest([{ supportedMethods: pmi }], D);
        for (const p of ['https://wpt', 'https://pay.example/', 'https://pay.example/pr?this=is&totally',
            'https://pay.example:443/pr?a#fine', 'https://:@pay.example:443/pr#x', ' \thttps://wpt\n ',
            'https://xn--c1yn36f', 'e', 'a-b-q-n-s-pw0', 'u4-n-t', 'x-x-t-t-c34-o',
            'secure-payment-confirmation', ['secure-payment-confirmation']])
          expect('valid ' + p, null, () => pr(p));
        for (const p of ['secure-💳', 'Secure-Payment-Confirmation', '0', '-', '--', 'a--b', '-a--b',
            'a-b-', '0-a', 'A-b', 'a-0', 'a-0b', ' a-b', 'a-b\n\t', 'secure-payment-confirmation?not-really',
            'secure-payment-confirmation://not-ok', 'secure payment confirmation', ' ', 'foo,var',
            ['visa', 'mastercard'], 'https://username@example.com/pay', 'https://:password@example.com/pay',
            'http://username:password@example.com/pay', 'http://foo.com:100000000/pay',
            'not-https://pay.example/pr', '../realitive/url', '/absolute/../path?', 'https://'])
          expect('invalid ' + JSON.stringify(p), RangeError, () => pr(p));
        expect('duplicate pmi', RangeError, () => new PaymentRequest([M[0], M[0]], D));
        expect('modifier pmi', RangeError, () => new PaymentRequest(M,
          { total: D.total, modifiers: [{ supportedMethods: 'A-B' }] }));
        "#,
    );
    assert_eq!(f, "");
}

#[test]
fn bug646_method_data_serialization() {
    let rt = runtime();
    let f = failures(
        &rt,
        r#"
        const rec = {}; rec.foo = rec;
        const s = M[0].supportedMethods;
        for (const data of [[], { object: {} }])
          expect('serializable ' + JSON.stringify(data), null,
            () => new PaymentRequest([{ supportedMethods: s, data }], D));
        for (const data of [rec, 'a string', null])
          expect('method data ' + typeof data, TypeError,
            () => new PaymentRequest([{ supportedMethods: s, data }], D));
        expect('modifier data cycle', TypeError, () => new PaymentRequest(M,
          { total: D.total, modifiers: [{ supportedMethods: s, data: rec }] }));
        expect('modifier data array', null, () => new PaymentRequest(M,
          { total: D.total, modifiers: [{ supportedMethods: s, data: ['x'] }] }));
        expect('no total', TypeError, () => new PaymentRequest(M, {}));
        expect('non-iterable methodData', TypeError, () => new PaymentRequest({}, D));
        expect('call without new', TypeError, () => PaymentRequest(M, D));
        "#,
    );
    assert_eq!(f, "");
}

#[test]
fn bug646_id_and_shipping_attributes() {
    let rt = runtime();
    let f = failures(
        &rt,
        r#"
        const check = (label, got, want) => { if (got !== want) failures.push(label + ': ' + got); };
        const a = new PaymentRequest(M, D), b = new PaymentRequest(M, D);
        check('id generated', Boolean(a.id) && a.id !== b.id, true);
        check('id provided', new PaymentRequest(M, Object.assign({ id: 'my-id' }, D)).id, 'my-id');
        const opt = (id, selected) => ({ id, label: '', selected,
          amount: { currency: 'USD', value: '5.00' } });
        const det = Object.assign({}, D,
          { shippingOptions: [opt('FAIL1', true), opt('FAIL2', false), opt('the-id', true)] });
        check('no shipping requested', new PaymentRequest(M, det).shippingOption, null);
        check('last selected wins', new PaymentRequest(M, det, { requestShipping: true }).shippingOption, 'the-id');
        const dup = Object.assign({}, D, { shippingOptions: [opt('X', true), opt('X', false)] });
        check('dup ignored without shipping', new PaymentRequest(M, dup).shippingOption, null);
        expect('dup ids', TypeError, () => new PaymentRequest(M, dup, { requestShipping: true }));
        expect('bad shippingType', TypeError, () => new PaymentRequest(M, D, { shippingType: 'invalid' }));
        check('shippingAddress', a.shippingAddress, null);
        check('shippingType off', a.shippingType, null);
        check('shippingType default', new PaymentRequest(M, D, { requestShipping: true }).shippingType, 'shipping');
        check('shippingType delivery',
          new PaymentRequest(M, D, { requestShipping: true, shippingType: 'delivery' }).shippingType, 'delivery');
        "#,
    );
    assert_eq!(f, "");
}
