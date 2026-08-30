use super::super::*;

// ── Clip stack / scissor ──────────────────────────────────────────────

#[test]
fn intersect_rects_overlapping() {
    let a = Rect::new(10.0, 10.0, 50.0, 50.0);
    let b = Rect::new(30.0, 30.0, 50.0, 50.0);
    let i = intersect_rects(a, b);
    assert_eq!(i, Rect::new(30.0, 30.0, 30.0, 30.0));
}

#[test]
fn intersect_rects_b_inside_a() {
    let a = Rect::new(0.0, 0.0, 100.0, 100.0);
    let b = Rect::new(20.0, 30.0, 40.0, 50.0);
    assert_eq!(intersect_rects(a, b), b);
}

#[test]
fn intersect_rects_disjoint_returns_zero_size() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(20.0, 20.0, 10.0, 10.0);
    let i = intersect_rects(a, b);
    assert_eq!(i.width, 0.0);
    assert_eq!(i.height, 0.0);
}

#[test]
fn intersect_rects_touching_edges_returns_zero_size() {
    // Касание ребра (x=10 правая граница a == x=10 левая граница b) —
    // пересечение пустое (right strictly > left требуется).
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(10.0, 0.0, 10.0, 10.0);
    let i = intersect_rects(a, b);
    assert_eq!(i.width, 0.0);
    assert_eq!(i.height, 0.0);
}

#[test]
fn css_to_device_scissor_dpr1_exact() {
    // DPR=1, rect полностью в viewport — scissor совпадает с rect.
    let r = Rect::new(10.0, 20.0, 100.0, 50.0);
    let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
    assert_eq!(s, DeviceScissor { x: 10, y: 20, width: 100, height: 50 });
}

#[test]
fn css_to_device_scissor_dpr2_doubles() {
    // DPR=2 — все координаты × 2.
    let r = Rect::new(10.0, 20.0, 100.0, 50.0);
    let s = css_rect_to_device_scissor(r, 2.0, 2048, 1440);
    assert_eq!(s, DeviceScissor { x: 20, y: 40, width: 200, height: 100 });
}

#[test]
fn css_to_device_scissor_fractional_expands_outward() {
    // Дробные координаты: x.floor(), right.ceil() — scissor расширяется
    // наружу, чтобы не обрезать pixel-perfect содержимое внутри.
    let r = Rect::new(10.3, 20.7, 100.4, 50.1);
    let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
    // x.floor() = 10; y.floor() = 20; right.ceil() = 111; bottom.ceil() = 71.
    assert_eq!(s, DeviceScissor { x: 10, y: 20, width: 101, height: 51 });
}

#[test]
fn css_to_device_scissor_clamps_to_surface() {
    // Rect частично за пределами surface — scissor клампается.
    let r = Rect::new(900.0, 600.0, 500.0, 500.0);
    let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
    // right = 1400 → clamp to 1024; bottom = 1100 → clamp to 720.
    assert_eq!(s, DeviceScissor { x: 900, y: 600, width: 124, height: 120 });
}

#[test]
fn css_to_device_scissor_negative_origin_clamps_to_zero() {
    // Rect частично слева/сверху surface — origin клампится в 0.
    let r = Rect::new(-50.0, -30.0, 100.0, 60.0);
    let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
    // x.floor()=-50 → max(0)=0, right.ceil()=50 → 50; y similar → 30.
    assert_eq!(s, DeviceScissor { x: 0, y: 0, width: 50, height: 30 });
}

#[test]
fn css_to_device_scissor_fully_outside_is_empty() {
    // Rect полностью справа от surface.
    let r = Rect::new(1500.0, 0.0, 100.0, 50.0);
    let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
    assert!(s.is_empty());
}

#[test]
fn css_to_device_scissor_zero_rect_is_empty() {
    // Rect с нулевой шириной — пустой scissor.
    let r = Rect::new(10.0, 20.0, 0.0, 50.0);
    let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
    assert!(s.is_empty());
}

#[test]
fn device_scissor_full_covers_surface() {
    let s = DeviceScissor::full(1024, 720);
    assert_eq!(s, DeviceScissor { x: 0, y: 0, width: 1024, height: 720 });
    assert!(!s.is_empty());
}

#[test]
fn device_scissor_is_empty_detects_zero_dim() {
    assert!(DeviceScissor { x: 0, y: 0, width: 0, height: 10 }.is_empty());
    assert!(DeviceScissor { x: 0, y: 0, width: 10, height: 0 }.is_empty());
    assert!(!DeviceScissor { x: 0, y: 0, width: 1, height: 1 }.is_empty());
}

#[test]
fn sync_scissor_pushes_full_on_empty_stack() {
    let mut current: Option<DeviceScissor> = None;
    let mut ops: Vec<DrawOp> = Vec::new();
    let ok = sync_scissor_to_stack(&[], None, &mut current, &mut ops, 1.0, 1024, 720);
    assert!(ok);
    assert_eq!(ops.len(), 1);
    assert!(matches!(ops[0], DrawOp::SetScissor(s) if s == DeviceScissor::full(1024, 720)));
    assert_eq!(current, Some(DeviceScissor::full(1024, 720)));
}

#[test]
fn sync_scissor_dedupes_same_scissor() {
    // Первый вызов выставляет full; второй с тем же стеком — не пушит.
    let mut current: Option<DeviceScissor> = None;
    let mut ops: Vec<DrawOp> = Vec::new();
    sync_scissor_to_stack(&[], None, &mut current, &mut ops, 1.0, 1024, 720);
    let n_after_first = ops.len();
    sync_scissor_to_stack(&[], None, &mut current, &mut ops, 1.0, 1024, 720);
    assert_eq!(ops.len(), n_after_first, "повторный вызов не должен пушить op");
}

#[test]
fn sync_scissor_pushes_on_stack_change() {
    let mut current: Option<DeviceScissor> = None;
    let mut ops: Vec<DrawOp> = Vec::new();
    sync_scissor_to_stack(&[], None, &mut current, &mut ops, 1.0, 1024, 720);
    // Стек добавил clip — scissor сужается.
    let stack = vec![Rect::new(100.0, 100.0, 200.0, 200.0)];
    sync_scissor_to_stack(&stack, None, &mut current, &mut ops, 1.0, 1024, 720);
    assert_eq!(ops.len(), 2);
    assert!(matches!(
        ops[1],
        DrawOp::SetScissor(s) if s == DeviceScissor { x: 100, y: 100, width: 200, height: 200 }
    ));
}

#[test]
fn sync_scissor_returns_false_on_empty_scissor() {
    // Clip полностью за пределами surface — sync возвращает false,
    // caller должен пропустить draw.
    let mut current: Option<DeviceScissor> = None;
    let mut ops: Vec<DrawOp> = Vec::new();
    let stack = vec![Rect::new(2000.0, 2000.0, 100.0, 100.0)];
    let ok = sync_scissor_to_stack(&stack, None, &mut current, &mut ops, 1.0, 1024, 720);
    assert!(!ok);
}

// ── current_blend_mode ───────────────────────────────────────────────

#[test]
fn current_blend_mode_empty_stack_is_normal() {
    assert_eq!(current_blend_mode(&[]), BlendMode::Normal);
}

#[test]
fn current_blend_mode_single_push() {
    assert_eq!(current_blend_mode(&[BlendMode::Multiply]), BlendMode::Multiply);
    assert_eq!(current_blend_mode(&[BlendMode::Screen]), BlendMode::Screen);
    assert_eq!(current_blend_mode(&[BlendMode::PlusLighter]), BlendMode::PlusLighter);
}

