use crate::schema::{AnimProperty, AnimValue, Animation, Easing, RepeatMode, RepeatSpec, SpringConfig};

pub fn eval_spring_physics(elapsed_sec: f32, cfg: SpringConfig) -> f32 {
    let t = elapsed_sec.max(0.0);
    let k = cfg.stiffness.max(1e-3);
    let c = cfg.damping.max(0.0);
    let m = cfg.mass.max(1e-3);

    let omega0 = (k / m).sqrt();
    let zeta = c / (2.0 * (k * m).sqrt());

    if zeta < 1.0 - 1e-4 {
        let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
        let exp = (-zeta * omega0 * t).exp();
        1.0 - exp * ((omega_d * t).cos() + (zeta / (1.0 - zeta * zeta).sqrt()) * (omega_d * t).sin())
    } else if (zeta - 1.0).abs() <= 1e-4 {
        let exp = (-omega0 * t).exp();
        1.0 - exp * (1.0 + omega0 * t)
    } else {
        let gamma = omega0 * (zeta * zeta - 1.0).sqrt();
        let exp = (-zeta * omega0 * t).exp();
        1.0 - exp * ((gamma * t).cosh() + (zeta / (zeta * zeta - 1.0).sqrt()) * (gamma * t).sinh())
    }
}

pub fn eval_cubic_bezier(t: f32, p: [f32; 4]) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let [x1, y1, x2, y2] = p;
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;

    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;

    let sample_x = |u: f32| ((ax * u + bx) * u + cx) * u;
    let sample_dx = |u: f32| (3.0 * ax * u + 2.0 * bx) * u + cx;
    let sample_y = |u: f32| ((ay * u + by) * u + cy) * u;

    // Solve x(u) = t with Newton-Raphson, falling back to bisection.
    let mut u = t;
    for _ in 0..8 {
        let err = sample_x(u) - t;
        if err.abs() < 1e-6 {
            return sample_y(u);
        }
        let dx = sample_dx(u);
        if dx.abs() < 1e-6 {
            break;
        }
        u = (u - err / dx).clamp(0.0, 1.0);
    }

    let mut lo = 0.0_f32;
    let mut hi = 1.0_f32;
    u = t;
    for _ in 0..16 {
        let x = sample_x(u);
        if (x - t).abs() < 1e-6 {
            break;
        }
        if t > x {
            lo = u;
        } else {
            hi = u;
        }
        u = 0.5 * (lo + hi);
    }
    sample_y(u)
}

pub fn apply_easing(t: f32, easing: Easing) -> f32 {
    apply_easing_with_bezier(t, easing, None)
}

pub fn apply_easing_with_bezier(t: f32, easing: Easing, bezier: Option<[f32; 4]>) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match easing {
        Easing::Linear => t,
        Easing::EaseIn => t * t * t,
        Easing::EaseOut => 1.0 - (1.0 - t).powi(3),
        Easing::EaseInOut => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
        Easing::EaseInQuad => t * t,
        Easing::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
        Easing::EaseInOutQuad => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            }
        }
        Easing::EaseInExpo => {
            if t == 0.0 {
                0.0
            } else {
                2.0_f32.powf(10.0 * t - 10.0)
            }
        }
        Easing::EaseOutExpo => {
            if t == 1.0 {
                1.0
            } else {
                1.0 - 2.0_f32.powf(-10.0 * t)
            }
        }
        Easing::EaseInOutExpo => {
            if t == 0.0 {
                0.0
            } else if t == 1.0 {
                1.0
            } else if t < 0.5 {
                2.0_f32.powf(20.0 * t - 10.0) / 2.0
            } else {
                (2.0 - 2.0_f32.powf(-20.0 * t + 10.0)) / 2.0
            }
        }
        Easing::EaseInBack => {
            const C1: f32 = 1.70158;
            const C3: f32 = C1 + 1.0;
            C3 * t * t * t - C1 * t * t
        }
        Easing::EaseOutBack => {
            const C1: f32 = 1.70158;
            const C3: f32 = C1 + 1.0;
            1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
        }
        Easing::EaseInOutBack => {
            const C1: f32 = 1.70158;
            const C2: f32 = C1 * 1.525;
            if t < 0.5 {
                ((2.0 * t).powi(2) * ((C2 + 1.0) * 2.0 * t - C2)) / 2.0
            } else {
                ((2.0 * t - 2.0).powi(2) * ((C2 + 1.0) * (t * 2.0 - 2.0) + C2) + 2.0) / 2.0
            }
        }
        Easing::EaseOutBounce => {
            const N1: f32 = 7.5625;
            const D1: f32 = 2.75;
            if t < 1.0 / D1 {
                N1 * t * t
            } else if t < 2.0 / D1 {
                let u = t - 1.5 / D1;
                N1 * u * u + 0.75
            } else if t < 2.5 / D1 {
                let u = t - 2.25 / D1;
                N1 * u * u + 0.9375
            } else {
                let u = t - 2.625 / D1;
                N1 * u * u + 0.984375
            }
        }
        Easing::CubicBezier => eval_cubic_bezier(t, bezier.unwrap_or([0.25, 0.1, 0.25, 1.0])),
        Easing::Spring => {
            let decay = (-6.0 * t).exp();
            let osc = (t * 12.0).cos();
            1.0 - decay * osc
        }
    }
}

