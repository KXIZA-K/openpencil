use jian_ops_schema::node::text::TextAlign;

pub(super) fn parse_px(value: &str) -> Option<f64> {
    value
        .trim()
        .strip_suffix("px")
        .unwrap_or(value.trim())
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

pub(super) fn parse_text_align(value: &str) -> Option<TextAlign> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" | "start" => Some(TextAlign::Left),
        "center" => Some(TextAlign::Center),
        "right" | "end" => Some(TextAlign::Right),
        "justify" => Some(TextAlign::Justify),
        _ => None,
    }
}

pub(super) fn matrix_rotation(value: &str) -> Option<f64> {
    let body = value.trim().strip_prefix("matrix(")?.strip_suffix(')')?;
    let values: Vec<f64> = body
        .split(',')
        .map(|part| part.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    if values.len() != 6 || !values.iter().all(|value| value.is_finite()) {
        return None;
    }
    Some(values[1].atan2(values[0]).to_degrees())
}