#[test]
fn current_blend_mode_nested_returns_top() {
    // Вложенные blend-mode-ы: активен самый внутренний (топ стека).
    assert_eq!(
        current_blend_mode(&[BlendMode::Multiply, BlendMode::Screen]),
        BlendMode::Screen
    );
    assert_eq!(
        current_blend_mode(&[BlendMode::Normal, BlendMode::Overlay, BlendMode::Darken]),
        BlendMode::Darken
    );
}

#[test]
fn current_blend_mode_pop_restores_previous() {
    let mut stack = vec![BlendMode::Multiply, BlendMode::Screen];
    assert_eq!(current_blend_mode(&stack), BlendMode::Screen);
    stack.pop();
    assert_eq!(current_blend_mode(&stack), BlendMode::Multiply);
    stack.pop();
    assert_eq!(current_blend_mode(&stack), BlendMode::Normal);
}

#[test]
fn current_blend_mode_normal_on_stack_returns_normal() {
    // Явный Normal на стеке — тот же результат что и пустой стек.
    assert_eq!(current_blend_mode(&[BlendMode::Normal]), BlendMode::Normal);
}

#[test]
fn apply_alpha_to_color_identity() {
    let c = [0.2, 0.3, 0.4, 0.8];
    assert_eq!(apply_alpha_to_color(c, 1.0), c);
}

#[test]
fn apply_alpha_to_color_half() {
    // Цвет (1, 0.5, 0.25, 0.8), alpha=0.5 → alpha-канал × 0.5 = 0.4.
    let out = apply_alpha_to_color([1.0, 0.5, 0.25, 0.8], 0.5);
    assert_eq!(out, [1.0, 0.5, 0.25, 0.4]);
}

#[test]
fn apply_alpha_to_color_zero() {
    // alpha=0 → final-color.a = 0 (полностью прозрачно).
    let out = apply_alpha_to_color([1.0, 0.5, 0.25, 1.0], 0.0);
    assert_eq!(out, [1.0, 0.5, 0.25, 0.0]);
}

// dash_segments unit-тесты переехали в crate::dash_math (PA-1).

// ── emit_border_side ──────────────────────────────────────────────────

fn collect_border_fill_quads(
    side_rect: Rect,
    horizontal: bool,
    width: f32,
    style: BorderStyle,
) -> Vec<Rect> {
    let color = [1.0f32; 4];
    let mut fill_verts: Vec<FillVertex> = Vec::new();
    let mut circle_verts: Vec<CircleVertex> = Vec::new();
    emit_border_side(&mut fill_verts, &mut circle_verts, side_rect, horizontal, width, color, style);
    fill_verts
        .chunks(6)
        .map(|v| {
            let xs = v.iter().map(|p| p.pos[0]);
            let ys = v.iter().map(|p| p.pos[1]);
            let x0 = xs.clone().fold(f32::INFINITY, f32::min);
            let x1 = xs.fold(f32::NEG_INFINITY, f32::max);
            let y0 = ys.clone().fold(f32::INFINITY, f32::min);
            let y1 = ys.fold(f32::NEG_INFINITY, f32::max);
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        })
        .collect()
}

fn collect_border_circle_quads(
    side_rect: Rect,
    horizontal: bool,
    width: f32,
    style: BorderStyle,
) -> Vec<Rect> {
    let color = [1.0f32; 4];
    let mut fill_verts: Vec<FillVertex> = Vec::new();
    let mut circle_verts: Vec<CircleVertex> = Vec::new();
    emit_border_side(&mut fill_verts, &mut circle_verts, side_rect, horizontal, width, color, style);
    circle_verts
        .chunks(6)
        .map(|v| {
            let xs = v.iter().map(|p| p.pos[0]);
            let ys = v.iter().map(|p| p.pos[1]);
            let x0 = xs.clone().fold(f32::INFINITY, f32::min);
            let x1 = xs.fold(f32::NEG_INFINITY, f32::max);
            let y0 = ys.clone().fold(f32::INFINITY, f32::min);
            let y1 = ys.fold(f32::NEG_INFINITY, f32::max);
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        })
        .collect()
}

#[test]
fn emit_border_side_solid_is_single_quad() {
    let r = Rect::new(10.0, 20.0, 100.0, 6.0);
    let quads = collect_border_fill_quads(r, true, 6.0, BorderStyle::Solid);
    assert_eq!(quads.len(), 1);
    assert_eq!(quads[0], r);
}

#[test]
fn emit_border_side_dashed_produces_multiple_quads() {
    // width=4: target_dash=max(6,8)=8, target_gap=max(5,4)=5, period=13
    // side=100 → n=round(100/13)=8 segments.
    let r = Rect::new(0.0, 0.0, 100.0, 4.0);
    let quads = collect_border_fill_quads(r, true, 4.0, BorderStyle::Dashed);
    assert!(quads.len() > 1, "dashed must produce multiple segments");
    for q in &quads {
        assert_eq!(q.height, 4.0, "all segments must span full border height");
    }
}

#[test]
fn emit_border_side_dotted_circle_segments() {
    // Dotted width≥3 → SDF-circles (circle_verts), not fill quads.
    // width=4 → dot=4, period=8; side=40 → n=floor(40/8)+1=6 dots.
    // Each quad is expanded 0.5px on each side: height = 4+1 = 5.
    let r = Rect::new(0.0, 0.0, 40.0, 4.0);
    let fill_quads = collect_border_fill_quads(r, true, 4.0, BorderStyle::Dotted);
    let circle_quads = collect_border_circle_quads(r, true, 4.0, BorderStyle::Dotted);
    assert_eq!(fill_quads.len(), 0, "dotted width=4 must NOT produce fill quads");
    assert!(circle_quads.len() > 1, "dotted must produce circle quads");
    assert_eq!(circle_quads.len(), 6, "dotted: n=floor(total/period)+1=6");
    for q in &circle_quads {
        assert_eq!(q.height, 5.0, "expanded quad: dot_size + 1 = 5.0");
    }
}

#[test]
fn emit_border_side_dotted_thin_uses_fill_quads() {
    // Dotted width≤2px → fill_quad rectangles (no SDF circles), matching
    // Chrome/Edge behavior of rendering thin dotted borders as squares.
    // width=2 → dot=2, period=4; side=20 → n=floor(20/4)+1=6 quads.
    let r = Rect::new(0.0, 0.0, 20.0, 2.0);
    let fill_quads = collect_border_fill_quads(r, true, 2.0, BorderStyle::Dotted);
    let circle_quads = collect_border_circle_quads(r, true, 2.0, BorderStyle::Dotted);
    assert_eq!(circle_quads.len(), 0, "thin dotted must NOT produce circle quads");
    assert!(fill_quads.len() > 1, "thin dotted must produce fill quads");
    assert_eq!(fill_quads.len(), 6, "thin dotted: n=floor(20/4)+1=6");
}

