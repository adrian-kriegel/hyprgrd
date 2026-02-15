//! CSS-like cubic Bézier easing for scalar interpolation.

/// Interpolate between two scalars using a CSS-like cubic-bezier easing.
///
/// Control points are (0,0), (x1,y1), (x2,y2), (1,1).
/// `u` is normalized time in [0,1].
pub fn bezier_scalar(a: f32, b: f32, u: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    fn clamp01(x: f32) -> f32 {
        x.max(0.0).min(1.0)
    }

    // Precompute polynomial coefficients for x(t) and y(t)
    // with endpoints fixed at (0,0) and (1,1).
    // Bezier cubic form:
    // B(t) = ((ax*t + bx)*t + cx)*t  (since d=0 for x,y; and x3,y3=1 gives the t^3 + ...)
    // We'll build for both x and y.

    // For x(t):
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;

    // For y(t):
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;

    #[inline]
    fn sample_curve(a: f32, b: f32, c: f32, t: f32) -> f32 {
        ((a * t + b) * t + c) * t
    }

    // Solve x(t) = u for t in [0,1]
    fn solve_t_for_x(u: f32, ax: f32, bx: f32, cx: f32) -> f32 {
        let u = u.max(0.0).min(1.0);

        // Newton-Raphson
        let mut t = u; // good initial guess
        for _ in 0..8 {
            let x = ((ax * t + bx) * t + cx) * t - u;
            if x.abs() < 1e-6 {
                return t;
            }
            let dx = (3.0 * ax * t + 2.0 * bx) * t + cx;
            if dx.abs() < 1e-6 {
                break; // fallback to bisection
            }
            t -= x / dx;
            if t < 0.0 || t > 1.0 {
                break; // fallback to bisection
            }
        }

        // Bisection fallback (robust)
        let mut lo = 0.0;
        let mut hi = 1.0;
        t = u;

        for _ in 0..24 {
            let x = ((ax * t + bx) * t + cx) * t;
            if (x - u).abs() < 1e-7 {
                return t;
            }
            if x < u {
                lo = t;
            } else {
                hi = t;
            }
            t = 0.5 * (lo + hi);
        }
        t
    }

    let u = clamp01(u);
    let t = solve_t_for_x(u, ax, bx, cx);
    let eased = sample_curve(ay, by, cy, t); // y(t)

    a + (b - a) * eased
}

/// Convenience for CSS `ease` == cubic-bezier(0.25, 0.1, 0.25, 1.0)
pub fn ease_scalar(a: f32, b: f32, u: f32) -> f32 {
    bezier_scalar(a, b, u, 0.25, 0.10, 0.25, 1.00)
}
