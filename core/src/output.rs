//! Deciding where a job's result goes: next to the source, suffixed, and never
//! overwriting anything that already exists.

use std::path::{Path, PathBuf};

/// Build an output path next to `input`.
///
/// - `suffix`: appended to the stem with a leading dash, or empty for none.
///   `clip` + `compressed` → `clip-compressed`.
/// - `new_ext`: extension without the dot, e.g. `"mp4"`.
///
/// If the candidate already exists (or would collide with the source), a
/// `-1`, `-2`, … counter is appended until a free name is found. The source
/// file is never touched.
///
/// `exists` is injected so this is pure and unit-testable; [`output_path`]
/// wraps it with the real filesystem check.
pub fn output_path_with(
    input: &Path,
    suffix: &str,
    new_ext: &str,
    exists: &dyn Fn(&Path) -> bool,
) -> PathBuf {
    let dir = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    let base = if suffix.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}-{suffix}")
    };

    // First candidate has no counter; then -1, -2, …
    let mut counter: u32 = 0;
    loop {
        let name = if counter == 0 {
            format!("{base}.{new_ext}")
        } else {
            format!("{base}-{counter}.{new_ext}")
        };
        let candidate = dir.join(name);
        // Guard against colliding with the source itself (e.g. re-encoding
        // clip.mp4 → mp4 with no suffix would otherwise target the source).
        if candidate != input && !exists(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

/// Real-filesystem wrapper around [`output_path_with`].
pub fn output_path(input: &Path, suffix: &str, new_ext: &str) -> PathBuf {
    output_path_with(input, suffix, new_ext, &|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn taken(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let set: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |p: &Path| set.contains(p)
    }

    #[test]
    fn simple_extension_change_no_collision() {
        let out = output_path_with(Path::new("/vids/clip.webm"), "", "mp4", &taken(&[]));
        assert_eq!(out, PathBuf::from("/vids/clip.mp4"));
    }

    #[test]
    fn adds_suffix() {
        let out = output_path_with(
            Path::new("/vids/clip.mov"),
            "compressed",
            "mp4",
            &taken(&[]),
        );
        assert_eq!(out, PathBuf::from("/vids/clip-compressed.mp4"));
    }

    #[test]
    fn increments_when_taken() {
        let out = output_path_with(
            Path::new("/vids/clip.mov"),
            "compressed",
            "mp4",
            &taken(&["/vids/clip-compressed.mp4", "/vids/clip-compressed-1.mp4"]),
        );
        assert_eq!(out, PathBuf::from("/vids/clip-compressed-2.mp4"));
    }

    #[test]
    fn never_overwrites_the_source() {
        // Converting clip.mp4 → mp4 with no suffix must not target the source.
        let out = output_path_with(Path::new("/vids/clip.mp4"), "", "mp4", &taken(&[]));
        assert_eq!(out, PathBuf::from("/vids/clip-1.mp4"));
    }
}