#[test]
fn emit_border_side_double_two_quads_horizontal() {
    // width=9 → line≈3; two lines at top and bottom of the side_rect.
    let r = Rect::new(0.0, 0.0, 100.0, 9.0);
    let quads = collect_border_fill_quads(r, true, 9.0, BorderStyle::Double);
    assert_eq!(quads.len(), 2, "double = two parallel lines");
    // First line at top edge.
    assert!((quads[0].y - 0.0).abs() < 1e-3, "first line at y=0");
    // Second line at bottom edge.
    let expected_y2 = 9.0 - (9.0 / 3.0_f32).max(1.0);
    assert!((quads[1].y - expected_y2).abs() < 1e-3, "second line at bottom");
    // Both lines span full width.
    assert_eq!(quads[0].width, 100.0);
    assert_eq!(quads[1].width, 100.0);
}

#[test]
fn emit_border_side_double_thin_fallback_to_solid() {
    // width < 3 → solid fallback (no room for gap).
    let r = Rect::new(0.0, 0.0, 100.0, 2.0);
    let quads = collect_border_fill_quads(r, true, 2.0, BorderStyle::Double);
    assert_eq!(quads.len(), 1, "width<3 must fall back to single solid quad");
}

#[test]
fn emit_border_side_double_vertical() {
    // Vertical double border (left/right side).
    let r = Rect::new(0.0, 0.0, 9.0, 100.0);
    let quads = collect_border_fill_quads(r, false, 9.0, BorderStyle::Double);
    assert_eq!(quads.len(), 2, "double vertical = two parallel lines");
    assert!((quads[0].x - 0.0).abs() < 1e-3);
    let expected_x2 = 9.0 - (9.0 / 3.0_f32).max(1.0);
    assert!((quads[1].x - expected_x2).abs() < 1e-3);
    assert_eq!(quads[0].height, 100.0);
    assert_eq!(quads[1].height, 100.0);
}

#[test]
fn apply_alpha_to_color_preserves_rgb() {
    // RGB не трогается (premultiplied alpha — отдельная история; здесь
    // straight alpha с alpha-blending в pipeline).
    let out = apply_alpha_to_color([0.123, 0.456, 0.789, 1.0], 0.5);
    assert_eq!(out[0], 0.123);
    assert_eq!(out[1], 0.456);
    assert_eq!(out[2], 0.789);
    assert_eq!(out[3], 0.5);
}

#[test]
fn sync_scissor_dpr_scales_stack_rect() {
    // Стек хранится в CSS-px; sync переводит в device-px через DPR.
    let mut current: Option<DeviceScissor> = None;
    let mut ops: Vec<DrawOp> = Vec::new();
    let stack = vec![Rect::new(50.0, 50.0, 100.0, 100.0)];
    sync_scissor_to_stack(&stack, None, &mut current, &mut ops, 2.0, 2048, 1440);
    assert!(matches!(
        ops[0],
        DrawOp::SetScissor(s) if s == DeviceScissor { x: 100, y: 100, width: 200, height: 200 }
    ));
}

// ── blend_mode_to_u32 ────────────────────────────────────────────────

#[test]
fn blend_mode_to_u32_correct_values() {
    // Значения должны совпадать с маппингом в BLEND_SHADER_SRC.
    assert_eq!(blend_mode_to_u32(BlendMode::Normal),      0);
    assert_eq!(blend_mode_to_u32(BlendMode::Multiply),    1);
    assert_eq!(blend_mode_to_u32(BlendMode::Screen),      2);
    assert_eq!(blend_mode_to_u32(BlendMode::Overlay),     3);
    assert_eq!(blend_mode_to_u32(BlendMode::Darken),      4);
    assert_eq!(blend_mode_to_u32(BlendMode::Lighten),     5);
    assert_eq!(blend_mode_to_u32(BlendMode::ColorDodge),  6);
    assert_eq!(blend_mode_to_u32(BlendMode::ColorBurn),   7);
    assert_eq!(blend_mode_to_u32(BlendMode::HardLight),   8);
    assert_eq!(blend_mode_to_u32(BlendMode::SoftLight),   9);
    assert_eq!(blend_mode_to_u32(BlendMode::Difference),  10);
    assert_eq!(blend_mode_to_u32(BlendMode::Exclusion),   11);
    assert_eq!(blend_mode_to_u32(BlendMode::Hue),         12);
    assert_eq!(blend_mode_to_u32(BlendMode::Saturation),  13);
    assert_eq!(blend_mode_to_u32(BlendMode::Color),       14);
    assert_eq!(blend_mode_to_u32(BlendMode::Luminosity),  15);
    assert_eq!(blend_mode_to_u32(BlendMode::PlusLighter), 16);
}

// ── Render plan: PushBlendMode / PopBlendMode level logic ────────────

/// Симулирует логику render-planning без GPU: применяет список команд
/// к level + blend_mode стекам, проверяет итоговый уровень.
fn sim_blend_level(cmds: &[DisplayCommand]) -> (usize, Vec<BlendMode>) {
    let mut current_level: usize = 0;
    let mut blend_mode_stack: Vec<BlendMode> = Vec::new();
    let mut level_blend_mode_stack: Vec<BlendMode> = Vec::new();
    for cmd in cmds {
        match cmd {
            DisplayCommand::PushBlendMode { mode, .. } => {
                blend_mode_stack.push(*mode);
                if *mode != BlendMode::Normal {
                    level_blend_mode_stack.push(*mode);
                    current_level += 1;
                }
            }
            DisplayCommand::PopBlendMode => {
                blend_mode_stack.pop();
                if level_blend_mode_stack.pop().is_some() {
                    current_level -= 1;
                }
            }
            _ => {}
        }
    }
    (current_level, blend_mode_stack)
}

#[test]
fn push_blend_mode_normal_does_not_create_new_level() {
    // PushBlendMode { Normal } — level остаётся 0.
    let cmds = vec![
        DisplayCommand::PushBlendMode { mode: BlendMode::Normal, bounds: Rect::new(0.0, 0.0, 10.0, 10.0) },
    ];
    let (level, stack) = sim_blend_level(&cmds);
    assert_eq!(level, 0, "Normal blend mode не должен открывать offscreen level");
    assert_eq!(stack, vec![BlendMode::Normal]);
}

#[test]
fn push_blend_mode_non_normal_creates_new_level() {
    // PushBlendMode { Multiply } — level становится 1.
    let cmds = vec![
        DisplayCommand::PushBlendMode { mode: BlendMode::Multiply, bounds: Rect::new(0.0, 0.0, 10.0, 10.0) },
    ];
    let (level, _) = sim_blend_level(&cmds);
    assert_eq!(level, 1, "не-Normal blend mode должен открывать offscreen level");
}

#[test]
fn pop_blend_mode_restores_level() {
    // Push/Pop пары: level возвращается в 0.
    let cmds = vec![
        DisplayCommand::PushBlendMode { mode: BlendMode::Screen, bounds: Rect::new(0.0, 0.0, 10.0, 10.0) },
        DisplayCommand::PopBlendMode,
    ];
    let (level, stack) = sim_blend_level(&cmds);
    assert_eq!(level, 0, "после PopBlendMode level должен вернуться в 0");
    assert!(stack.is_empty(), "blend_mode_stack должен быть пуст после Pop");
}

// ── vertex transform: 2D fast path vs 3D perspective projection ────────

fn fv(x: f32, y: f32) -> FillVertex {
    FillVertex { pos: [x, y], z: 0.0, color: [0.0, 0.0, 0.0, 1.0] }
}

