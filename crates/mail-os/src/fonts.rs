//! The font families installed on the machine, by name: what the system's font list holds,
//! the families a person installed with it. Read once, as reading them walks every font file's
//! names, and kept for as long as mailo runs.

use std::sync::OnceLock;

/// The installed families, sorted by name, once [`load`] has read them.
static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();

/// The families installed, sorted by name and each once; empty until [`load`] has read them.
pub fn installed() -> &'static [String] {
    FAMILIES.get().map_or(&[], Vec::as_slice)
}

/// Read the installed families, if they are not read yet, and answer them. It walks the
/// system's fonts, so it is called off the thread that draws.
pub fn load() -> &'static [String] {
    FAMILIES.get_or_init(read)
}

fn read() -> Vec<String> {
    let mut collection = fontique::Collection::new(fontique::CollectionOptions {
        shared: false,
        system_fonts: true,
    });
    let names: Vec<String> = collection.family_names().map(str::to_owned).collect();
    shown(names)
}

/// The families a person can pick, sorted: a name that starts with a dot is the system's own,
/// drawn with but never chosen.
fn shown(names: Vec<String>) -> Vec<String> {
    let mut names: Vec<String> = names
        .into_iter()
        .filter(|name| !name.is_empty() && !name.starts_with('.'))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::shown;

    #[test]
    fn the_system_s_own_families_are_left_out_and_the_rest_sorted_once() {
        let names = ["Zapfino", ".SF NS", "avenir", "Avenir Next", "", "Zapfino"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(shown(names), ["avenir", "Avenir Next", "Zapfino"]);
    }
}
