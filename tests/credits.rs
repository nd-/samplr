// The README's Credits follow the dependencies: a dependency added without
// a credit, or a credit left for a dependency gone, fails here.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Each dependency credited, with what the Credits must say of it: its name,
/// or its project's when the crate is part of a bigger one.
const CREDITED: &[(&str, &str)] = &[
    ("clap", "clap"),
    ("ebur128", "ebur128"),
    ("notify-debouncer-full", "notify-rs/notify"),
    ("rodio", "rodio"),
    ("rusqlite", "rusqlite"),
    ("symphonia", "Symphonia"),
    ("trash", "trash"),
    ("walkdir", "walkdir"),
    ("tauri", "tauri-apps/tauri"),
    ("tauri-build", "tauri-apps/tauri"),
    ("tauri-plugin-dialog", "tauri-apps/plugins-workspace"),
    ("tauri-plugin-window-state", "tauri-apps/plugins-workspace"),
    ("tauri-plugin-drag", "crabnebula-dev/drag-rs"),
    ("ssstretch", "ssstretch"),
    ("@playwright/test", "Playwright"),
];

/// Small helpers the Credits leave out on purpose.
const UNCREDITED: &[&str] = &["dirs", "dunce", "serde", "serde_json"];

fn read(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The names declared in the given sections of a Cargo.toml.
fn cargo_dependencies(manifest: &str, sections: &[&str]) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = sections.contains(&line);
            continue;
        }
        if !inside || line.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            names.push(name.trim().to_string());
        }
    }
    names
}

/// Every dependency of samplr: the library and command line, the
/// interface, and the interface's tests.
fn dependencies() -> BTreeSet<String> {
    let mut all: BTreeSet<String> = cargo_dependencies(&read("Cargo.toml"), &["[dependencies]"])
        .into_iter()
        .chain(cargo_dependencies(
            &read("src-tauri/Cargo.toml"),
            &["[dependencies]", "[build-dependencies]"],
        ))
        .collect();
    // The interface depends on the library itself.
    all.remove("samplr");
    let package: serde_json::Value = serde_json::from_str(&read("tests/ui/package.json")).unwrap();
    for section in ["dependencies", "devDependencies"] {
        if let Some(packages) = package[section].as_object() {
            all.extend(packages.keys().cloned());
        }
    }
    all
}

/// The README's Credits section.
fn credits() -> String {
    let readme = read("README.md");
    let start = readme
        .find("\n## Credits")
        .expect("no Credits section in README.md");
    let section = &readme[start + 1..];
    let end = section[3..]
        .find("\n## ")
        .map_or(section.len(), |end| end + 3);
    section[..end].to_string()
}

#[test]
fn every_dependency_is_credited() {
    let credited: BTreeSet<&str> = CREDITED.iter().map(|(name, _)| *name).collect();
    for dependency in dependencies() {
        assert!(
            credited.contains(dependency.as_str()) || UNCREDITED.contains(&dependency.as_str()),
            "{dependency} is a new dependency: credit it in README.md and add it to CREDITED in \
             tests/credits.rs, or add it to UNCREDITED"
        );
    }
}

#[test]
fn every_credit_is_for_a_dependency_still_there() {
    let dependencies = dependencies();
    let credits = credits();
    for (dependency, said) in CREDITED {
        assert!(
            dependencies.contains(*dependency),
            "{dependency} is no longer a dependency: take its credit out of README.md and \
             CREDITED"
        );
        assert!(
            credits.contains(said),
            "README.md's Credits do not mention {said}, for {dependency}"
        );
    }
    for dependency in UNCREDITED {
        assert!(
            dependencies.contains(*dependency),
            "{dependency} is no longer a dependency: take it out of UNCREDITED"
        );
    }
}