fn approxf(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn apply_verts_2d_affine_uses_fast_path() {
    // translate(10, 20) · scale(2, 3): точка (4, 5) → (2·4+10, 3·5+20) = (18, 35).
    let m = Mat4::translation_2d(10.0, 20.0).multiply(&Mat4::scale_2d(2.0, 3.0));
    let mut verts = [fv(4.0, 5.0)];
    apply_affine_to_verts(&mut verts, &m);
    assert!(approxf(verts[0].pos[0], 18.0));
    assert!(approxf(verts[0].pos[1], 35.0));
}

#[test]
fn apply_verts_perspective_divides_by_w() {
    // perspective(800) к вершине z=0: w' = 1, без изменений (z=0 в плоскости).
    // Но композиция perspective · translateZ сдвигает вершину по z и даёт
    // перспективное масштабирование. translateZ(+400) → точка на z=400,
    // perspective(800) → w' = 1 − 400/800 = 0.5 → x' = x/0.5 = 2x.
    let m = Mat4::perspective(800.0).multiply(&Mat4::translate_3d(0.0, 0.0, 400.0));
    assert!(!m.is_2d_affine(), "перспективная матрица не 2D affine");
    let mut verts = [fv(100.0, 50.0)];
    apply_affine_to_verts(&mut verts, &m);
    assert!(approxf(verts[0].pos[0], 200.0), "x' = {}", verts[0].pos[0]);
    assert!(approxf(verts[0].pos[1], 100.0), "y' = {}", verts[0].pos[1]);
}

#[test]
fn apply_verts_rotate_y_flattens_x() {
    // rotateY(90°): x' = cos90·x + sin90·z = 0 (z=0). Грань схлопывается по X.
    let m = Mat4::rotate_y(std::f32::consts::FRAC_PI_2);
    let mut verts = [fv(100.0, 50.0)];
    apply_affine_to_verts(&mut verts, &m);
    assert!(approxf(verts[0].pos[0], 0.0), "x' = {}", verts[0].pos[0]);
    assert!(approxf(verts[0].pos[1], 50.0), "y' = {}", verts[0].pos[1]);
}

// ── GPU depth buffer: FillVertex.z field ────────────────────────────────

#[test]
fn fill_vertex_z_default_zero() {
    // push_fill_quad creates vertices with z=0 (no transform → depth=0.5 in shader).
    let mut out = Vec::new();
    let rect = lumen_core::geom::Rect::new(0.0, 0.0, 100.0, 50.0);
    push_fill_quad(&mut out, rect, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(out.len(), 6);
    for v in &out {
        assert_eq!(v.z, 0.0, "push_fill_quad must produce z=0 vertices");
    }
}

#[test]
fn apply_verts_2d_affine_leaves_z_zero() {
    // 2D affine transform: z stays 0 (no depth change for flat 2D elements).
    let m = Mat4::translation_2d(50.0, 30.0);
    let mut verts = [fv(10.0, 20.0)];
    apply_affine_to_verts(&mut verts, &m);
    assert_eq!(verts[0].z, 0.0, "2D affine must leave z unchanged at 0.0");
}

#[test]
fn apply_verts_rotate_x_sets_depth() {
    // rotateX(90°) on a vertex at y=100, z_in=0: in CSS Y-down convention,
    // rotating +Y toward the viewer moves y=100 → z_out ≈ +100 (closer to viewer).
    // Vertex at y=0 (on the axis) stays at z=0.
    let m = Mat4::rotate_x(std::f32::consts::FRAC_PI_2);
    assert!(!m.is_2d_affine());
    let mut verts = [fv(50.0, 100.0), fv(50.0, 0.0)];
    apply_affine_to_verts(&mut verts, &m);
    // y=100 rotated about X: z_out ≈ +100 (toward viewer in CSS convention)
    assert!(verts[0].z.abs() > 50.0, "rotateX on y=100 should give |z| > 50, got {}", verts[0].z);
    // y=0 (on axis) stays at z=0
    assert!(approxf(verts[1].z, 0.0), "vertex on rotation axis stays at z=0, got {}", verts[1].z);
}

#[test]
fn apply_verts_perspective_sets_depth() {
    // perspective(800) + translateZ(400): w' = 1 - 400/800 = 0.5.
    // z_out = project_point_z(...).2 — should be non-zero, showing z is propagated.
    let m = Mat4::perspective(800.0).multiply(&Mat4::translate_3d(0.0, 0.0, 400.0));
    let mut verts = [fv(0.0, 0.0)];
    apply_affine_to_verts(&mut verts, &m);
    // With translateZ(400) and perspective(800), the z after perspective divide is
    // pz/pw where pw = 0.5 (computed from 1/d · z term). Non-zero depth expected.
    assert!(verts[0].z.abs() > 0.0, "perspective transform must propagate depth, z={}", verts[0].z);
}

#[test]
fn depth_ndc_formula_maps_correctly() {
    // Verify the NDC formula used in the shader: depth = clamp(0.5 - z/20000, 0, 1)
    // z=0 → depth=0.5 (2D elements, painter's order via LessEqual)
    // z=10000 (close) → depth=0.0 (front)
    // z=-10000 (far) → depth=1.0 (back)
    fn depth_ndc(z: f32) -> f32 { (0.5 - z / 20000.0).clamp(0.0, 1.0) }
    assert!((depth_ndc(0.0) - 0.5).abs() < 1e-6);
    assert!((depth_ndc(10000.0) - 0.0).abs() < 1e-6);
    assert!((depth_ndc(-10000.0) - 1.0).abs() < 1e-6);
    // Closer element has smaller depth → wins LessEqual test
    assert!(depth_ndc(100.0) < depth_ndc(-100.0), "closer (positive z) must have smaller NDC depth");
}

// ── GPU depth buffer: TextVertex / ImageVertex / RRectVertex.z field ────

/// `TextVertex` carries a CSS-px depth field so 3D-transformed glyph quads
/// participate in the same GPU depth test as `FillVertex` (no painter-order
/// fallback for text under preserve-3d).
#[test]
fn text_vertex_carries_depth_field() {
    let mut v = TextVertex { pos: [10.0, 20.0], z: 0.0, uv: [0.0, 0.0], color: [1.0; 4] };
    assert_eq!(v.z, 0.0, "TextVertex z initial value must be 0.0");
    // VertexPos::set_depth must write into the z field.
    v.set_depth(150.0);
    assert!(approxf(v.z, 150.0), "TextVertex set_depth must update z, got {}", v.z);
    // Struct stride matches the wgpu vertex attribute layout
    // (pos 8 + z 4 + uv 8 + color 16 = 36 bytes).
    assert_eq!(std::mem::size_of::<TextVertex>(), 36);
}

/// `ImageVertex` carries a CSS-px depth field so 3D-transformed `<img>`
/// quads occlude correctly against background rects.
#[test]
fn image_vertex_carries_depth_field() {
    let mut v = ImageVertex { pos: [5.0, 7.0], z: 0.0, uv: [1.0, 1.0], alpha: 1.0 };
    assert_eq!(v.z, 0.0, "ImageVertex z initial value must be 0.0");
    v.set_depth(-300.0);
    assert!(approxf(v.z, -300.0), "ImageVertex set_depth must update z, got {}", v.z);
    // Struct stride matches wgpu attribute layout
    // (pos 8 + z 4 + uv 8 + alpha 4 = 24 bytes).
    assert_eq!(std::mem::size_of::<ImageVertex>(), 24);
}

/// `RRectVertex` (SDF rounded-rect) carries a CSS-px depth field so border-
/// radius backgrounds participate in cross-type depth testing.
#[test]
fn rrect_vertex_carries_depth_field() {
    let mut v = RRectVertex {
        pos: [0.0, 0.0],
        z: 0.0,
        color: [0.0, 0.0, 0.0, 1.0],
        center: [50.0, 50.0],
        half_size: [50.0, 50.0],
        radii_x: [10.0; 4],
        radii_y: [10.0; 4],
    };
    assert_eq!(v.z, 0.0, "RRectVertex z initial value must be 0.0");
    v.set_depth(42.0);
    assert!(approxf(v.z, 42.0), "RRectVertex set_depth must update z, got {}", v.z);
    // Stride matches wgpu attribute layout
    // (pos 8 + z 4 + color 16 + center 8 + half_size 8 + radii_x 16 + radii_y 16 = 76 bytes).
    assert_eq!(std::mem::size_of::<RRectVertex>(), 76);
}

/// Constructors emit z=0 for all 6 quad vertices — equivalent to the 2D
/// painter's-order path (depth=0.5 in shader); 3D transforms override later
/// via `apply_affine_to_verts` / `apply_affine_to_rrect_verts`.
#[test]
fn push_image_quad_emits_zero_depth() {
    let mut out = Vec::new();
    let rect = lumen_core::geom::Rect::new(0.0, 0.0, 100.0, 50.0);
    push_image_quad(&mut out, rect, [0.0, 0.0], [1.0, 1.0], 1.0);
    assert_eq!(out.len(), 6);
    for v in &out {
        assert_eq!(v.z, 0.0, "push_image_quad must produce z=0 vertices");
    }
}

/// BUG-277 срез 15 — квад `DrawCrossFade` обязан проходить через
/// накопленный `PushTransform`, как и остальные image-пути. Пока
/// `CrossFadeVertex` не реализовывал `VertexPos`, применить матрицу было
/// нечем, и картинка ложилась в нетрансформированных координатах (под
/// живым хромом — на `CHROME_H` px выше своего бокса).
#[test]
fn cross_fade_quad_follows_accumulated_transform() {
    let mut out = Vec::new();
    let rect = lumen_core::geom::Rect::new(10.0, 20.0, 100.0, 50.0);
    push_cross_fade_quad(&mut out, rect);
    assert_eq!(out.len(), 6);
    let m = Mat4::translate_3d(0.0, 69.0, 0.0);
    apply_affine_to_verts(&mut out, &m);
    for v in &out {
        assert!(
            v.pos[1] >= 89.0 - 1e-4,
            "квад cross-fade должен сдвинуться на 69px вниз, got y={}",
            v.pos[1]
        );
    }
    assert!(
        out.iter().any(|v| approxf(v.pos[0], 10.0)),
        "translate по Y не должен трогать X"
    );
}

/// `push_rrect_quad` similarly emits z=0 for all 6 vertices.
#[test]
fn push_rrect_quad_emits_zero_depth() {
    let mut out = Vec::new();
    let rect = lumen_core::geom::Rect::new(0.0, 0.0, 100.0, 50.0);
    let radii = CornerRadii {
        tl: 8.0, tr: 8.0, br: 8.0, bl: 8.0,
        tl_y: 8.0, tr_y: 8.0, br_y: 8.0, bl_y: 8.0,
    };
    push_rrect_quad(&mut out, rect, [1.0, 0.0, 0.0, 1.0], radii);
    assert_eq!(out.len(), 6);
    for v in &out {
        assert_eq!(v.z, 0.0, "push_rrect_quad must produce z=0 vertices");
    }
}

/// BUG-277 срез 4 — `push_rrect_quad` уносит в шейдер радиусы, уже
/// уменьшенные единым коэффициентом CSS Backgrounds L3 §5.5
/// (`clamped_to_box`), а не по-осевым `min(r, half)`. Для «пилюли»
/// (`border-radius: 999px` на 300×140) это разница между стадионом с
/// круглыми торцами r=70 и сплошным эллипсом 150×70.
#[test]
fn push_rrect_quad_applies_css_overlap_clamp() {
    let mut out = Vec::new();
    let rect = lumen_core::geom::Rect::new(0.0, 0.0, 300.0, 140.0);
    let radii = CornerRadii {
        tl: 999.0, tr: 999.0, br: 999.0, bl: 999.0,
        tl_y: 999.0, tr_y: 999.0, br_y: 999.0, bl_y: 999.0,
    };
    push_rrect_quad(&mut out, rect, [1.0, 0.0, 0.0, 1.0], radii);
    for v in &out {
        for (axis, got) in [("x", v.radii_x), ("y", v.radii_y)] {
            for r in got {
                assert!(
                    (r - 70.0).abs() < 1e-4,
                    "radii_{axis} = {r}, ожидалось 70 (§5.5: 140/2), \
                     иначе торцы пилюли станут эллиптическими"
                );
            }
        }
    }
}

/// BUG-277 срез 5 — quad скруглённого клипа кладёт NDC/UV/мировую позицию
/// в одну и ту же точку экрана: uv промахнётся мимо текселя уровня, если
/// конвертации разъедутся (уровень рисуется тем же viewport-маппингом).
#[test]
fn push_rrect_clip_quad_maps_ndc_uv_and_world() {
    let mut out = Vec::new();
    let rect = lumen_core::geom::Rect::new(100.0, 50.0, 200.0, 100.0);
    let radii = CornerRadii {
        tl: 10.0, tr: 10.0, br: 10.0, bl: 10.0,
        tl_y: 10.0, tr_y: 10.0, br_y: 10.0, bl_y: 10.0,
    };
    push_rrect_clip_quad(&mut out, rect, radii, 1000.0, 500.0);
    assert_eq!(out.len(), 6);
    for v in &out {
        let [wx, wy] = v.world_pos;
        assert!((v.uv[0] - wx / 1000.0).abs() < 1e-6, "uv.x != world.x/vw");
        assert!((v.uv[1] - wy / 500.0).abs() < 1e-6, "uv.y != world.y/vh");
        assert!((v.pos[0] - (v.uv[0] * 2.0 - 1.0)).abs() < 1e-6, "ndc.x != uv.x*2-1");
        assert!((v.pos[1] - (1.0 - v.uv[1] * 2.0)).abs() < 1e-6, "ndc.y != 1-uv.y*2");
        assert_eq!(v.center, [200.0, 100.0]);
        assert_eq!(v.half_size, [100.0, 50.0]);
    }
}

/// BUG-277 срез 8 — форма `clip-path` едет в шейдер в ЭКРАННЫХ px:
/// контент уровня уже трансформирован по вершинам, поэтому нетронутая
/// page-форма обрезала бы его по чужому месту (класс BUG-276/срез 6/7).
#[test]
fn path_clip_params_transforms_polygon_verts_to_screen() {
    let shape = ResolvedClipShape::Polygon {
        verts: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 20.0)],
        even_odd: true,
    };
    let m = Mat4::translation_2d(100.0, 50.0);
    let p = path_clip_params(&shape, Some(&m)).expect("2D-аффинный трансформ");
    assert_eq!(p.header[0], 1, "вид формы = полигон");
    assert_eq!(p.header[1], 3, "три вершины");
    assert_eq!(p.header[2], 1, "even-odd fill rule");
    // Две точки на vec4: (v0.x, v0.y, v1.x, v1.y), (v2.x, v2.y, _, _).
    assert_eq!(p.verts[0], [100.0, 50.0, 110.0, 50.0]);
    assert_eq!(p.verts[1][0], 110.0);
    assert_eq!(p.verts[1][1], 70.0);
}

