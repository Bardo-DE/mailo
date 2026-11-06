//! The font families installed on the machine, by name: what the system's font list holds,
//! the families a person installed with it.

/// The families installed, sorted by name and each once. It walks every font file's names, so
/// it is called off the thread that draws; it is read afresh each time, so a font installed while
/// mailo runs is in the next answer.
pub fn installed() -> Vec<String> {
    let mut collection = fontique::Collection::new(fontique::CollectionOptions {
        shared: false,
        system_fonts: true,
    });
    let names: Vec<String> = collection.family_names().map(str::to_owned).collect();
    shown(names)
}

/// The families a person can pick, sorted, each once whatever its case: a name that starts with
/// a dot is the system's own, drawn with but never chosen.
fn shown(names: Vec<String>) -> Vec<String> {
    let mut names: Vec<String> = names
        .into_iter()
        .filter(|name| !name.is_empty() && !name.starts_with('.'))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|one, other| one.to_lowercase() == other.to_lowercase());
    names
}

#[cfg(test)]
mod tests {
    use super::shown;

    #[test]
    fn the_system_s_own_families_are_left_out_and_the_rest_sorted_once() {
        let names = [
            "Zapfino",
            ".SF NS",
            "avenir",
            "Avenir Next",
            "",
            "zapfino",
            "Zapfino",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(shown(names), ["avenir", "Avenir Next", "Zapfino"]);
    }
}
