//! Shared handlebars setup for text output.

use handlebars::{handlebars_helper, Handlebars};

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.to_owned(),
        serde_json::Value::Null => String::new(),
        _ => v.to_string(),
    }
}

/// Pad `v` on the left to at least `width` characters. Never truncates.
pub fn left_pad(width: usize, v: &str) -> String {
    format!("{v:>width$}")
}

/// Pad `v` on the right to at least `width` characters. Never truncates.
pub fn right_pad(width: usize, v: &str) -> String {
    format!("{v:<width$}")
}

/// Build a handlebars engine with the `lpad` and `rpad` helpers registered.
///
/// Usage in templates: `{{ lpad 6 value }}` and `{{ rpad 20 value }}`.
pub fn engine() -> Handlebars<'static> {
    handlebars_helper!(lpad: |num: u64, v: Json| {
        left_pad(usize::try_from(num).unwrap_or(0), &value_to_string(v))
    });
    handlebars_helper!(rpad: |num: u64, v: Json| {
        right_pad(usize::try_from(num).unwrap_or(0), &value_to_string(v))
    });
    let mut hb = Handlebars::new();
    // Output is terminal text, not HTML.
    hb.register_escape_fn(handlebars::no_escape);
    hb.register_helper("lpad", Box::new(lpad));
    hb.register_helper("rpad", Box::new(rpad));
    hb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads() {
        assert_eq!(left_pad(4, "ab"), "  ab");
        assert_eq!(right_pad(4, "ab"), "ab  ");
    }

    #[test]
    fn does_not_truncate() {
        assert_eq!(left_pad(1, "abc"), "abc");
        assert_eq!(right_pad(1, "abc"), "abc");
    }

    #[test]
    fn helpers_render() {
        let hb = engine();
        let out = hb
            .render_template(
                "{{ lpad 5 a }}|{{ rpad 5 b }}|",
                &serde_json::json!({"a": 1, "b": "x"}),
            )
            .unwrap();
        assert_eq!(out, "    1|x    |");
    }
}
