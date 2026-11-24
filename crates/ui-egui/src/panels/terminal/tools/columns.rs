//! Constrained TableView geometry: preserve column minima when the viewport shrinks.
pub(super) enum Policy {
    Even,
    Last,
}
pub(super) fn snap(widths: &mut [f32], scale: f32) {
    if !scale.is_finite() || scale <= 0. {
        return;
    }
    let mut edge = 0.;
    let mut previous = 0.;
    for width in widths {
        edge += *width;
        let next = (edge * scale).round() / scale;
        *width = next - previous;
        previous = next;
    }
}
pub(super) fn fit(widths: &mut [f32], minimums: &[f32], available: f32, policy: Policy) {
    if widths.is_empty() || widths.len() != minimums.len() || !available.is_finite() {
        return;
    }
    for (width, min) in widths.iter_mut().zip(minimums) {
        *width = width.max(*min);
    }
    let target = available.max(minimums.iter().sum());
    let delta = target - widths.iter().sum::<f32>();
    if delta >= 0. {
        match policy {
            Policy::Last => *widths.last_mut().unwrap() += delta,
            Policy::Even => {
                let extra = delta / widths.len() as f32;
                for width in widths {
                    *width += extra;
                }
            }
        }
        return;
    }
    let mut remaining = -delta;
    if matches!(policy, Policy::Last) {
        let last = widths.len() - 1;
        let remove = remaining.min(widths[last] - minimums[last]);
        widths[last] -= remove;
        remaining -= remove;
    }
    let spare = widths
        .iter()
        .zip(minimums)
        .map(|(width, min)| width - min)
        .sum::<f32>();
    if spare > 0. {
        let factor = ((spare - remaining) / spare).clamp(0., 1.);
        for (width, min) in widths.iter_mut().zip(minimums) {
            *width = *min + (*width - *min) * factor;
        }
    }
}
