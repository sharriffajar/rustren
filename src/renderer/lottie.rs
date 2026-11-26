use serde_json::Value;
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

/// Pure-Rust CPU Lottie (Bodymovin JSON) vector renderer on `tiny_skia`.
/// Supports:
/// - Composition `w`, `h`, `fr`, `ip`, `op`
/// - Shape layers (`ty: 4`), Solid layers (`ty: 1`), Null layers (`ty: 3`), Precomp layers (`ty: 0`)
/// - Layer parenting (`ind` / `parent`), `ip`/`op` visibility, `ks` transform (`a`, `p`, `s`, `r`, `o`)
/// - Static & keyframed properties with cubic-bezier easing & hold frames
/// - Shapes: `gr` (group + `tr`), `rc` (rect), `el` (ellipse), `sr` (star/polygon), `sh` (bezier path + morph), `fl` (fill), `st` (stroke)
pub fn render_lottie_json(
    pixmap: &mut Pixmap,
    json_bytes: &[u8],
    time_sec: f32,
    loop_animation: bool,
    speed: f32,
    rx: f32,
    ry: f32,
    w: f32,
    h: f32,
    rot_deg: f32,
    opacity: f32,
) -> bool {
    let Ok(doc) = serde_json::from_slice::<Value>(json_bytes) else {
        return false;
    };
    let comp_w = doc.get("w").and_then(|v| v.as_f64()).unwrap_or(512.0) as f32;
    let comp_h = doc.get("h").and_then(|v| v.as_f64()).unwrap_or(512.0) as f32;
    let fr = doc.get("fr").and_then(|v| v.as_f64()).unwrap_or(30.0) as f32;
    let ip = doc.get("ip").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let op = doc.get("op").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;

    if comp_w <= 0.0 || comp_h <= 0.0 || op <= ip {
        return false;
    }

    let dur_frames = (op - ip).max(1.0);
    let raw_frame = time_sec.max(0.0) * speed.max(0.0) * fr;
    let frame = if loop_animation {
        ip + (raw_frame % dur_frames)
    } else {
        (ip + raw_frame).min(op - 0.001)
    };

    let sx = w / comp_w;
    let sy = h / comp_h;
    let base_tf = Transform::from_scale(sx, sy)
        .post_concat(Transform::from_translate(rx, ry))
        .post_concat(Transform::from_rotate_at(rot_deg, rx + w * 0.5, ry + h * 0.5));

    let assets = doc.get("assets").and_then(|v| v.as_array());
    if let Some(layers) = doc.get("layers").and_then(|v| v.as_array()) {
        render_layer_list(pixmap, layers, assets, frame, base_tf, opacity.clamp(0.0, 1.0));
        return true;
    }
    false
}