enum AnimStep {
    FinishedFrom,
    FinishedTo,
    Interpolate(f32),
}

fn eval_anim_step(anim: &Animation, time_sec: f32) -> AnimStep {
    let cycle_dur = (anim.end_sec - anim.start_sec).max(f32::EPSILON);
    let (is_finished, finished_is_from, cycle_t) = match anim.repeat {
        None => {
            if time_sec >= anim.end_sec {
                (true, false, 1.0)
            } else {
                (false, false, (time_sec - anim.start_sec) / cycle_dur)
            }
        }
        Some(RepeatSpec::Count(0)) => (true, true, 0.0),
        Some(RepeatSpec::Count(n)) => {
            let total_end = anim.start_sec + cycle_dur * (n as f32);
            if time_sec >= total_end {
                (true, anim.yoyo && n % 2 == 0, 1.0)
            } else {
                let elapsed = time_sec - anim.start_sec;
                let cycle_idx = (elapsed / cycle_dur).floor() as u32;
                let mut ct = (elapsed / cycle_dur) - (cycle_idx as f32);
                if anim.yoyo && cycle_idx % 2 == 1 {
                    ct = 1.0 - ct;
                }
                (false, false, ct)
            }
        }
        Some(RepeatSpec::Mode(RepeatMode::Infinite)) => {
            let elapsed = time_sec - anim.start_sec;
            let cycle_idx = (elapsed / cycle_dur).floor() as u32;
            let mut ct = (elapsed / cycle_dur) - (cycle_idx as f32);
            if anim.yoyo && cycle_idx % 2 == 1 {
                ct = 1.0 - ct;
            }
            (false, false, ct)
        }
    };

    if is_finished {
        if finished_is_from {
            AnimStep::FinishedFrom
        } else {
            AnimStep::FinishedTo
        }
    } else {
        let eased_t = match (anim.easing, anim.spring) {
            (Easing::Spring, Some(cfg)) => eval_spring_physics(cycle_t * cycle_dur, cfg),
            _ => apply_easing_with_bezier(cycle_t, anim.easing, anim.bezier),
        };
        AnimStep::Interpolate(eased_t)
    }
}

fn eval_keyframes_step(kfs: &[crate::schema::Keyframe], time_sec: f32) -> Option<(&crate::schema::Keyframe, &crate::schema::Keyframe, f32)> {
    let first = kfs.first()?;
    if kfs.len() == 1 || time_sec <= first.time {
        return Some((first, first, 0.0));
    }
    for win in kfs.windows(2) {
        let (k0, k1) = (&win[0], &win[1]);
        if time_sec <= k1.time {
            let dur = (k1.time - k0.time).max(1e-6);
            let raw_t = ((time_sec - k0.time) / dur).clamp(0.0, 1.0);
            let eased_t = apply_easing_with_bezier(raw_t, k1.easing, k1.bezier);
            return Some((k0, k1, eased_t));
        }
    }
    let last = kfs.last()?;
    Some((last, last, 1.0))
}