/// Круг под поворотом остаётся кругом, но его центр едет вместе с боксом
/// (TEST-109 c0). Проверка через контракт шейдера: `length(inv_m·p) == 1`
/// ровно на контуре фигуры в экранном пространстве.
#[test]
fn path_clip_params_maps_rotated_circle_contour_to_unit_length() {
    let shape = ResolvedClipShape::Circle { cx: 40.0, cy: 40.0, r: 20.0 };
    let m = Mat4::rotate_2d(std::f32::consts::FRAC_PI_4);
    let p = path_clip_params(&shape, Some(&m)).expect("аффинный трансформ");
    assert_eq!(p.header[0], 0, "вид формы = эллипс");
    let (ecx, ecy) = m.transform_point_2d(40.0, 40.0);
    assert!((p.center[0] - ecx).abs() < 1e-4 && (p.center[1] - ecy).abs() < 1e-4);
    // Восемь точек контура в page-пространстве → экран → unit-space.
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let (sx, sy) = m.transform_point_2d(40.0 + 20.0 * a.cos(), 40.0 + 20.0 * a.sin());
        let (dx, dy) = (sx - p.center[0], sy - p.center[1]);
        let qx = p.inv_m[0] * dx + p.inv_m[1] * dy;
        let qy = p.inv_m[2] * dx + p.inv_m[3] * dy;
        let len = (qx * qx + qy * qy).sqrt();
        assert!((len - 1.0).abs() < 1e-3, "точка контура дала length = {len}");
    }
}

