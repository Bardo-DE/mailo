//! A person's avatar.

/// A person's avatar fill, stable for an address: quire's person hash (design/03 section 13),
/// the colour its `Avatar` gives the same address, as the text a tile's style takes.
pub fn avatar_color(email: &str) -> String {
    ds::components::content::avatar::person_hue(email)
        .hex()
        .css()
}