pub fn eval_property(animations: &[Animation], prop: AnimProperty, time_sec: f32, default: f32) -> f32 {
    let mut matching = animations.iter().filter(|a| a.property == prop).peekable();
    let Some(first) = matching.peek().copied() else {
        return default;
    };
    if !first.keyframes.is_empty() {
        if time_sec <= first.keyframes[0].time {
            return match first.keyframes[0].value {
                AnimValue::Number(v) => v,
                AnimValue::Color(_) => default,
            };
        }
    } else if time_sec <= first.start_sec {
        return match first.from {
            AnimValue::Number(v) => v,
            AnimValue::Color(_) => default,
        };
    }

    let mut result = default;
    for anim in matching {
        if !anim.keyframes.is_empty() {
            if time_sec < anim.keyframes[0].time {
                break;
            }
            if let Some((k0, k1, eased_t)) = eval_keyframes_step(&anim.keyframes, time_sec) {
                if let (AnimValue::Number(v0), AnimValue::Number(v1)) = (&k0.value, &k1.value) {
                    result = *v0 + (*v1 - *v0) * eased_t;
                }
            }
            continue;
        }
        if time_sec < anim.start_sec {
            break;
        }
        let (AnimValue::Number(from), AnimValue::Number(to)) = (&anim.from, &anim.to) else {
            continue;
        };
        result = match eval_anim_step(anim, time_sec) {
            AnimStep::FinishedFrom => *from,
            AnimStep::FinishedTo => *to,
            AnimStep::Interpolate(eased_t) => *from + (*to - *from) * eased_t,
        };
    }
    result
}

pub fn parse_rgba_u8(hex: &str) -> Option<[u8; 4]> {
    let s = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    match s.len() {
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&s[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&s[2..3], 16).ok()? * 17;
            Some([r, g, b, 255])
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some([r, g, b, 255])
        }
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            let a = u8::from_str_radix(&s[6..8], 16).ok()?;
            Some([r, g, b, a])
        }
        _ => None,
    }
}