/// BUG-277 срез 13: покрытие включается только там, где кромка квада
/// действительно сошла с осей растровой сетки. Осевые матрицы обязаны
/// остаться на прежнем (побитово идентичном) пути, иначе каждый обычный
/// фон платил бы за CPU-растеризацию без единого изменённого пикселя.
#[test]
fn rotates_axes_2d_only_for_rotate_and_skew() {
    assert!(!rotates_axes_2d(&Mat4::IDENTITY));
    assert!(!rotates_axes_2d(&Mat4::translation_2d(10.0, -20.0)));
    assert!(!rotates_axes_2d(&Mat4::scale_2d(2.0, 3.0)));
    assert!(!rotates_axes_2d(&Mat4::scale_2d(-1.0, 1.0)), "flip остаётся осевым");
    assert!(rotates_axes_2d(&Mat4::rotate_2d(std::f32::consts::FRAC_PI_4)));
    assert!(rotates_axes_2d(&Mat4::skew_x(0.3)));
    assert!(rotates_axes_2d(&Mat4::skew_y(0.3)));
    // Поворот на кратное 90° меняет оси местами, но кромки остаются на сетке
    // — b/c при этом ненулевые, поэтому предикат срабатывает. Это осознанно
    // безопасная сторона ошибки: лишняя растеризация даёт cov=1 и тот же
    // пиксель, тогда как пропуск настоящего поворота вернул бы ступеньку.
    assert!(rotates_axes_2d(&Mat4::rotate_2d(std::f32::consts::FRAC_PI_2)));
    // 3D/перспектива исключены: antialias_fill_soup не переносит z.
    assert!(!rotates_axes_2d(&Mat4::rotate_y(0.5)));
    assert!(!rotates_axes_2d(&Mat4::perspective(800.0).multiply(&Mat4::rotate_x(0.5))));
}

/// Растеризация обязана идти в device px, а результат — вернуться в CSS px:
/// при `dpr = 2` квад, лежащий ровно на пиксельной сетке, обязан остаться
/// полностью непрозрачным и не сдвинуться (иначе AA-путь двигал бы обычную
/// геометрию на повёрнутых страницах).
#[test]
fn antialias_fill_soup_roundtrips_device_px_and_keeps_aligned_quad_opaque() {
    let color = [0.2, 0.4, 0.6, 1.0];
    let mut verts = Vec::new();
    push_fill_quad(&mut verts, Rect::new(4.0, 6.0, 10.0, 8.0), color);
    antialias_fill_soup(&mut verts, 0, color, 2.0, None);
    assert!(!verts.is_empty(), "квад не должен исчезнуть");
    for v in &verts {
        assert!((v.color[3] - 1.0).abs() < 1e-3, "alpha = {}", v.color[3]);
        assert_eq!([v.color[0], v.color[1], v.color[2]], [color[0], color[1], color[2]]);
        assert!(
            (4.0..=14.0).contains(&v.pos[0]) && (6.0..=14.0).contains(&v.pos[1]),
            "вершина ушла за исходный прямоугольник: {:?}",
            v.pos
        );
    }
}

/// BUG-405 срез 9: кэш покрытия обязан возвращать РОВНО те же вершины, что
/// и пересчёт, — и на промахе, и на попадании.
///
/// Гейты плечами живут в `headless_tests` и требуют GPU, поэтому помечены
/// `#[ignore]`; этот тест идёт в обычном прогоне и держит то же
/// утверждение на уровне самой мемоизируемой функции.
///
/// Контроль на ложно-зелёный: СДВИНУТАЯ фигура обязана дать промах, иначе
/// кэш, отвечающий готовым на любой запрос, был бы тут зелен.
#[test]
fn coverage_cache_returns_identical_vertices() {
    let color = [1.0, 0.0, 0.0, 1.0];
    let soup = |dx: f32| {
        let mut v = Vec::new();
        push_fill_quad(&mut v, Rect::new(dx, 0.0, 20.0, 20.0), color);
        apply_affine_to_verts(&mut v, &Mat4::rotate_2d(std::f32::consts::FRAC_PI_4));
        v
    };
    let aa = |cache: Option<&mut CoverageCache>, dx: f32| {
        let mut v = soup(dx);
        antialias_fill_soup(&mut v, 0, color, 1.0, cache);
        v
    };

    let plain = aa(None, 0.0);
    let mut cache = CoverageCache::default();
    let miss = aa(Some(&mut cache), 0.0);
    assert_eq!((cache.hits, cache.misses), (0, 1), "первый вызов не мог попасть");
    let hit = aa(Some(&mut cache), 0.0);
    assert_eq!((cache.hits, cache.misses), (1, 1), "повторный вызов не попал в кэш");

    assert!(!plain.is_empty(), "покрытие пусто — сравнивать нечего");
    let bits = |v: &[FillVertex]| -> Vec<[u32; 3]> {
        v.iter().map(|x| [x.pos[0].to_bits(), x.pos[1].to_bits(), x.color[3].to_bits()]).collect()
    };
    assert_eq!(bits(&plain), bits(&miss), "промах разошёлся с пересчётом");
    assert_eq!(bits(&plain), bits(&hit), "попадание разошлось с пересчётом");

    let shifted = aa(Some(&mut cache), 3.5);
    assert_eq!(cache.misses, 2, "сдвинутая фигура взята из кэша — ключ не различает формы");
    assert_ne!(bits(&plain), bits(&shifted), "сдвиг не изменил покрытие — контроль негоден");
}

