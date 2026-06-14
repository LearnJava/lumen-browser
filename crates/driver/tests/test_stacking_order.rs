//! Тест stacking context paint order (CSS 2.1 Appendix E).
//!
//! Проверяет, что display_list_for_compare() использует корректный порядок
//! отрисовки: negative-z SC перед in-flow content, positive-z SC после.
//! До PH1-6 драйвер использовал DOM-order (build_display_list); после — правильный
//! stacking order (build_display_list_ordered через StackingTree + PaintOrder).

use lumen_driver::InProcessSession;
use lumen_paint::DisplayCommand;

/// Создаёт сессию с тестовой страницей:
///   .neg  — position:absolute, z-index:-1, background:red
///   .base — position:relative, background:green (in-flow блок в root SC)
///   .pos  — position:absolute, z-index:1, background:blue
///
/// Правильный CSS 2.1 Appendix E порядок:
///   [body bg] → [red/neg z:-1] → [green/base content] → [blue/pos z:1]
fn make_session() -> InProcessSession {
    let html = r#"<!DOCTYPE html>
<html>
<head>
<style>
  html, body { margin: 0; padding: 0; background: white; }
  .base { position: relative; width: 200px; height: 200px; background: #00ff00; }
  .neg  { position: absolute; z-index: -1; width: 100px; height: 100px;
          background: #ff0000; top: 0; left: 0; }
  .pos  { position: absolute; z-index:  1; width: 100px; height: 100px;
          background: #0000ff; top: 50px; left: 50px; }
</style>
</head>
<body>
  <div class="base">
    <div class="neg"></div>
    <div class="pos"></div>
  </div>
</body>
</html>"#;
    let mut s = InProcessSession::new();
    s.navigate_html(html).expect("navigate_html");
    s
}

/// Извлечь позицию первого FillRect данного цвета (r,g,b) в display list.
fn first_fill_idx(dl: &[DisplayCommand], r: u8, g: u8, b: u8) -> Option<usize> {
    dl.iter().position(|cmd| {
        if let DisplayCommand::FillRect { color, .. } = cmd {
            color.r == r && color.g == g && color.b == b
        } else {
            false
        }
    })
}

#[test]
fn negative_z_paints_before_parent_content() {
    let s = make_session();
    let dl = s.display_list_for_compare().expect("display_list_for_compare");

    let red_idx  = first_fill_idx(&dl, 0xff, 0x00, 0x00)
        .expect("red FillRect (.neg z:-1) not found in display list");
    let green_idx = first_fill_idx(&dl, 0x00, 0xff, 0x00)
        .expect("green FillRect (.base) not found in display list");

    assert!(
        red_idx < green_idx,
        "z-index:-1 (red) должен быть нарисован ДО in-flow parent (green): \
         red at {red_idx}, green at {green_idx}"
    );
}

#[test]
fn positive_z_paints_after_parent_content() {
    let s = make_session();
    let dl = s.display_list_for_compare().expect("display_list_for_compare");

    let green_idx = first_fill_idx(&dl, 0x00, 0xff, 0x00)
        .expect("green FillRect (.base) not found in display list");
    let blue_idx  = first_fill_idx(&dl, 0x00, 0x00, 0xff)
        .expect("blue FillRect (.pos z:1) not found in display list");

    assert!(
        blue_idx > green_idx,
        "z-index:1 (blue) должен быть нарисован ПОСЛЕ in-flow parent (green): \
         green at {green_idx}, blue at {blue_idx}"
    );
}

#[test]
fn stacking_order_negative_before_positive() {
    let s = make_session();
    let dl = s.display_list_for_compare().expect("display_list_for_compare");

    let red_idx  = first_fill_idx(&dl, 0xff, 0x00, 0x00)
        .expect("red FillRect not found");
    let blue_idx = first_fill_idx(&dl, 0x00, 0x00, 0xff)
        .expect("blue FillRect not found");

    assert!(
        red_idx < blue_idx,
        "z-index:-1 (red) должен быть нарисован ДО z-index:1 (blue): \
         red at {red_idx}, blue at {blue_idx}"
    );
}
