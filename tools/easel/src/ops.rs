//! The op table: accepted keys per op, and the `easel ops` reference text.

/// Static description of one DSL op.
pub struct OpSpec {
    pub name: &'static str,
    /// Accepted keys; empty means "any name" (used by `set`).
    pub keys: &'static [&'static str],
    pub required: &'static [&'static str],
    pub help: &'static str,
}

macro_rules! keys {
    ($($k:literal),* $(,)?) => { &[$($k),*] };
}

/// Keys shared by every op that places marks.
macro_rules! mark_keys {
    ($($k:literal),* $(,)?) => {
        keys!["r", "color", "colors", "jitter", "alpha", "mark", "aspect", "angle",
              "soft", "clip", "seed", $($k),*]
    };
}

pub const OPS: &[OpSpec] = &[
    OpSpec {
        name: "layer",
        keys: keys!["name", "opacity", "blend", "visible", "above", "below", "top"],
        required: keys!["name"],
        help: include_str!("help/layer.txt"),
    },
    OpSpec {
        name: "delete-layer",
        keys: keys!["name"],
        required: keys!["name"],
        help: include_str!("help/delete-layer.txt"),
    },
    OpSpec {
        name: "rename-layer",
        keys: keys!["name", "to"],
        required: keys!["name", "to"],
        help: include_str!("help/rename-layer.txt"),
    },
    OpSpec {
        name: "copy-layer",
        keys: keys!["name", "to"],
        required: keys!["name", "to"],
        help: include_str!("help/copy-layer.txt"),
    },
    OpSpec {
        name: "set",
        keys: keys![],
        required: keys![],
        help: include_str!("help/set.txt"),
    },
    OpSpec {
        name: "fill",
        keys: keys!["in", "color", "feather", "alpha", "clip"],
        required: keys!["in", "color"],
        help: include_str!("help/fill.txt"),
    },
    OpSpec {
        name: "gradient",
        keys: keys![
            "in", "kind", "stops", "from", "to", "center", "radius", "focus", "feather", "alpha",
            "clip"
        ],
        required: keys!["in", "stops"],
        help: include_str!("help/gradient.txt"),
    },
    OpSpec {
        name: "clear",
        keys: keys!["in", "feather", "clip"],
        required: keys![],
        help: include_str!("help/clear.txt"),
    },
    OpSpec {
        name: "blur",
        keys: keys!["r", "in", "clip"],
        required: keys!["r"],
        help: include_str!("help/blur.txt"),
    },
    OpSpec {
        name: "dot",
        keys: mark_keys!["at"],
        required: keys!["at"],
        help: include_str!("help/dot.txt"),
    },
    OpSpec {
        name: "stipple",
        keys: mark_keys![
            "in",
            "count",
            "density",
            "placement",
            "spacing",
            "falloff",
            "weight",
            "gamma"
        ],
        required: keys!["in"],
        help: include_str!("help/stipple.txt"),
    },
    OpSpec {
        name: "pointillize",
        keys: mark_keys![
            "source",
            "in",
            "count",
            "density",
            "placement",
            "spacing",
            "min-alpha",
            "falloff",
            "weight",
            "gamma"
        ],
        required: keys!["source"],
        help: include_str!("help/pointillize.txt"),
    },
    OpSpec {
        name: "dabs",
        keys: mark_keys!["path", "smooth", "spacing", "count", "scatter", "taper"],
        required: keys!["path"],
        help: include_str!("help/dabs.txt"),
    },
    OpSpec {
        name: "stroke",
        keys: keys!["path", "width", "color", "smooth", "alpha", "cap", "taper", "clip"],
        required: keys!["path", "width", "color"],
        help: include_str!("help/stroke.txt"),
    },
    OpSpec {
        name: "image",
        keys: keys!["src", "at", "size", "alpha"],
        required: keys!["src"],
        help: include_str!("help/image.txt"),
    },
];

/// Reference topics that are not ops.
pub const TOPICS: &[(&str, &str)] = &[
    ("workflow", include_str!("help/workflow.txt")),
    ("syntax", include_str!("help/syntax.txt")),
    ("values", include_str!("help/values.txt")),
    ("marks", include_str!("help/marks.txt")),
];

pub fn spec(name: &str) -> Option<&'static OpSpec> {
    OPS.iter().find(|s| s.name == name)
}

/// The full reference (no argument) or one op/topic.
///
/// # Errors
///
/// Returns a message listing valid names when `topic` is unknown.
pub fn reference(topic: Option<&str>) -> Result<String, String> {
    match topic {
        None => {
            let mut out = String::new();
            for (_, text) in TOPICS {
                out.push_str(text);
                out.push('\n');
            }
            out.push_str("OPS\n\n");
            for op in OPS {
                out.push_str(op.help);
                out.push('\n');
            }
            Ok(out)
        }
        Some(name) => spec(name)
            .map(|s| s.help.to_string())
            .or_else(|| {
                TOPICS
                    .iter()
                    .find(|(t, _)| *t == name)
                    .map(|(_, text)| text.to_string())
            })
            .ok_or_else(|| {
                let ops: Vec<&str> = OPS.iter().map(|s| s.name).collect();
                let topics: Vec<&str> = TOPICS.iter().map(|(t, _)| *t).collect();
                format!(
                    "no op or topic '{name}'; ops: {}; topics: {}",
                    ops.join(" "),
                    topics.join(" ")
                )
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_op_has_help_naming_it_and_its_keys() {
        for op in OPS {
            assert!(
                op.help.starts_with(op.name),
                "help for {} must start with its name",
                op.name
            );
            for key in op.keys {
                let marks = reference(Some("marks")).expect("marks topic exists");
                let documented =
                    op.help.contains(&format!("{key}=")) || marks.contains(&format!("{key}="));
                assert!(documented, "{}: key '{key}' is undocumented", op.name);
            }
        }
    }

    fn examples(op: &OpSpec) -> impl Iterator<Item = &'static str> {
        op.help.lines().filter_map(|l| l.strip_prefix("  e.g. "))
    }

    #[test]
    fn help_examples_run() {
        const SETUP: &str = "layer name=scratch\nlayer name=under visible=false\n\
                             fill in=canvas color=#888\nlayer name=paint\n";
        for op in OPS {
            assert!(examples(op).count() > 0, "{} has no example", op.name);
            for line in examples(op) {
                let stmts = crate::dsl::parse_script(line)
                    .unwrap_or_else(|e| panic!("{}: example '{line}' fails: {e}", op.name));
                if op.name == "image" {
                    continue; // needs an imported asset
                }
                let white = crate::color::Rgba::new(1.0, 1.0, 1.0, 1.0);
                let mut state = crate::render::RenderState::new(
                    600,
                    800,
                    white,
                    1.0,
                    std::path::PathBuf::new(),
                );
                let setup = crate::dsl::parse_script(SETUP).expect("setup parses");
                state.run(&setup, 1).expect("setup runs");
                state
                    .run(&stmts, 2)
                    .unwrap_or_else(|e| panic!("{}: example '{line}' fails: {e}", op.name));
            }
        }
    }
}
