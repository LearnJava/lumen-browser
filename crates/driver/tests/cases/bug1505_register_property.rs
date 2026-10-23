//! BUG-1505 — `CSS.registerProperty()` feeds the cascade like `@property`: initial value in
//! `getComputedStyle`, computed form of lengths and numbers, `inherits`, re-registration,
//! `CSS.unregisterProperty`.

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><body><div id="p" style="font-size:10px"><div id="t">x</div></div></body></html>"#;

fn ev(s: &mut InProcessSession, js: &str) -> String {
    s.eval(js).expect("eval").trim_matches('"').to_owned()
}

fn session() -> InProcessSession {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    s
}

fn gcs(s: &mut InProcessSession, id: &str, name: &str) -> String {
    ev(s, &format!("getComputedStyle(document.getElementById('{id}')).getPropertyValue('{name}')"))
}

#[test]
fn initial_value_is_visible_for_both_inherits_modes() {
    let mut s = session();
    ev(&mut s, "CSS.registerProperty({name:'--ri',syntax:'<length>',inherits:false,initialValue:'100px'}); \
                CSS.registerProperty({name:'--rt',syntax:'<length>',inherits:true,initialValue:'100px'}); 'ok'");
    assert_eq!(gcs(&mut s, "t", "--ri"), "100px");
    assert_eq!(gcs(&mut s, "t", "--rt"), "100px");
}

#[test]
fn non_inheriting_registration_resets_in_the_child() {
    let mut s = session();
    ev(&mut s, "CSS.registerProperty({name:'--ni',syntax:'<length>',inherits:false,initialValue:'1px'}); \
                CSS.registerProperty({name:'--yi',syntax:'<length>',inherits:true,initialValue:'1px'}); \
                document.getElementById('p').style.cssText='font-size:10px;--ni:5px;--yi:5px'; 'ok'");
    assert_eq!(gcs(&mut s, "p", "--ni"), "5px");
    assert_eq!(gcs(&mut s, "t", "--ni"), "1px");
    assert_eq!(gcs(&mut s, "t", "--yi"), "5px");
}

#[test]
fn lengths_and_numbers_are_computed() {
    let mut s = session();
    ev(&mut s, "CSS.registerProperty({name:'--len',syntax:'<length>',inherits:false,initialValue:'0px'}); \
                CSS.registerProperty({name:'--num',syntax:'<number>',inherits:false,initialValue:'0'}); \
                CSS.registerProperty({name:'--lp',syntax:'<length-percentage>',inherits:false,initialValue:'0px'}); \
                CSS.registerProperty({name:'--list',syntax:'<length>#',inherits:false,initialValue:'0px'}); \
                document.getElementById('t').style.cssText='--len:2em;--num:calc(1 + 2);--lp:calc(19em - 2%);--list:10px, 3em'; 'ok'");
    assert_eq!(gcs(&mut s, "t", "--len"), "20px");
    assert_eq!(gcs(&mut s, "t", "--num"), "3");
    assert_eq!(gcs(&mut s, "t", "--lp"), "calc(-2% + 190px)");
    assert_eq!(gcs(&mut s, "t", "--list"), "10px, 30px");
}

#[test]
fn value_not_matching_the_syntax_is_ignored() {
    let mut s = session();
    ev(&mut s, "CSS.registerProperty({name:'--ro',syntax:'<length>',inherits:false,initialValue:'7px'}); \
                document.getElementById('t').style.setProperty('--ro','red'); 'ok'");
    assert_eq!(gcs(&mut s, "t", "--ro"), "7px");
}

#[test]
fn re_registration_throws_and_unregister_removes() {
    let mut s = session();
    let r = ev(
        &mut s,
        "CSS.registerProperty({name:'--twice',syntax:'<length>',inherits:false,initialValue:'3px'}); \
         var n; try { CSS.registerProperty({name:'--twice',syntax:'<length>',inherits:false,initialValue:'3px'}); n='none'; } \
         catch (e) { n = e.name; } n",
    );
    assert_eq!(r, "InvalidModificationError");
    assert_eq!(gcs(&mut s, "t", "--twice"), "3px");
    assert_eq!(ev(&mut s, "typeof CSS.unregisterProperty"), "function");
    ev(&mut s, "CSS.unregisterProperty('--twice'); 'ok'");
    assert_eq!(gcs(&mut s, "t", "--twice"), "");
    let again = ev(&mut s, "var n; try { CSS.unregisterProperty('--twice'); n='none'; } catch (e) { n = e.name; } n");
    assert_eq!(again, "NotFoundError");
}

#[test]
fn var_cycle_makes_every_member_invalid() {
    let mut s = InProcessSession::new();
    s.navigate_html(
        r#"<html><head><style>
#q{--ca:var(--cb,hello);--cb:var(--ca,world);--cc:var(--ca,fb);--cd:var(--cb)}
</style></head><body><div id=q></div></body></html>"#,
    )
    .expect("navigate_html");
    ev(&mut s, "CSS.registerProperty({name:'--ca',syntax:'*',inherits:true}); CSS.registerProperty({name:'--cb',syntax:'*',inherits:true}); 'ok'");
    let r = ev(&mut s, "var cs=getComputedStyle(document.getElementById('q')); ['--ca','--cb','--cc','--cd'].map(function(x){return cs.getPropertyValue(x)}).join('|')");
    assert_eq!(r, "||fb|");
}

#[test]
fn single_keyframe_starts_from_the_underlying_value() {
    let mut s = session();
    let r = ev(
        &mut s,
        "CSS.registerProperty({name:'--sk',syntax:'<length>',inherits:false,initialValue:'100px'}); var t=document.getElementById('t');          var a=t.animate({'--sk':'200px'},{duration:1000,iterations:3}); a.pause(); a.currentTime=2500;          getComputedStyle(t).getPropertyValue('--sk')",
    );
    assert_eq!(r, "150px");
}