fn render_layer_list(
    pixmap: &mut Pixmap,
    layers: &[Value],
    assets: Option<&Vec<Value>>,
    frame: f32,
    parent_tf: Transform,
    parent_opacity: f32,
) {
    // Lottie layers are ordered top-to-bottom, so render in reverse (bottom-to-top).
    for layer in layers.iter().rev() {
        if layer.get("hd").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let ip = layer.get("ip").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let op = layer.get("op").and_then(|v| v.as_f64()).unwrap_or(1e9) as f32;
        if frame < ip || frame >= op {
            continue;
        }

        let (layer_tf, layer_op) = resolve_layer_transform(layer, layers, frame, parent_tf);
        let final_op = (parent_opacity * layer_op).clamp(0.0, 1.0);
        if final_op <= 0.001 {
            continue;
        }

        let ty = layer.get("ty").and_then(|v| v.as_i64()).unwrap_or(-1);
        match ty {
            1 => {
                // Solid layer
                let sw = layer.get("sw").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let sh = layer.get("sh").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                if sw > 0.0 && sh > 0.0 {
                    let hex = layer.get("sc").and_then(|v| v.as_str()).unwrap_or("#ffffff");
                    if let Some((r, g, b)) = parse_hex_rgb(hex) {
                        if let Some(rect) = Rect::from_xywh(0.0, 0.0, sw, sh) {
                            let mut pb = PathBuilder::new();
                            pb.push_rect(rect);
                            if let Some(path) = pb.finish() {
                                let mut paint = Paint::default();
                                paint.set_color(Color::from_rgba(r, g, b, final_op).unwrap_or(Color::WHITE));
                                paint.anti_alias = true;
                                pixmap.fill_path(&path, &paint, FillRule::Winding, layer_tf, None);
                            }
                        }
                    }
                }
            }
            4 => {
                // Shape layer
                if let Some(shapes) = layer.get("shapes").and_then(|v| v.as_array()) {
                    render_shape_group(pixmap, shapes, frame, layer_tf, final_op);
                }
            }
            0 => {
                // Precomp layer
                if let (Some(ref_id), Some(asset_list)) = (
                    layer.get("refId").and_then(|v| v.as_str()),
                    assets,
                ) {
                    let st = layer.get("st").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    let sr = layer.get("sr").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                    let sub_frame = (frame - st) / sr.max(0.001);
                    if let Some(comp) = asset_list
                        .iter()
                        .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(ref_id))
                    {
                        if let Some(sub_layers) = comp.get("layers").and_then(|v| v.as_array()) {
                            render_layer_list(pixmap, sub_layers, assets, sub_frame, layer_tf, final_op);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn resolve_layer_transform(
    layer: &Value,
    all_layers: &[Value],
    frame: f32,
    root_tf: Transform,
) -> (Transform, f32) {
    let mut chain = vec![layer];
    let mut cur = layer;
    for _ in 0..16 {
        let Some(parent_ind) = cur.get("parent").and_then(|v| v.as_i64()) else {
            break;
        };
        if let Some(p) = all_layers
            .iter()
            .find(|l| l.get("ind").and_then(|v| v.as_i64()) == Some(parent_ind))
        {
            chain.push(p);
            cur = p;
        } else {
            break;
        }
    }

    let mut tf = root_tf;
    for node in chain.iter().rev() {
        if let Some(ks) = node.get("ks") {
            let (local_tf, _) = eval_ks(ks, frame);
            tf = local_tf.post_concat(tf);
        }
    }
    let own_op = layer
        .get("ks")
        .map(|ks| eval_ks(ks, frame).1)
        .unwrap_or(1.0);
    (tf, own_op)
}

fn eval_ks(ks: &Value, frame: f32) -> (Transform, f32) {
    let a = ks.get("a").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let p = ks.get("p").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let s = ks.get("s").map(|v| eval_vec2(v, frame, [100.0, 100.0])).unwrap_or([100.0, 100.0]);
    let r = ks.get("r").map(|v| eval_scalar(v, frame, 0.0)).unwrap_or(0.0);
    let o = ks.get("o").map(|v| eval_scalar(v, frame, 100.0)).unwrap_or(100.0) / 100.0;

    let tf = Transform::from_translate(-a[0], -a[1])
        .post_concat(Transform::from_scale(s[0] / 100.0, s[1] / 100.0))
        .post_concat(Transform::from_rotate(r))
        .post_concat(Transform::from_translate(p[0], p[1]));
    (tf, o.clamp(0.0, 1.0))
}

fn render_shape_group(
    pixmap: &mut Pixmap,
    items: &[Value],
    frame: f32,
    parent_tf: Transform,
    parent_op: f32,
) {
    // Check for group transform (`tr`) in this item list
    let mut tf = parent_tf;
    let mut group_op = parent_op;
    for item in items {
        if item.get("ty").and_then(|v| v.as_str()) == Some("tr") {
            let (tr_tf, tr_op) = eval_ks(item, frame);
            tf = tr_tf.post_concat(tf);
            group_op = (group_op * tr_op).clamp(0.0, 1.0);
        }
    }

    let mut paths: Vec<Path> = Vec::new();
    for item in items {
        if item.get("hd").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let ty = item.get("ty").and_then(|v| v.as_str()).unwrap_or("");
        match ty {
            "gr" => {
                if let Some(sub) = item.get("it").and_then(|v| v.as_array()) {
                    render_shape_group(pixmap, sub, frame, tf, group_op);
                }
            }
            "rc" => {
                if let Some(path) = build_rect_path(item, frame) {
                    paths.push(path);
                }
            }
            "el" => {
                if let Some(path) = build_ellipse_path(item, frame) {
                    paths.push(path);
                }
            }
            "sr" => {
                if let Some(path) = build_polystar_path(item, frame) {
                    paths.push(path);
                }
            }
            "sh" => {
                if let Some(path) = build_bezier_path(item, frame) {
                    paths.push(path);
                }
            }
            "fl" => {
                let c = item.get("c").map(|v| eval_color(v, frame)).unwrap_or([1.0, 1.0, 1.0, 1.0]);
                let o = item.get("o").map(|v| eval_scalar(v, frame, 100.0)).unwrap_or(100.0) / 100.0;
                let alpha = (c[3] * o * group_op).clamp(0.0, 1.0);
                if alpha > 0.001 {
                    let mut paint = Paint::default();
                    paint.set_color(Color::from_rgba(c[0], c[1], c[2], alpha).unwrap_or(Color::WHITE));
                    paint.anti_alias = true;
                    let rule = if item.get("r").and_then(|v| v.as_i64()) == Some(2) {
                        FillRule::EvenOdd
                    } else {
                        FillRule::Winding
                    };
                    for path in &paths {
                        pixmap.fill_path(path, &paint, rule, tf, None);
                    }
                }
            }
            "st" => {
                let c = item.get("c").map(|v| eval_color(v, frame)).unwrap_or([1.0, 1.0, 1.0, 1.0]);
                let o = item.get("o").map(|v| eval_scalar(v, frame, 100.0)).unwrap_or(100.0) / 100.0;
                let sw = item.get("w").map(|v| eval_scalar(v, frame, 2.0)).unwrap_or(2.0);
                let alpha = (c[3] * o * group_op).clamp(0.0, 1.0);
                if alpha > 0.001 && sw > 0.0 {
                    let mut paint = Paint::default();
                    paint.set_color(Color::from_rgba(c[0], c[1], c[2], alpha).unwrap_or(Color::WHITE));
                    paint.anti_alias = true;
                    let mut stroke = Stroke {
                        width: sw,
                        line_cap: match item.get("lc").and_then(|v| v.as_i64()) {
                            Some(2) => LineCap::Round,
                            Some(3) => LineCap::Square,
                            _ => LineCap::Butt,
                        },
                        line_join: match item.get("lj").and_then(|v| v.as_i64()) {
                            Some(2) => LineJoin::Round,
                            Some(3) => LineJoin::Bevel,
                            _ => LineJoin::Miter,
                        },
                        ..Stroke::default()
                    };
                    if let Some(ml) = item.get("ml").and_then(|v| v.as_f64()) {
                        stroke.miter_limit = ml as f32;
                    }
                    for path in &paths {
                        pixmap.stroke_path(path, &paint, &stroke, tf, None);
                    }
                }
            }
            _ => {}
        }
    }
}

fn build_rect_path(item: &Value, frame: f32) -> Option<Path> {
    let p = item.get("p").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let s = item.get("s").map(|v| eval_vec2(v, frame, [100.0, 100.0])).unwrap_or([100.0, 100.0]);
    let r = item.get("r").map(|v| eval_scalar(v, frame, 0.0)).unwrap_or(0.0).max(0.0);
    let w = s[0].abs();
    let h = s[1].abs();
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let x = p[0] - w * 0.5;
    let y = p[1] - h * 0.5;
    let rad = r.min(w * 0.5).min(h * 0.5);
    let mut pb = PathBuilder::new();
    if rad <= 0.01 {
        pb.push_rect(Rect::from_xywh(x, y, w, h)?);
    } else {
        let k = rad * 0.55228475;
        pb.move_to(x + rad, y);
        pb.line_to(x + w - rad, y);
        pb.cubic_to(x + w - rad + k, y, x + w, y + rad - k, x + w, y + rad);
        pb.line_to(x + w, y + h - rad);
        pb.cubic_to(x + w, y + h - rad + k, x + w - rad + k, y + h, x + w - rad, y + h);
        pb.line_to(x + rad, y + h);
        pb.cubic_to(x + rad - k, y + h, x, y + h - rad + k, x, y + h - rad);
        pb.line_to(x, y + rad);
        pb.cubic_to(x, y + rad - k, x + rad - k, y, x + rad, y);
        pb.close();
    }
    pb.finish()
}

fn build_ellipse_path(item: &Value, frame: f32) -> Option<Path> {
    let p = item.get("p").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let s = item.get("s").map(|v| eval_vec2(v, frame, [100.0, 100.0])).unwrap_or([100.0, 100.0]);
    let rx = s[0].abs() * 0.5;
    let ry = s[1].abs() * 0.5;
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let kx = rx * 0.55228475;
    let ky = ry * 0.55228475;
    let cx = p[0];
    let cy = p[1];
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - ry);
    pb.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
    pb.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
    pb.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
    pb.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
    pb.close();
    pb.finish()
}

fn build_polystar_path(item: &Value, frame: f32) -> Option<Path> {
    let p = item.get("p").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let pts = item.get("pt").map(|v| eval_scalar(v, frame, 5.0)).unwrap_or(5.0).round().max(3.0) as usize;
    let or = item.get("or").map(|v| eval_scalar(v, frame, 50.0)).unwrap_or(50.0);
    let ir = item.get("ir").map(|v| eval_scalar(v, frame, 25.0)).unwrap_or(25.0);
    let rot = item.get("r").map(|v| eval_scalar(v, frame, 0.0)).unwrap_or(0.0).to_radians();
    let is_star = item.get("sy").and_then(|v| v.as_i64()).unwrap_or(1) == 1;
    let total = if is_star { pts * 2 } else { pts };
    let mut pb = PathBuilder::new();
    for i in 0..total {
        let ang = rot - std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::TAU / (total as f32);
        let rad = if is_star && i % 2 == 1 { ir } else { or };
        let px = p[0] + rad * ang.cos();
        let py = p[1] + rad * ang.sin();
        if i == 0 {
            pb.move_to(px, py);
        } else {
            pb.line_to(px, py);
        }
    }
    pb.close();
    pb.finish()
}

fn build_bezier_path(item: &Value, frame: f32) -> Option<Path> {
    let ks = item.get("ks")?;
    let shape_val = eval_shape_value(ks, frame)?;
    let v_arr = shape_val.get("v")?.as_array()?;
    let i_arr = shape_val.get("i")?.as_array()?;
    let o_arr = shape_val.get("o")?.as_array()?;
    let closed = shape_val.get("c").and_then(|v| v.as_bool()).unwrap_or(false);
    let n = v_arr.len();
    if n == 0 || i_arr.len() < n || o_arr.len() < n {
        return None;
    }
    let pt = |arr: &[Value], idx: usize| -> [f32; 2] {
        let a = arr[idx].as_array();
        [
            a.and_then(|x| x.first()).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            a.and_then(|x| x.get(1)).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        ]
    };
    let mut pb = PathBuilder::new();
    let p0 = pt(v_arr, 0);
    pb.move_to(p0[0], p0[1]);
    let segs = if closed { n } else { n.saturating_sub(1) };
    for idx in 0..segs {
        let next = (idx + 1) % n;
        let a = pt(v_arr, idx);
        let oa = pt(o_arr, idx);
        let b = pt(v_arr, next);
        let ib = pt(i_arr, next);
        pb.cubic_to(a[0] + oa[0], a[1] + oa[1], b[0] + ib[0], b[1] + ib[1], b[0], b[1]);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

fn eval_shape_value(prop: &Value, frame: f32) -> Option<Value> {
    let animated = prop.get("a").and_then(|v| v.as_i64()).unwrap_or(0) == 1;
    let k = prop.get("k")?;
    if !animated {
        return Some(k.clone());
    }
    let kfs = k.as_array()?;
    if kfs.is_empty() {
        return None;
    }
    let mut chosen = &kfs[0];
    for kf in kfs {
        let t = kf.get("t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        if frame >= t {
            chosen = kf;
        }
    }
    chosen
        .get("s")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .cloned()
}

fn eval_scalar(prop: &Value, frame: f32, default: f32) -> f32 {
    eval_vec_n(prop, frame, &[default])[0]
}

fn eval_vec2(prop: &Value, frame: f32, default: [f32; 2]) -> [f32; 2] {
    let v = eval_vec_n(prop, frame, &default);
    [v[0], v.get(1).copied().unwrap_or(default[1])]
}

fn eval_color(prop: &Value, frame: f32) -> [f32; 4] {
    let v = eval_vec_n(prop, frame, &[1.0, 1.0, 1.0, 1.0]);
    let mut r = v[0];
    let mut g = v.get(1).copied().unwrap_or(1.0);
    let mut b = v.get(2).copied().unwrap_or(1.0);
    let a = v.get(3).copied().unwrap_or(1.0);
    if r > 1.0 || g > 1.0 || b > 1.0 {
        r /= 255.0;
        g /= 255.0;
        b /= 255.0;
    }
    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a.clamp(0.0, 1.0)]
}

fn eval_vec_n(prop: &Value, frame: f32, default: &[f32]) -> Vec<f32> {
    let animated = prop.get("a").and_then(|v| v.as_i64()).unwrap_or(0) == 1;
    let Some(k) = prop.get("k") else {
        return default.to_vec();
    };
    if !animated {
        return value_to_floats(k, default);
    }
    let Some(kfs) = k.as_array() else {
        return default.to_vec();
    };
    if kfs.is_empty() {
        return default.to_vec();
    }
    let t0 = kfs[0].get("t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    if frame <= t0 {
        return kfs[0]
            .get("s")
            .map(|v| value_to_floats(v, default))
            .unwrap_or_else(|| default.to_vec());
    }
    for i in 0..kfs.len() {
        let cur = &kfs[i];
        let t_cur = cur.get("t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        if i + 1 >= kfs.len() {
            return cur
                .get("s")
                .map(|v| value_to_floats(v, default))
                .unwrap_or_else(|| default.to_vec());
        }
        let next = &kfs[i + 1];
        let t_next = next.get("t").and_then(|v| v.as_f64()).unwrap_or(t_cur as f64) as f32;
        if frame >= t_cur && frame <= t_next {
            let s_vals = cur
                .get("s")
                .map(|v| value_to_floats(v, default))
                .unwrap_or_else(|| default.to_vec());
            if cur.get("h").and_then(|v| v.as_i64()) == Some(1) || t_next <= t_cur {
                return s_vals;
            }
            let e_vals = cur
                .get("e")
                .or_else(|| next.get("s"))
                .map(|v| value_to_floats(v, &s_vals))
                .unwrap_or_else(|| s_vals.clone());
            let raw_t = ((frame - t_cur) / (t_next - t_cur)).clamp(0.0, 1.0);
            let eased = eval_lottie_bezier(cur, raw_t);
            let len = s_vals.len().max(e_vals.len());
            return (0..len)
                .map(|idx| {
                    let a = s_vals.get(idx).copied().unwrap_or(0.0);
                    let b = e_vals.get(idx).copied().unwrap_or(a);
                    a + (b - a) * eased
                })
                .collect();
        }
    }
    default.to_vec()
}

fn value_to_floats(v: &Value, default: &[f32]) -> Vec<f32> {
    if let Some(n) = v.as_f64() {
        return vec![n as f32];
    }
    if let Some(arr) = v.as_array() {
        let out: Vec<f32> = arr
            .iter()
            .filter_map(|x| x.as_f64().map(|f| f as f32))
            .collect();
        if !out.is_empty() {
            return out;
        }
    }
    default.to_vec()
}

fn eval_lottie_bezier(kf: &Value, t: f32) -> f32 {
    let ox = kf.pointer("/o/x").and_then(first_f32).unwrap_or(0.0);
    let oy = kf.pointer("/o/y").and_then(first_f32).unwrap_or(0.0);
    let ix = kf.pointer("/i/x").and_then(first_f32).unwrap_or(1.0);
    let iy = kf.pointer("/i/y").and_then(first_f32).unwrap_or(1.0);
    if (ox - oy).abs() < 1e-4 && (ix - iy).abs() < 1e-4 {
        return t;
    }
    // Solve cubic bezier x(u) = t via Newton-Raphson / binary search
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    let mut u = t;
    for _ in 0..8 {
        let x = cubic(u, ox, ix);
        if (x - t).abs() < 1e-4 {
            break;
        }
        if x < t {
            lo = u;
        } else {
            hi = u;
        }
        u = 0.5 * (lo + hi);
    }
    cubic(u, oy, iy)
}

fn cubic(u: f32, p1: f32, p2: f32) -> f32 {
    let inv = 1.0 - u;
    3.0 * inv * inv * u * p1 + 3.0 * inv * u * u * p2 + u * u * u
}

fn first_f32(v: &Value) -> Option<f32> {
    v.as_f64()
        .map(|f| f as f32)
        .or_else(|| v.as_array()?.first()?.as_f64().map(|f| f as f32))
}

fn parse_hex_rgb(hex: &str) -> Option<(f32, f32, f32)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 {
        let r = u8::from_str_radix(&h[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&h[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&h[4..6], 16).ok()? as f32 / 255.0;
        return Some((r, g, b));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_lottie_animated_circle_moves_pixels() {
        let lottie_json = r#"{
            "v": "5.7.4", "fr": 30, "ip": 0, "op": 60, "w": 200, "h": 200,
            "layers": [{
                "ty": 4, "ind": 1, "ip": 0, "op": 60,
                "ks": {
                    "o": { "a": 0, "k": 100 },
                    "r": { "a": 0, "k": 0 },
                    "p": {
                        "a": 1,
                        "k": [
                            { "t": 0, "s": [40, 100], "e": [160, 100] },
                            { "t": 30, "s": [160, 100] }
                        ]
                    },
                    "a": { "a": 0, "k": [0, 0] },
                    "s": { "a": 0, "k": [100, 100] }
                },
                "shapes": [{
                    "ty": "gr",
                    "it": [
                        { "ty": "el", "p": { "a": 0, "k": [0, 0] }, "s": { "a": 0, "k": [50, 50] } },
                        { "ty": "fl", "c": { "a": 0, "k": [0.97, 0.45, 0.08, 1] }, "o": { "a": 0, "k": 100 } }
                    ]
                }]
            }]
        }"#;

        let mut pm0 = Pixmap::new(200, 200).unwrap();
        assert!(render_lottie_json(&mut pm0, lottie_json.as_bytes(), 0.0, true, 1.0, 0.0, 0.0, 200.0, 200.0, 0.0, 1.0));
        let mut pm1 = Pixmap::new(200, 200).unwrap();
        assert!(render_lottie_json(&mut pm1, lottie_json.as_bytes(), 1.0, true, 1.0, 0.0, 0.0, 200.0, 200.0, 0.0, 1.0));

        // Pixel at (40, 100) should be orange at t=0 and transparent at t=1
        assert!(pm0.pixel(40, 100).unwrap().alpha() > 200);
        assert_eq!(pm1.pixel(40, 100).unwrap().alpha(), 0);
        // Pixel at (160, 100) should be orange at t=1
        assert!(pm1.pixel(160, 100).unwrap().alpha() > 200);
    }
}