/// BUG-405 срез 12: фигура, взятая из кэша, обязана дать те же вершины,
/// что пересчёт, и ключ обязан различать всё, от чего вершины зависят.
///
/// Утверждение проверяется на уровне самой мемоизируемой цепочки
/// (тесселяция → сдвиг → матрица → сглаживание), а не только на пикселях:
/// пиксельный тест не увидел бы расхождения в вершинах, накрытых одним и
/// тем же пикселем.
///
/// Контроли на ложно-зелёный: сдвиг, смена параметров обводки и та же
/// геометрия под заливкой вместо обводки обязаны дать промах — кэш,
/// отвечающий готовым на любой запрос, был бы тут зелен.
#[test]
fn svg_shape_cache_returns_identical_vertices() {
    let contours = vec![vec![[2.0_f32, 3.0], [18.0, 7.0], [10.0, 19.0]]];
    let params = crate::svg_path::StrokeParams { half_width: 1.5, ..Default::default() };
    let color = [0.2_f32, 0.4, 0.6, 0.8];
    let run = |shapes: &mut SvgShapeCache,
               on: bool,
               dx: f32,
               p: Option<&crate::svg_path::StrokeParams>| {
        let mut cov = CoverageCache::default();
        let shape =
            svg_shape_verts(shapes, &mut cov, on, true, &contours, p, dx, 0.0, None, 1.0, false);
        let mut v = Vec::new();
        emit_svg_shape(&mut v, &shape, color, false);
        v
    };
    let bits = |v: &[FillVertex]| -> Vec<[u32; 4]> {
        v.iter()
            .map(|x| [x.pos[0].to_bits(), x.pos[1].to_bits(), x.z.to_bits(), x.color[3].to_bits()])
            .collect()
    };

    let mut shapes = SvgShapeCache::default();
    let plain = run(&mut shapes, false, 0.0, Some(&params));
    assert_eq!((shapes.hits, shapes.misses), (0, 0), "плечо отката трогало кэш");
    assert!(!plain.is_empty(), "обводка не дала вершин — сравнивать нечего");

    let miss = run(&mut shapes, true, 0.0, Some(&params));
    assert_eq!((shapes.hits, shapes.misses), (0, 1), "первый вызов не мог попасть");
    let hit = run(&mut shapes, true, 0.0, Some(&params));
    assert_eq!((shapes.hits, shapes.misses), (1, 1), "повторный вызов не попал в кэш");
    assert_eq!(bits(&plain), bits(&miss), "промах разошёлся с пересчётом");
    assert_eq!(bits(&plain), bits(&hit), "попадание разошлось с пересчётом");

    let shifted = run(&mut shapes, true, 3.5, Some(&params));
    assert_eq!(shapes.misses, 2, "сдвинутая фигура взята из кэша — ключ не видит сдвига");
    assert_ne!(bits(&plain), bits(&shifted), "сдвиг не изменил вершины — контроль негоден");

    let thick = crate::svg_path::StrokeParams { half_width: 4.0, ..params.clone() };
    let wide = run(&mut shapes, true, 0.0, Some(&thick));
    assert_eq!(shapes.misses, 3, "другая толщина взята из кэша — ключ не видит параметров");
    assert_ne!(bits(&plain), bits(&wide), "толщина не изменила вершины — контроль негоден");

    let filled = run(&mut shapes, true, 0.0, None);
    assert_eq!(shapes.misses, 4, "заливка взята из кэша обводки — ключ не видит вида команды");
    assert_ne!(bits(&plain), bits(&filled), "заливка совпала с обводкой — контроль негоден");
}

/// А кромка, сошедшая с сетки, обязана дать дробное покрытие — ради этого
/// срез и делается. Ромб (квад под 45°) не может состоять из одних
/// полностью залитых пикселей.
#[test]
fn antialias_fill_soup_gives_fractional_alpha_on_rotated_quad() {
    let color = [1.0, 0.0, 0.0, 1.0];
    let mut verts = Vec::new();
    push_fill_quad(&mut verts, Rect::new(0.0, 0.0, 20.0, 20.0), color);
    apply_affine_to_verts(&mut verts, &Mat4::rotate_2d(std::f32::consts::FRAC_PI_4));
    antialias_fill_soup(&mut verts, 0, color, 1.0, None);
    assert!(
        verts.iter().any(|v| v.color[3] > 0.01 && v.color[3] < 0.99),
        "ни одного частично покрытого пикселя на кромке ромба"
    );
    assert!(
        verts.iter().any(|v| v.color[3] > 0.99),
        "внутренность ромба обязана остаться сплошной"
    );
}

/// BUG-277 срез 14: осевой трансформ (или его отсутствие) обязан остаться
/// на дешёвом scissor-пути — там AABB и есть сам клип, а точный контур
/// стоил бы offscreen-уровень и composite-пасс на каждый `overflow: hidden`
/// корпуса без единого изменённого пикселя.
#[test]
fn rotated_rect_clip_params_only_for_rotate_and_skew() {
    let r = Rect::new(10.0, 20.0, 100.0, 50.0);
    assert!(rotated_rect_clip_params(r, None).is_none(), "трансформа нет");
    assert!(rotated_rect_clip_params(r, Some(&Mat4::IDENTITY)).is_none());
    assert!(rotated_rect_clip_params(r, Some(&Mat4::translation_2d(5.0, -7.0))).is_none());
    assert!(rotated_rect_clip_params(r, Some(&Mat4::scale_2d(2.0, 3.0))).is_none());
    // 3D/перспектива: контур в экранном пространстве — уже не этот полигон.
    assert!(rotated_rect_clip_params(r, Some(&Mat4::rotate_y(0.5))).is_none());
    // Вырожденный прямоугольник ничего осмысленного не клиппит.
    let rot = Mat4::rotate_2d(std::f32::consts::FRAC_PI_4);
    assert!(rotated_rect_clip_params(Rect::new(0.0, 0.0, 0.0, 50.0), Some(&rot)).is_none());
    assert!(rotated_rect_clip_params(r, Some(&rot)).is_some(), "поворот");
    assert!(rotated_rect_clip_params(r, Some(&Mat4::skew_x(0.3))).is_some(), "скос");
}

/// А под поворотом контур обязан быть настоящим квадрилатералом бокса, а не
/// его описанной рамкой: именно в зазор между ними протекал ребёнок
/// (TEST-100 c5, `overflow: hidden` на повёрнутом контейнере).
#[test]
fn rotated_rect_clip_params_maps_corners_not_bbox() {
    let r = Rect::new(0.0, 0.0, 300.0, 300.0);
    let m = Mat4::rotate_2d(std::f32::consts::FRAC_PI_4);
    let p = rotated_rect_clip_params(r, Some(&m)).expect("поворот даёт точный контур");
    assert_eq!(p.header, [1, 4, 0, 0], "полигон, 4 вершины, nonzero");
    // Две точки на vec4: (v0, v1), (v2, v3).
    let got = [
        (p.verts[0][0], p.verts[0][1]),
        (p.verts[0][2], p.verts[0][3]),
        (p.verts[1][0], p.verts[1][1]),
        (p.verts[1][2], p.verts[1][3]),
    ];
    for (i, (x, y)) in [(0.0, 0.0), (300.0, 0.0), (300.0, 300.0), (0.0, 300.0)]
        .into_iter()
        .enumerate()
    {
        let (ex, ey) = m.transform_point_2d(x, y);
        assert!(
            (got[i].0 - ex).abs() < 1e-3 && (got[i].1 - ey).abs() < 1e-3,
            "вершина {i}: {:?}, ожидалось ({ex}, {ey})",
            got[i]
        );
    }
    // Ключевое отличие от прежнего поведения: AABB строго шире контура.
    let bbox = apply_transform_to_clip(r, Some(&m));
    assert!(bbox.width > 300.0 * 1.4, "AABB ромба шире стороны: {}", bbox.width);
}

