//! What the window loads before the first picture: the Spaces, Today, and where they live.
//!
//! How a Space is painted is `paint.rs`.

use crate::appearance::WindowDirs;
use crate::space::{self, Spaces};
use crate::today::{self, Today};
use dioxus::prelude::*;
use ds::prelude::*;
use ds::style::kit::Styles;
use ds_settings::Environment;

/// What the first render needs, and nothing it has to ask the disk for again.
#[derive(Clone)]
pub struct Boot {
    pub spaces: Spaces,
    pub today: Today,
    pub dirs: Option<WindowDirs>,
}

impl std::fmt::Debug for Boot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Boot")
    }
}

/// Load Spaces and Today.
///
/// Called from the component, where the contexts are. A missing file is a first run: one Space
/// per account, saved when there is a config directory to save it in (`space::boot`). A window
/// handed no directories takes the Spaces it was handed, if any (a test's), else a first run.
pub fn load_boot() -> Boot {
    let dirs = try_consume_dirs();
    let client = crate::live::client();
    let ids = crate::data::accounts(&client);
    // What mailo wrote before quire, read only: the window-wide theme a Space without one of its
    // own starts from.
    let legacy = dirs
        .as_ref()
        .map(|dirs| crate::appearance::legacy(&dirs.config))
        .unwrap_or_default();
    let spaces = match (&dirs, dioxus::prelude::try_consume_context::<Spaces>()) {
        (None, Some(handed)) => handed,
        _ => space::boot(dirs.as_ref(), &ids, &legacy),
    };
    // Today keeps an entry until it is cleared: nothing is pruned for being idle.
    let today = dirs
        .as_ref()
        .map(|dirs| today::load(&dirs.state))
        .unwrap_or_default();
    Boot {
        spaces,
        today,
        dirs,
    }
}

fn try_consume_dirs() -> Option<WindowDirs> {
    dioxus::prelude::try_consume_context::<WindowDirs>()
}

/// Write `spaces` to `spaces.json` when the window has a config directory.
///
/// Atomic, through `space::save`. A window launched with nowhere to write keeps its Spaces
/// for the session, and a file that cannot be written changes nothing on screen.
pub fn keep(spaces: &Spaces) {
    if let Some(dirs) = try_consume_dirs()
        && space::save(Some(&dirs), spaces).is_ok()
    {
        // The other windows wear the Space too, and read the file again.
        crate::configured::told_configuration();
    }
}

/// Follow the configuration files the other windows write (`configured::Configured`): when one
/// writes `settings.toml`, `keyboard.json` or `spaces.json`, read them again into the window's
/// settings, `shell` and `spaces`. The main window's Spaces go through `handle`, which leaves
/// them alone while a part of a Space's menu is open, whose close writes them; another window's
/// only wear them. A keymap that differs from the one the window's `keys` hold, whether this
/// window's Keyboard page changed it or another's did, is handed to `keys` at once, so a
/// rebinding or a reset applies without reopening anything.
pub fn use_followed_configuration(
    mut shell: Signal<crate::view::Shell>,
    mut spaces: Signal<Spaces>,
    handle: Option<crate::space::Handle>,
    keys: ds::prelude::Keys,
) {
    let mut applied = use_hook(|| CopyValue::new(shell.peek().keymap.overrides()));
    use_effect(move || {
        let overrides = shell.read().keymap.overrides();
        if *applied.peek() != overrides {
            applied.set(overrides.clone());
            keys.set_overrides(overrides);
        }
    });
    let configured = use_signal(|| 0u64);
    crate::configured::use_shared_configuration(configured);
    use_effect(move || {
        let _ = configured();
        crate::prefs::reread();
        let Some(dirs) = try_consume_dirs() else {
            return;
        };
        let keymap = crate::keymap::load(&dirs.config);
        if shell.peek().keymap != keymap {
            shell.write().keymap = keymap;
        }
        // A missing or damaged file changes nothing here: the main window's own load says why.
        let Some(stored) = space::stored(&dirs) else {
            return;
        };
        match handle {
            Some(handle) => handle.reload(stored),
            None if *spaces.peek() != stored => spaces.set(stored),
            None => {}
        }
    });
}

/// What the window resolves its look from: the live settings and desktop preferences `launch`
/// provides, else a fixed value a test provides, else the first run.
///
/// A test never reaches `ds_settings::use_environment`, so it never reads or watches the real
/// config directory.
pub fn environment() -> Environment {
    if let Some(live) = try_consume_context::<ReadSignal<Environment>>() {
        return live();
    }
    try_consume_context::<Environment>().unwrap_or_default()
}

/// The person's own stylesheets, live when `launch` watches the config directory, else none.
pub fn styles() -> ReadSignal<Styles> {
    match try_consume_context::<ReadSignal<Styles>>() {
        Some(live) => live,
        None => ReadSignal::new(use_signal(Styles::default)),
    }
}

/// The quire root the window draws inside, wearing the current Space.
///
/// Its own component so a Space change re-renders only the root's attributes and frame
/// layers, not `App`: the children are `App`'s, unchanged. The Space's look is the frame (the
/// sidebar sits on its colour, the card on paper); its motion is the root's motion level, since
/// quire's `SpaceLook` has none (reported to quire); the rest of the appearance, the type
/// included, is `appearance.toml`'s. A switch or an edit only writes the Spaces, and `Ds`
/// cross-fades the frame's layers itself. `sheet` is the design system's stylesheet when a window
/// needs more than `ds::stylesheet()`: the add-account window's parts are `ds-shell`'s.
#[component]
pub fn Frame(
    spaces: Signal<Spaces>,
    #[props(default)] sheet: Option<&'static str>,
    children: Element,
) -> Element {
    let environment = environment();
    let look = spaces.read().current().look.clone();
    let appearance = window_appearance(&environment);
    rsx! {
        Ds {
            appearance,
            system: environment.system,
            look,
            material: Material::Window,
            styles: styles(),
            custom_styles: environment.settings.appearance.custom_styles,
            surface: Some("window"),
            sheet,
            {children}
        }
    }
}

/// `appearance.toml`'s appearance. How much the window moves is the person's motion setting and
/// the desktop's reduced-motion preference (`Ds { system }`), never a Space's.
pub fn window_appearance(environment: &Environment) -> ds::prelude::Appearance {
    environment.settings.appearance.appearance()
}