pub fn eval_color_property(
    animations: &[Animation],
    prop: AnimProperty,
    time_sec: f32,
    default: Option<&str>,
) -> Option<String> {
    let mut matching = animations.iter().filter(|a| a.property == prop).peekable();
    let Some(first) = matching.peek().copied() else {
        return default.map(|s| s.to_string());
    };
    if !first.keyframes.is_empty() {
        if time_sec <= first.keyframes[0].time {
            return match &first.keyframes[0].value {
                AnimValue::Color(c) => Some(c.clone()),
                AnimValue::Number(_) => default.map(|s| s.to_string()),
            };
        }
    } else if time_sec <= first.start_sec {
        return match &first.from {
            AnimValue::Color(c) => Some(c.clone()),
            AnimValue::Number(_) => default.map(|s| s.to_string()),
        };
    }

    let mut result = default.map(|s| s.to_string());
    for anim in matching {
        if !anim.keyframes.is_empty() {
            if time_sec < anim.keyframes[0].time {
                break;
            }
            if let Some((k0, k1, eased_t)) = eval_keyframes_step(&anim.keyframes, time_sec) {
                if let (AnimValue::Color(c0s), AnimValue::Color(c1s)) = (&k0.value, &k1.value) {
                    if let (Some(c0), Some(c1)) = (parse_rgba_u8(c0s), parse_rgba_u8(c1s)) {
                        let lerp = |a: u8, b: u8| -> u8 {
                            ((a as f32) + ((b as f32) - (a as f32)) * eased_t)
                                .round()
                                .clamp(0.0, 255.0) as u8
                        };
                        result = Some(format!(
                            "#{:02X}{:02X}{:02X}{:02X}",
                            lerp(c0[0], c1[0]),
                            lerp(c0[1], c1[1]),
                            lerp(c0[2], c1[2]),
                            lerp(c0[3], c1[3])
                        ));
                    } else {
                        result = Some(c1s.clone());
                    }
                }
            }
            continue;
        }
        if time_sec < anim.start_sec {
            break;
        }
        let (AnimValue::Color(from_s), AnimValue::Color(to_s)) = (&anim.from, &anim.to) else {
            continue;
        };
        result = match eval_anim_step(anim, time_sec) {
            AnimStep::FinishedFrom => Some(from_s.clone()),
            AnimStep::FinishedTo => Some(to_s.clone()),
            AnimStep::Interpolate(eased_t) => {
                if let (Some(c0), Some(c1)) = (parse_rgba_u8(from_s), parse_rgba_u8(to_s)) {
                    let lerp = |a: u8, b: u8| -> u8 {
                        ((a as f32) + ((b as f32) - (a as f32)) * eased_t)
                            .round()
                            .clamp(0.0, 255.0) as u8
                    };
                    Some(format!(
                        "#{:02x}{:02x}{:02x}{:02x}",
                        lerp(c0[0], c1[0]),
                        lerp(c0[1], c1[1]),
                        lerp(c0[2], c1[2]),
                        lerp(c0[3], c1[3])
                    ))
                } else {
                    Some(to_s.clone())
                }
            }
        };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_linear_and_clamping() {
        let anims = vec![Animation {
            property: AnimProperty::X,
            from: 100.0.into(),
            to: 200.0.into(),
            start_sec: 1.0,
            end_sec: 3.0,
            keyframes: vec![],
            easing: Easing::Linear,
            bezier: None,
            spring: None,
            repeat: None,
            yoyo: false,
        }];
        assert_eq!(eval_property(&anims, AnimProperty::X, 0.5, 0.0), 100.0);
        assert_eq!(eval_property(&anims, AnimProperty::X, 2.0, 0.0), 150.0);
        assert_eq!(eval_property(&anims, AnimProperty::X, 4.0, 0.0), 200.0);
        assert_eq!(eval_property(&anims, AnimProperty::Y, 2.0, 42.0), 42.0);
    }

    #[test]
    fn test_spring_bounds() {
        assert!((apply_easing(0.0, Easing::Spring) - 0.0).abs() < 1e-5);
        let end = apply_easing(1.0, Easing::Spring);
        assert!((end - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_custom_spring_overshoot() {
        let cfg = SpringConfig {
            stiffness: 180.0,
            damping: 10.0,
            mass: 1.0,
        };
        assert!((eval_spring_physics(0.0, cfg)).abs() < 1e-5);
        // Underdamped spring overshoots 1.0 around peak
        let peak = eval_spring_physics(0.25, cfg);
        assert!(peak > 1.0, "expected spring overshoot > 1.0, got {peak}");
    }

    #[test]
    fn test_repeat_and_yoyo() {
        let anims = vec![Animation {
            property: AnimProperty::Scale,
            from: 0.5.into(),
            to: 1.5.into(),
            start_sec: 0.0,
            end_sec: 1.0,
            keyframes: vec![],
            easing: Easing::Linear,
            bezier: None,
            spring: None,
            repeat: Some(RepeatSpec::Count(2)),
            yoyo: true,
        }];
        assert!((eval_property(&anims, AnimProperty::Scale, 0.5, 1.0) - 1.0).abs() < 1e-5);
        assert!((eval_property(&anims, AnimProperty::Scale, 1.5, 1.0) - 1.0).abs() < 1e-5);
        assert!((eval_property(&anims, AnimProperty::Scale, 2.5, 1.0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_color_interpolation_and_cubic_bezier() {
        let anims = vec![Animation {
            property: AnimProperty::Color,
            from: AnimValue::Color("#ff0000".to_string()),
            to: AnimValue::Color("#0000ff".to_string()),
            start_sec: 0.0,
            end_sec: 1.0,
            keyframes: vec![],
            easing: Easing::Linear,
            bezier: None,
            spring: None,
            repeat: None,
            yoyo: false,
        }];
        let mid = eval_color_property(&anims, AnimProperty::Color, 0.5, Some("#000000")).unwrap();
        assert_eq!(mid, "#800080ff");

        let b0 = eval_cubic_bezier(0.0, [0.42, 0.0, 0.58, 1.0]);
        let b1 = eval_cubic_bezier(1.0, [0.42, 0.0, 0.58, 1.0]);
        let b_mid = eval_cubic_bezier(0.5, [0.42, 0.0, 0.58, 1.0]);
        assert!((b0 - 0.0).abs() < 1e-4);
        assert!((b1 - 1.0).abs() < 1e-4);
        assert!((b_mid - 0.5).abs() < 1e-3);
    }
}