/// Неравномерный масштаб превращает круг в эллипс — шейдеру нужна полная
/// обратная матрица, а не пара радиусов (TEST-109 c1 масштабирует бокс).
#[test]
fn path_clip_params_handles_non_uniform_scale() {
    let shape = ResolvedClipShape::Ellipse { cx: 0.0, cy: 0.0, rx: 10.0, ry: 5.0 };
    let m = Mat4::scale_2d(2.0, 3.0);
    let p = path_clip_params(&shape, Some(&m)).expect("аффинный трансформ");
    // Полуоси в экране: 20 по X, 15 по Y → inv_m = diag(1/20, 1/15).
    assert!((p.inv_m[0] - 0.05).abs() < 1e-6, "inv_m[0] = {}", p.inv_m[0]);
    assert!((p.inv_m[3] - 1.0 / 15.0).abs() < 1e-6, "inv_m[3] = {}", p.inv_m[3]);
    assert_eq!([p.inv_m[1], p.inv_m[2]], [0.0, 0.0]);
}

/// Фолбэк на исторический bbox-клип (BUG-140) обязан срабатывать там, где
/// точная форма не представима: 3D-трансформ, вырожденная фигура и
/// полигон длиннее `PATH_CLIP_MAX_VERTS` (иначе — цикл на каждый пиксель).
#[test]
fn path_clip_params_falls_back_on_unsupported_shapes() {
    let circle = ResolvedClipShape::Circle { cx: 0.0, cy: 0.0, r: 10.0 };
    let persp = Mat4::perspective(500.0).multiply(&Mat4::rotate_y(0.5));
    assert!(path_clip_params(&circle, Some(&persp)).is_none(), "3D-трансформ");
    let zero = ResolvedClipShape::Circle { cx: 0.0, cy: 0.0, r: 0.0 };
    assert!(path_clip_params(&zero, None).is_none(), "нулевой радиус");
    let two = ResolvedClipShape::Polygon { verts: vec![(0.0, 0.0), (1.0, 1.0)], even_odd: false };
    assert!(path_clip_params(&two, None).is_none(), "меньше трёх вершин");
    let long = ResolvedClipShape::Polygon {
        verts: (0..PATH_CLIP_MAX_VERTS + 1).map(|i| (i as f32, 0.0)).collect(),
        even_odd: false,
    };
    assert!(path_clip_params(&long, None).is_none(), "длиннее лимита uniform-а");
}

/// BUG-277 срез 5 — контур клипа обязан пройти тот же §5.5-клэмп, что и
/// заливка бокса: иначе `border-radius: 999px` обрежет детей эллипсом,
/// а сам бокс останется стадионом (расхождение маски и фона).
#[test]
fn push_rrect_clip_quad_applies_css_overlap_clamp() {
    let mut out = Vec::new();
    let rect = lumen_core::geom::Rect::new(0.0, 0.0, 300.0, 140.0);
    let radii = CornerRadii {
        tl: 999.0, tr: 999.0, br: 999.0, bl: 999.0,
        tl_y: 999.0, tr_y: 999.0, br_y: 999.0, bl_y: 999.0,
    };
    push_rrect_clip_quad(&mut out, rect, radii, 1024.0, 720.0);
    for v in &out {
        for r in v.radii_x.iter().chain(v.radii_y.iter()) {
            assert!((r - 70.0).abs() < 1e-4, "radius = {r}, ожидалось 70 (§5.5)");
        }
    }
}

/// BUG-277 срез 5 — скруглённый клип открывает offscreen-уровень только
/// когда контур остаётся rounded-rect в экранных координатах: поворот и
/// скос уводят его в произвольную форму, там остаётся bbox-фолбэк.
#[test]
fn axis_aligned_scale_rejects_rotation_and_keeps_scale() {
    assert_eq!(axis_aligned_scale(None), Some((1.0, 1.0)));
    let t = Mat4::translate_3d(10.0, 20.0, 0.0);
    assert_eq!(axis_aligned_scale(Some(&t)), Some((1.0, 1.0)));
    let s = Mat4::scale_2d(2.0, 3.0);
    let (sx, sy) = axis_aligned_scale(Some(&s)).expect("scale is axis-aligned");
    assert!((sx - 2.0).abs() < 1e-6 && (sy - 3.0).abs() < 1e-6);
    let r = Mat4::rotate_z(std::f32::consts::FRAC_PI_4);
    assert_eq!(axis_aligned_scale(Some(&r)), None, "поворот — не axis-aligned");
    let r3 = Mat4::rotate_x(std::f32::consts::FRAC_PI_2);
    assert_eq!(axis_aligned_scale(Some(&r3)), None, "3D — не 2D-аффинное");
}

/// `apply_affine_to_rrect_verts` propagates projected z through the 3D
/// path (`Mat4::project_point_z`) so border-radius backgrounds get correct
/// depth values when transformed.
#[test]
fn apply_rrect_affine_3d_sets_depth() {
    // rotateX(90°) on a vertex at y=100 should produce non-zero projected z.
    let m = Mat4::rotate_x(std::f32::consts::FRAC_PI_2);
    assert!(!m.is_2d_affine());
    let mut verts = vec![RRectVertex {
        pos: [50.0, 100.0],
        z: 0.0,
        color: [1.0; 4],
        center: [50.0, 100.0],
        half_size: [50.0, 100.0],
        radii_x: [0.0; 4],
        radii_y: [0.0; 4],
    }];
    apply_affine_to_rrect_verts(&mut verts, &m);
    // Same orientation as `apply_verts_rotate_x_sets_depth`: |z| should grow.
    assert!(verts[0].z.abs() > 50.0,
        "rotateX on y=100 must produce |z|>50 in RRectVertex, got {}", verts[0].z);
}

/// 2D affine on `apply_affine_to_rrect_verts` must leave z untouched
/// (fast path identical to the pre-depth pipeline).
#[test]
fn apply_rrect_affine_2d_leaves_z_zero() {
    let m = Mat4::translation_2d(20.0, 30.0);
    let mut verts = vec![RRectVertex {
        pos: [0.0, 0.0],
        z: 0.0,
        color: [1.0; 4],
        center: [50.0, 50.0],
        half_size: [50.0, 50.0],
        radii_x: [0.0; 4],
        radii_y: [0.0; 4],
    }];
    apply_affine_to_rrect_verts(&mut verts, &m);
    assert_eq!(verts[0].z, 0.0, "2D affine on rrect must leave z unchanged");
}

/// `apply_affine_to_verts` (the generic path used by Text/Image) propagates
/// projected depth via `VertexPos::set_depth` into TextVertex/ImageVertex.
#[test]
fn apply_verts_3d_sets_text_and_image_depth() {
    let m = Mat4::rotate_x(std::f32::consts::FRAC_PI_2);
    let mut text = [TextVertex { pos: [50.0, 100.0], z: 0.0, uv: [0.0, 0.0], color: [1.0; 4] }];
    apply_affine_to_verts(&mut text, &m);
    assert!(text[0].z.abs() > 50.0,
        "rotateX must propagate depth into TextVertex.z, got {}", text[0].z);
    let mut image = [ImageVertex { pos: [50.0, 100.0], z: 0.0, uv: [0.0, 0.0], alpha: 1.0 }];
    apply_affine_to_verts(&mut image, &m);
    assert!(image[0].z.abs() > 50.0,
        "rotateX must propagate depth into ImageVertex.z, got {}", image[0].z);
}

