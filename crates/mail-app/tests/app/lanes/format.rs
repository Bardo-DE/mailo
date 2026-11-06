//! Lane 2: formatting in the composer as a modern editor (Notion, Bear, Apple Notes) does it,
//! driven only by typing, the keyboard, the selection bubble, the `/` and `@` menus and paste.

use super::body::{composing, outline, run_is, select_back};
use super::drive::{Drive, Key, PRIMARY};
use super::window::{Window, ms, parsed, queued};

/// The bubble's mark segments, in order: Bold, Italic, Underline, Strike, Code.
fn mark_segment(n: usize) -> String {
    format!(".bubble .ds-segmented-segment:nth-child({n})")
}

/// Start a code block with the `/` menu.
fn code_block(window: &mut Window) {
    window.type_text("/code");
    window.until("/ opens its menu", |h| {
        h.count(".ds-menu .ds-menu-item") > 0
    });
    window.press(&[], Key::Enter, 1);
    window.until("the menu goes", |h| h.count(".ds-menu") == 0);
}

/// Press `key` with `held` until `done` holds.
fn press_until(window: &mut Window, held: &[Key], key: Key, done: impl Fn(&Window) -> bool) {
    for _ in 0..80 {
        if done(window) {
            return;
        }
        window.harness.chord(held, key);
        window.harness.advance(ms(16));
    }
    window.harness.advance(ms(100));
    assert!(
        done(window),
        "{key:?} never got there: {:?}",
        outline(&window.harness)
    );
}

/// Markdown typed at a line start makes headings, lists, to-dos and quotes, and Enter on an
/// empty item leaves them; backticks, stars and underscores close into code, bold and italic.
/// Undo walks the whole session back to an empty body and redo walks it forward, both ways of
/// asking for it; then the message goes out as HTML and as text that reads naturally.
#[test]
fn a_message_formatted_by_typing_undoes_and_redoes_whole_and_goes_out_as_html_and_text() {
    let mut window = composing();
    window.type_text("# Plan\n");
    assert_eq!(outline(&window.harness), ["h2 Plan", "p"], "after # Plan");
    window.type_text("- a\nb\n");
    assert_eq!(
        outline(&window.harness),
        ["h2 Plan", "ul [a | b | ]"],
        "a third bullet waits after b"
    );
    window.type_text("\n");
    assert_eq!(
        outline(&window.harness),
        ["h2 Plan", "ul [a | b]", "p"],
        "Enter on an empty bullet leaves the list"
    );
    window.type_text("1. one\ntwo\n\n");
    window.type_text("[ ] call\n\n");
    window.type_text("> wise words\n");
    window.type_text("after `cargo` now, **bold**, _soft_ ok");
    assert!(run_is(&window.harness, ".m-code", "cargo"), "no code run");
    assert!(run_is(&window.harness, ".m-b", "bold"), "no bold run");
    assert!(run_is(&window.harness, ".m-i", "soft"), "no italic run");
    window.type_text("\nunder");
    select_back(&mut window, 5);
    window.press(&[PRIMARY], Key::Char('u'), 1);
    let done = [
        "h2 Plan",
        "ul [a | b]",
        "ol [one | two]",
        "ul.todo [call]",
        "blockquote wise words",
        "p after cargo now, bold, soft ok",
        "p under",
    ];
    assert_eq!(outline(&window.harness), done);
    assert!(run_is(&window.harness, ".m-u", "under"), "no underline");

    // Undo to the empty body, redo it all, then one step each way.
    press_until(&mut window, &[PRIMARY], Key::Char('z'), |w| {
        outline(&w.harness) == ["p"]
    });
    press_until(&mut window, &[PRIMARY, Key::Shift], Key::Char('z'), |w| {
        outline(&w.harness) == done && run_is(&w.harness, ".m-u", "under")
    });
    window.press(&[PRIMARY], Key::Char('z'), 1);
    assert_eq!(
        window.harness.count(".c-body .m-u"),
        0,
        "undo left the underline"
    );
    window.press(&[PRIMARY, Key::Shift], Key::Char('z'), 1);
    assert!(
        run_is(&window.harness, ".m-u", "under"),
        "redo did not redo"
    );

    window.press(&[], Key::End, 1);
    window.click(".c-props [*|data-row=to] .c-pin input");
    window.type_text("ada@example.test\n");
    window.click(".c-title input");
    window.type_text("Plan");
    window.click(&super::look::send());
    window.until("the send pill shows", |h| h.count(".ds-send-pill") == 1);
    let sent = queued(&window.store);
    assert_eq!(sent.len(), 1, "not one submission");
    let mail = parsed(&sent[0].raw);

    let html = mail.html.unwrap_or_default();
    for tag in [
        "<h1>Plan</h1>",
        "<ul><li><p>a</p></li><li><p>b</p></li></ul>",
        "<ol><li><p>one</p></li><li><p>two</p></li></ol>",
        "<blockquote><p>wise words</p></blockquote>",
        "<strong>bold</strong>",
        "<em>soft</em>",
        "<code>cargo</code>",
        "<u>under</u>",
    ] {
        assert!(html.contains(tag), "no {tag} in the HTML part:\n{html}");
    }
    let text = mail.text.unwrap_or_default();
    for line in [
        "Plan",
        "- a",
        "- b",
        "1. one",
        "2. two",
        "[ ] call",
        "> wise words",
    ] {
        assert!(
            text.lines().any(|l| l.trim_end() == line),
            "no line {line:?} in the text part:\n{text}"
        );
    }
}

/// A selection made with Shift and the arrows takes Bold from the bubble, which leaves the
/// keyboard with the body for Ctrl-I and Ctrl-U; a drag selects as well. Backspace at the start
/// of a heading or a list item unstyles it before it merges.
#[test]
fn a_selection_takes_marks_from_the_bubble_and_the_keys_and_backspace_unstyles_before_merging() {
    let mut window = composing();
    window.type_text("a bold move");
    select_back(&mut window, 4);
    window.until("the bubble is over the selection", |h| {
        h.count(&mark_segment(1)) == 1
    });
    window.click(&mark_segment(1));
    window.until("the bubble's Bold bolds it", |h| run_is(h, ".m-b", "move"));
    assert!(
        window.harness.is_focused(".c-body"),
        "the bubble took the keyboard from the body"
    );
    window.press(&[PRIMARY], Key::Char('i'), 1);
    window.until("Ctrl-I italicises it", |h| run_is(h, ".m-b.m-i", "move"));
    window.press(&[PRIMARY], Key::Char('u'), 1);
    window.until("Ctrl-U underlines it", |h| {
        run_is(h, ".m-b.m-i.m-u", "move")
    });
    assert_eq!(outline(&window.harness), ["p a bold move"]);

    // A drag selects too: across "bold", then Ctrl-B.
    let (start, end) = word_ends(&window, "bold");
    window.harness.drag(start, end, 6);
    window.harness.advance(ms(100));
    window.press(&[PRIMARY], Key::Char('b'), 1);
    window.until("the dragged word is bold", |h| {
        h.text_of(".c-body .m-b")
            .is_some_and(|run| run.starts_with("bold"))
    });

    window.press(&[], Key::End, 1);
    window.type_text("\n# Title");
    window.press(&[], Key::Home, 1);
    window.press(&[], Key::Backspace, 1);
    assert_eq!(
        outline(&window.harness),
        ["p a bold move", "p Title"],
        "the heading became a paragraph"
    );
    window.press(&[], Key::Backspace, 1);
    assert_eq!(
        outline(&window.harness),
        ["p a bold moveTitle"],
        "then it merged"
    );
    window.press(&[], Key::End, 1);
    window.type_text("\n- item");
    window.press(&[], Key::Home, 1);
    window.press(&[], Key::Backspace, 1);
    assert_eq!(
        outline(&window.harness),
        ["p a bold moveTitle", "p item"],
        "the bullet became a paragraph"
    );
    window.press(&[], Key::Backspace, 1);
    assert_eq!(outline(&window.harness), ["p a bold moveTitleitem"]);
}

/// Where a drag over `word` in the body's only paragraph starts and ends: the paragraph's text
/// is laid out in one line, so the word's ends are found by its letters' share of the line.
fn word_ends(window: &Window, word: &str) -> (ds::prelude::Point, ds::prelude::Point) {
    let line = window
        .harness
        .rect(".c-body > p")
        .expect("the paragraph is drawn");
    let text = window.text(".c-body > p");
    let start = text.find(word).expect("the word is in the paragraph");
    let rects: Vec<_> = (1..=window.harness.count(".c-body > p > span"))
        .filter_map(|n| {
            window
                .harness
                .rect(&format!(".c-body > p > span:nth-child({n})"))
        })
        .collect();
    let left = rects.first().map_or(line.origin.x.0, |r| r.origin.x.0);
    let right = rects
        .last()
        .map_or(line.origin.x.0, |r| r.origin.x.0 + r.size.width.0);
    let per = (right - left) / text.chars().count() as f32;
    let y = rects.first().map_or(line.origin.y.0 + 8.0, |r| {
        r.origin.y.0 + r.size.height.0 / 2.0
    });
    let at = |chars: usize| ds::prelude::Point {
        x: ds::prelude::Px(left + per * chars as f32),
        y: ds::prelude::Px(y),
    };
    (at(start), at(start + word.chars().count()))
}

/// The `/` menu turns a line into a quote and a code block, and `@` puts a person in Cc.
#[test]
fn the_slash_menu_makes_a_quote_and_a_code_block_and_an_at_sign_puts_a_person_in_cc() {
    let mut window = composing();
    let pick = |window: &mut Window, what: &str| {
        window.until(what, |h| h.count(".ds-menu .ds-menu-item") > 0);
        window.press(&[], Key::Enter, 1);
        window.until("the menu goes", |h| h.count(".ds-menu") == 0);
    };
    window.type_text("thanks @grac");
    pick(&mut window, "@ offers the contact");
    window.type_text("\n/quo");
    pick(&mut window, "/ opens its menu");
    window.type_text("Measure twice\n");
    window.type_text("/code");
    pick(&mut window, "/ opens its menu again");
    window.type_text("cargo test");
    assert_eq!(
        outline(&window.harness)[1..],
        ["blockquote Measure twice", "pre cargo test"]
    );
    window.until("Grace is in Cc", |h| {
        h.text_of(".c-props [*|data-row=cc]")
            .is_some_and(|cc| cc.contains("Grace Hopper"))
    });
}

/// Clipboard HTML as a word processor in a browser writes it: a wrapper that says "not bold",
/// styled spans for the marks, a nested list and a link.
const DOCS: &str = concat!(
    r#"<meta charset="utf-8"><b style="font-weight:normal;" id="docs-internal-guid-1">"#,
    r#"<h1 dir="ltr"><span style="font-size:20pt;font-weight:400;">Agenda</span></h1>"#,
    r#"<p dir="ltr"><span style="font-weight:400;">Bring </span>"#,
    r#"<span style="font-weight:700;">the numbers</span>"#,
    r#"<span style="font-weight:400;"> and </span>"#,
    r#"<span style="font-style:italic;font-weight:400;">a pen</span>"#,
    r#"<span style="font-weight:400;">, see </span>"#,
    r#"<a href="https://example.test/plan?utm_source=docs"><span style="color:#1155cc;text-decoration:underline;">the plan</span></a></p>"#,
    r#"<ul><li dir="ltr"><p dir="ltr"><span>Budget</span></p></li>"#,
    r#"<ul><li dir="ltr"><p dir="ltr"><span>Travel</span></p></li></ul>"#,
    r#"<li dir="ltr"><p dir="ltr"><span>Hiring</span></p></li></ul></b>"#,
);

#[test]
fn rich_html_pasted_from_a_word_processor_keeps_its_structure_spaces_and_link() {
    let mut window = composing();
    window.harness.paste_html(
        DOCS,
        "Agenda\nBring the numbers and a pen, see the plan\nBudget\nTravel\nHiring",
    );
    window.until("the paste lands", |h| h.count(".c-body > h2") == 1);
    assert_eq!(
        outline(&window.harness),
        [
            "h2 Agenda",
            "p Bring the numbers and a pen, see the plan",
            // Travel is nested under Budget, drawn as a list of its own a level in.
            "ul [Budget]",
            "ul [Travel]",
            "ul [Hiring]",
        ]
    );
    assert_eq!(
        window.harness.count(".c-body .m-b"),
        1,
        "the not-bold wrapper made more than the bold span bold"
    );
    assert_eq!(
        window.harness.attr(".c-body .m-a", "href").as_deref(),
        Some("https://example.test/plan"),
        "the link keeps its address, without the tracking parameter"
    );
}

#[test]
fn styled_spans_pasted_from_a_word_processor_keep_their_bold_and_italic() {
    let mut window = composing();
    window
        .harness
        .paste_html(DOCS, "Agenda\nBring the numbers and a pen, see the plan");
    window.until("the paste lands", |h| h.count(".c-body > h2") == 1);
    assert!(
        run_is(&window.harness, ".m-b", "the numbers"),
        "the styled bold span is not bold"
    );
    assert!(
        run_is(&window.harness, ".m-i", "a pen"),
        "the styled italic span is not italic"
    );
}

/// What an input method commits is written, and nothing else. A zhuyin composition is drawn at
/// the caret as its preedit and leaves the body untouched until the commit, a key pressed while
/// it composes is the IME's, and typing after the commit is text again; then a kana to kanji
/// conversion after typed text. The IME's candidate window stands at the caret.
#[test]
fn an_input_method_writes_its_commit_and_nothing_else_and_its_window_stands_at_the_caret() {
    let mut window = composing();

    // Zhuyin.
    window.harness.ime_start();
    window.harness.ime_update("ㄓ", "ㄓ".len());
    window.harness.ime_update("ㄓㄨ", "ㄓㄨ".len());
    window.until("the zhuyin preedit is drawn", |h| {
        h.text_of(".c-preedit").as_deref() == Some("ㄓㄨ")
    });
    assert_eq!(
        outline(&window.harness),
        ["p"],
        "the preedit touched the body"
    );
    // A key while the IME composes is the IME's.
    window.harness.key(Key::Char('x'));
    // winit clears the preedit (an empty update), then commits.
    window.harness.ime_commit("注");
    window.until("the zhuyin commit is written", |h| outline(h) == ["p 注"]);
    assert_eq!(
        window.harness.count(".c-preedit"),
        0,
        "the preedit outlived the commit"
    );
    window.type_text("a");
    assert_eq!(
        outline(&window.harness),
        ["p 注a"],
        "typing after the commit"
    );

    // The candidate window follows the caret.
    window.harness.advance(ms(100));
    let caret = window.harness.rect(".c-caret").expect("no caret is drawn");
    let area = window
        .harness
        .ime_cursor_area()
        .expect("the IME has no area");
    assert!(
        (area.origin.x.0 - caret.origin.x.0).abs() <= 1.0
            && (area.origin.y.0 - caret.origin.y.0).abs() <= 1.0,
        "the IME's area {area:?} is not at the caret {caret:?}"
    );

    // Kana to kanji, after typed text.
    window.type_text(" ");
    window.harness.ime_start();
    for preedit in ["に", "にほ", "にほん", "日本"] {
        window.harness.ime_update(preedit, preedit.len());
    }
    window.until("the kana preedit is drawn", |h| {
        h.text_of(".c-preedit").as_deref() == Some("日本")
    });
    assert_eq!(
        outline(&window.harness),
        ["p 注a "],
        "the kana preedit touched the body"
    );
    window.harness.ime_commit("日本");
    window.harness.ime_end();
    window.until("the kanji commit is written", |h| {
        outline(h) == ["p 注a 日本"]
    });
}

/// The paragraph and text styles from the keys and the bubble: ⌘} aligns right, Tab nests a
/// list item and Enter brings it out, ⌘' quotes a quote a level deeper, ⌘> sizes a word up and
/// the bubble's colour swatches colour another. All of it goes out in the HTML part, and the
/// nesting and the levels in the text part.
#[test]
fn alignment_nesting_quote_levels_size_and_colour_go_out_in_the_html() {
    let mut window = composing();
    window.type_text("To the right");
    window.press(&[PRIMARY, Key::Shift], Key::Char('}'), 1);
    window.until("⌘} aligns the paragraph right", |h| {
        h.attr(".c-body > p", "style")
            .is_some_and(|style| style.contains("text-align:right"))
    });

    window.type_text("\n");
    window.press(&[PRIMARY, Key::Shift], Key::Char('{'), 1);
    window.type_text("- one\ntwo");
    window.press(&[], Key::Tab, 1);
    assert_eq!(
        outline(&window.harness)[1..],
        ["ul [one]", "ul [two]"],
        "Tab nests the second item in a list of its own"
    );
    assert!(
        window.harness.is_focused(".c-body"),
        "Tab in a list item left the body"
    );
    window.type_text("\nthree");
    window.press(&[Key::Shift], Key::Tab, 1);
    assert_eq!(
        outline(&window.harness)[1..],
        ["ul [one]", "ul [two]", "ul [three]"],
        "Shift-Tab brings the third item back out"
    );

    window.type_text("\n\n> deep");
    window.press(&[PRIMARY], Key::Char('\''), 1);
    window.until("⌘' quotes it a level deeper", |h| {
        h.attr(".c-body > blockquote", "style")
            .is_some_and(|style| style.contains("margin-left"))
    });

    window.type_text("\nbig and blue");
    window.press(&[], Key::Home, 1);
    window.press(&[PRIMARY], Key::Char('\''), 1);
    window.press(&[PRIMARY, Key::Alt], Key::Char('\''), 1);
    assert_eq!(
        outline(&window.harness).last().map(String::as_str),
        Some("p big and blue"),
        "⌥⌘' takes the quote it made away again"
    );
    // On the right, where the bar is placed from the selection's end to stay in the window.
    window.press(&[PRIMARY, Key::Shift], Key::Char('}'), 1);
    window.press(&[], Key::End, 1);
    select_back(&mut window, 4);
    window.until("the bubble is over the selection", |h| {
        h.count(&mark_segment(1)) == 1
    });
    window.click(&format!(
        ".bubble [*|aria-label=\"{}\"]",
        mail_ui_model::copy::aria::TEXT_COLOUR
    ));
    window.until("the swatches are in the bar's place", |h| {
        h.count(".bubble .ds-radio-group-item") == 7
    });
    // The palette after No Colour: grey, red, orange, green, blue.
    window.click(".bubble .ds-radio-group-item:nth-child(6)");
    window.until("the swatch colours the word", |h| {
        h.attr(".c-body > p:last-of-type > span:last-child", "style")
            .is_some_and(|style| style.contains("#1a73e8"))
    });
    window.press(&[], Key::Home, 1);
    window.press(&[Key::Shift], Key::Right, 3);
    window.press(&[PRIMARY, Key::Shift], Key::Char('>'), 1);
    window.until("⌘> sizes the word up", |h| {
        h.attr(".c-body > p:last-of-type > span:first-child", "style")
            .is_some_and(|style| style.contains("font-size"))
    });

    window.click(".c-props [*|data-row=to] .c-pin input");
    window.type_text("ada@example.test\n");
    window.click(".c-title input");
    window.type_text("Styles");
    window.click(&super::look::send());
    window.until("the send pill shows", |h| h.count(".ds-send-pill") == 1);
    let sent = queued(&window.store);
    assert_eq!(sent.len(), 1, "not one submission");
    let mail = parsed(&sent[0].raw);

    let html = mail.html.unwrap_or_default();
    for tag in [
        r#"<p style="text-align:right">To the right</p>"#,
        "<ul><li><p>one</p><ul><li><p>two</p></li></ul></li><li><p>three</p></li></ul>",
        "<blockquote><blockquote><p>deep</p></blockquote></blockquote>",
        r#"<span style="font-size:large">big</span>"#,
        r#"<span style="color:#1a73e8">blue</span>"#,
        r#"<p style="text-align:right"><span"#,
    ] {
        assert!(html.contains(tag), "no {tag} in the HTML part:\n{html}");
    }
    let text = mail.text.unwrap_or_default();
    for line in ["- one", "- three", ">> deep"] {
        assert!(
            text.lines().any(|l| l.trim_end() == line),
            "no line {line:?} in the text part:\n{text}"
        );
    }
    // The nested item is indented (and, as format=flowed has it, space-stuffed).
    assert!(
        text.lines()
            .any(|l| l.starts_with("  ") && l.trim() == "- two"),
        "the nested item is not indented in the text part:\n{text}"
    );
}

/// A selection in a code block shows over the block's own ground, where the drag made it.
#[test]
fn a_selection_in_a_code_block_shows_over_the_block() {
    let mut window = composing();
    window.type_text("/code");
    window.until("/ opens its menu", |h| {
        h.count(".ds-menu .ds-menu-item") > 0
    });
    window.press(&[], Key::Enter, 1);
    window.until("the menu goes", |h| h.count(".ds-menu") == 0);
    window.type_text("cargo test --all");
    select_back(&mut window, 5);
    window.until("the selection is drawn", |h| h.count(".c-sel") == 1);
    let block = window
        .harness
        .rect(".c-body > pre")
        .expect("the code block");
    let sel = window.harness.rect(".c-sel").expect("the selection");
    let shot = window.harness.render().expect("the window renders");
    let scale = shot.width() as f32 / window.harness.window_size().width as f32;
    // The selection's top row is the line's leading, clear of the letters.
    let y = sel.origin.y.0 + 1.0;
    let at = |x: f32| {
        let pixel = shot.get_pixel((x * scale) as u32, (y * scale) as u32);
        [pixel[0], pixel[1], pixel[2]]
    };
    let selected = at(sel.origin.x.0 + sel.size.width.0 / 2.0);
    let ground = at(block.origin.x.0 + block.size.width.0 - 12.0);
    assert_ne!(
        selected, ground,
        "the selection is hidden under the code block's ground"
    );
}

/// An empty line is as high as a line with words on it, in a paragraph and after a line break.
#[test]
fn an_empty_line_is_one_line_high() {
    let mut window = composing();
    window.type_text("one\n\nthree\nfour");
    window.press(&[Key::Shift], Key::Enter, 1);
    let height = |window: &Window, n: usize| {
        window
            .harness
            .rect(&format!(".c-body > :nth-child({n})"))
            .map(|rect| rect.size.height.0)
            .unwrap_or_default()
    };
    let line = height(&window, 1);
    assert!(line > 0.0, "the first line is not drawn");
    assert!(
        (height(&window, 2) - line).abs() < 0.5,
        "an empty paragraph is {} px high, a line {line} px",
        height(&window, 2)
    );
    assert!(
        (height(&window, 4) - 2.0 * line).abs() < 0.5,
        "a line and the empty one a trailing break makes are {} px high, a line {line} px",
        height(&window, 4)
    );
    window.press(&[], Key::Up, 3);
    window.type_text("two");
    assert_eq!(
        outline(&window.harness),
        ["p one", "p two", "p three", "p four"],
        "the caret stands on the empty line"
    );
}

/// An empty heading keeps its height and a caret to stand on.
#[test]
fn an_empty_heading_is_one_line_high() {
    let mut window = composing();
    window.type_text("# x");
    let full = window.harness.rect(".c-body > h2").expect("the heading");
    window.press(&[], Key::Backspace, 1);
    window.until("the heading is empty", |h| outline(h) == ["h2"]);
    let empty = window.harness.rect(".c-body > h2").expect("the heading");
    assert!(
        (empty.size.height.0 - full.size.height.0).abs() < 0.5,
        "an empty heading is {} px high, a heading {} px",
        empty.size.height.0,
        full.size.height.0
    );
    let caret = window.harness.rect(".c-caret").expect("the caret");
    assert!(caret.size.height.0 > 0.0, "the caret has no height");
}

/// The bubble's menus open with the bubble still there, and a pick from Turn into turns the line.
#[test]
fn the_bubble_s_menus_open_over_the_bubble_and_turn_into_turns_the_line() {
    let mut window = composing();
    window.type_text("hello world");
    select_back(&mut window, 5);
    window.until("the bubble shows", |h| h.count(".bubble") == 1);
    for label in ["Text Size", "Paragraph"] {
        window.click(&format!(".bubble [*|aria-label=\"{label}\"]"));
        window.until(&format!("{label} opens its menu"), |h| {
            h.count(".ds-menu .ds-menu-item") > 0
        });
        assert_eq!(
            window.harness.count(".bubble"),
            1,
            "{label} took the bubble"
        );
        window.press(&[], Key::Escape, 1);
        window.until("Escape closes the menu and leaves the bubble", |h| {
            h.count(".ds-menu") == 0 && h.count(".bubble") == 1
        });
    }
    window.click(".bubble .ds-popup .ds-button");
    window.until("Turn into opens its menu", |h| {
        h.count(".ds-menu .ds-menu-item") > 0
    });
    assert_eq!(
        window.harness.count(".bubble"),
        1,
        "Turn into took the bubble"
    );
    window.menu_item("Quote");
    window.until("the line is a quote", |h| {
        outline(h) == ["blockquote hello world"]
    });
    window.until("the body has the keyboard back", |h| h.focus_of(".c-body"));
}

#[test]
fn typing_past_the_bottom_keeps_the_caret_in_view() {
    let mut window = composing();
    for n in 0..45 {
        window.type_text(&format!("line {n}\n"));
    }
    window.harness.advance(ms(300));
    let bottom = window.harness.window_size().height as f32;
    let caret = window.harness.rect(".c-caret").expect("the caret");
    assert!(
        caret.origin.y.0 + caret.size.height.0 <= bottom,
        "the caret ends at {} px in a window {bottom} px high",
        caret.origin.y.0 + caret.size.height.0
    );
    // Wheeled away, the view stays until the caret moves again.
    let at = window.harness.centre(".c-body").expect("the body");
    window
        .harness
        .wheel(at, ds::prelude::Px(0.0), ds::prelude::Px(240.0));
    window.harness.advance(ms(300));
    let wheeled = window.harness.rect(".c-caret").expect("the caret");
    assert!(
        wheeled.origin.y.0 > caret.origin.y.0,
        "the wheel did not move the view"
    );
    window.type_text("x");
    window.harness.advance(ms(300));
    let typed = window.harness.rect(".c-caret").expect("the caret");
    assert!(
        typed.origin.y.0 + typed.size.height.0 <= bottom,
        "typing did not bring the caret back"
    );
}

#[test]
fn the_link_field_takes_the_keyboard_and_typing_leaves_the_words_alone() {
    let mut window = composing();
    window.type_text("hello world");
    select_back(&mut window, 5);
    window.press(&[PRIMARY], Key::Char('k'), 1);
    window.until("the field has the keyboard", |h| {
        h.focus_of(".link-in input")
    });
    window.type_text("https://example.test\n");
    window.until("the words are a link", |h| h.count(".c-body .m-a") == 1);
    assert_eq!(outline(&window.harness), ["p hello world"]);
}

#[test]
fn enter_on_an_empty_last_line_leaves_a_code_block() {
    let mut window = composing();
    code_block(&mut window);
    window.type_text("x\n\ny");
    assert_eq!(outline(&window.harness), ["pre x", "p y"]);
}

#[test]
fn a_paste_into_a_code_block_stays_in_it_as_it_was_written() {
    let mut window = composing();
    code_block(&mut window);
    window.type_text("x");
    window.harness.paste_html("", "\nfn main() {\n\n}");
    window.until("the paste lands", |h| {
        h.text_of(".c-body").is_some_and(|text| text.contains('}'))
    });
    assert_eq!(outline(&window.harness), ["pre x\nfn main() {\n\n}"]);
}

#[test]
fn a_plain_paste_keeps_the_blank_lines_past_a_paragraph_s_own() {
    let mut window = composing();
    window.harness.paste_html("", "one\n\ntwo\n\n\nfour");
    window.until("the paste lands", |h| {
        h.text_of(".c-body")
            .is_some_and(|text| text.contains("four"))
    });
    assert_eq!(outline(&window.harness), ["p one", "p two", "p", "p four"]);
}

#[test]
fn a_slash_that_matches_nothing_closes_the_menu_and_enter_breaks_the_line() {
    let mut window = composing();
    window.type_text("/usr/local/bin");
    window.until("the menu goes", |h| h.count(".ds-menu") == 0);
    window.type_text("\nnext");
    assert_eq!(outline(&window.harness), ["p /usr/local/bin", "p next"]);
}

/// A slip past what the menu matches comes back with a Backspace.
#[test]
fn a_backspace_brings_a_slash_menu_back() {
    let mut window = composing();
    window.type_text("/codx");
    window.until("the menu goes", |h| h.count(".ds-menu") == 0);
    window.press(&[], Key::Backspace, 1);
    window.until("the menu is back", |h| {
        h.count(".ds-menu .ds-menu-item") > 0
    });
}

/// An `@` that finds nobody draws no menu, and leaves the arrows to the line.
#[test]
fn an_at_sign_that_finds_nobody_leaves_the_arrows_alone() {
    let mut window = composing();
    window.type_text("first\nsee @zzzz");
    window.until("no menu is drawn", |h| h.count(".ds-menu") == 0);
    window.press(&[], Key::Up, 1);
    window.type_text("!");
    assert_eq!(outline(&window.harness)[0], "p first!");
}

#[test]
fn escape_over_a_selection_closes_the_bubble_and_leaves_the_composer() {
    let mut window = composing();
    window.type_text("hello world");
    select_back(&mut window, 5);
    window.until("the bubble shows", |h| h.count(".bubble") == 1);
    window.press(&[], Key::Escape, 1);
    window.until("the bubble goes", |h| h.count(".bubble") == 0);
    assert_eq!(
        window.harness.count(".cpage .c-body"),
        1,
        "the composer closed"
    );
}

#[test]
fn enter_at_the_start_of_a_heading_pushes_it_down_a_line() {
    let mut window = composing();
    window.type_text("# Title");
    window.press(&[], Key::Home, 1);
    window.press(&[], Key::Enter, 1);
    assert_eq!(outline(&window.harness), ["p", "h2 Title"]);
}

#[test]
fn several_lines_turned_into_code_are_one_block() {
    let mut window = composing();
    window.type_text("one\ntwo");
    window.press(&[PRIMARY], Key::Char('a'), 1);
    window.click(".bubble .ds-popup .ds-button");
    window.menu_item("Code");
    window.until("the lines are code", |h| h.count(".c-body > pre") > 0);
    assert_eq!(outline(&window.harness), ["pre one\ntwo"]);
}

#[test]
fn tab_in_a_code_block_stays_in_the_block() {
    let mut window = composing();
    code_block(&mut window);
    window.type_text("a");
    window.press(&[], Key::Tab, 1);
    window.type_text("b");
    assert!(window.harness.focus_of(".c-body"), "Tab left the body");
    let text = window.harness.text_of(".c-body > pre").unwrap_or_default();
    assert!(text.ends_with('b'), "the b was lost: {text:?}");
}

/// Pasted code keeps its blank lines, and Shift-Enter makes one where Enter would leave.
#[test]
fn a_blank_line_in_a_code_block_comes_from_shift_enter() {
    let mut window = composing();
    code_block(&mut window);
    window.type_text("x");
    window.press(&[Key::Shift], Key::Enter, 2);
    window.type_text("y");
    assert_eq!(outline(&window.harness), ["pre x\n\ny"]);
}

/// In a code block `/`, `@` and `:` are what they are; Tab with words selected keeps the words.
#[test]
fn a_code_block_opens_no_menu_and_tab_leaves_a_selection_alone() {
    let mut window = composing();
    code_block(&mut window);
    window.type_text("/usr @home :smile");
    assert_eq!(window.harness.count(".ds-menu"), 0, "a menu opened in code");
    select_back(&mut window, 6);
    window.press(&[], Key::Tab, 1);
    assert_eq!(outline(&window.harness), ["pre /usr @home :smile"]);
    assert!(window.harness.focus_of(".c-body"), "Tab left the body");
}

/// Turn into keeps the selection, as a size or an alignment does, so a second pick can follow.
#[test]
fn turn_into_keeps_the_selection() {
    let mut window = composing();
    window.type_text("one\ntwo");
    window.press(&[PRIMARY], Key::Char('a'), 1);
    window.click(".bubble .ds-popup .ds-button");
    window.menu_item("Code");
    window.until("the bubble is back over the block", |h| {
        h.count(".bubble") == 1 && outline(h) == ["pre one\ntwo"]
    });
    window.click(".bubble .ds-popup .ds-button");
    window.menu_item("Quote");
    window.until("the block is a quote", |h| {
        outline(h) == ["blockquote one\ntwo"]
    });
    window.press(&[PRIMARY], Key::Char('z'), 2);
    assert_eq!(outline(&window.harness), ["p one", "p two"], "undo");
}

/// Escape in the link field closes the field and gives the bar back, and the draft stays open.
#[test]
fn escape_in_the_link_field_leaves_the_composer() {
    let mut window = composing();
    window.type_text("hello world");
    select_back(&mut window, 5);
    window.press(&[PRIMARY], Key::Char('k'), 1);
    window.until("the field has the keyboard", |h| {
        h.focus_of(".link-in input")
    });
    window.press(&[], Key::Escape, 1);
    window.until("the bar is back", |h| {
        h.count(".link-in") == 0 && h.count(".bubble") == 1
    });
    assert_eq!(
        window.harness.count(".cpage .c-body"),
        1,
        "the composer closed"
    );
}

/// Escape on the colour row gives the bar back with the selection as it was.
#[test]
fn escape_on_the_colour_row_keeps_the_selection() {
    let mut window = composing();
    window.type_text("hello world");
    select_back(&mut window, 5);
    window.click(".bubble [*|aria-label=\"Text Colour\"]");
    window.until("the swatches show", |h| h.count(".c-swatches") == 1);
    window.press(&[], Key::Escape, 1);
    window.until("the bar is back", |h| {
        h.count(".c-swatches") == 0 && h.count(".bubble") == 1
    });
    assert_eq!(window.harness.count(".c-sel"), 1, "the selection went");
}

/// Select All leaves the view where it is, so the bubble over the first line can be seen.
#[test]
fn select_all_does_not_move_the_view() {
    let mut window = composing();
    for n in 0..45 {
        window.type_text(&format!("line {n}\n"));
    }
    window.press(&[], Key::Up, 45);
    window.harness.advance(ms(300));
    let before = window.harness.rect(".c-body").expect("the body");
    window.press(&[PRIMARY], Key::Char('a'), 1);
    window.harness.advance(ms(300));
    let after = window.harness.rect(".c-body").expect("the body");
    assert_eq!(before.origin.y, after.origin.y, "Select All scrolled");
}

/// Lines either side of a divider each join; the divider stays between them.
#[test]
fn lines_turned_into_code_join_up_to_an_object() {
    let mut window = composing();
    window.type_text("a\nb\n/divider");
    window.until("/ opens its menu", |h| {
        h.count(".ds-menu .ds-menu-item") > 0
    });
    window.press(&[], Key::Enter, 1);
    window.until("the menu goes", |h| h.count(".ds-menu") == 0);
    window.type_text("c\nd");
    window.press(&[PRIMARY], Key::Char('a'), 1);
    window.click(".bubble .ds-popup .ds-button");
    window.menu_item("Code");
    window.until("the lines are code", |h| h.count(".c-body > pre") == 2);
    let body = outline(&window.harness);
    assert_eq!(
        (body[0].as_str(), body[2].as_str()),
        ("pre a\nb", "pre c\nd")
    );
    window.press(&[PRIMARY], Key::Char('z'), 1);
    assert_eq!(outline(&window.harness)[0], "p a", "undo");
}

/// Enter in a block of one blank line turns it back; no empty block is left above.
#[test]
fn enter_in_a_blank_code_block_leaves_no_block_behind() {
    let mut window = composing();
    code_block(&mut window);
    window.press(&[Key::Shift], Key::Enter, 1);
    window.press(&[], Key::Enter, 1);
    assert_eq!(outline(&window.harness), ["p"]);
}

/// A clipboard with only HTML goes into code as its words.
#[test]
fn a_paste_into_code_of_html_alone_is_its_words() {
    let mut window = composing();
    code_block(&mut window);
    window.type_text("keep");
    window.harness.paste_html("<p><b>bold</b></p>", "");
    window.until("the paste lands", |h| outline(h) == ["pre keepbold"]);
    assert_eq!(window.harness.count(".c-body .m-b"), 0, "the bold came too");
}

/// A font picked from the bubble sets the selected words in it, in the composer and in the
/// message sent, with the kind of face a reader without it falls back to.
#[test]
fn a_font_picked_in_the_bubble_sets_the_words_in_it() {
    let mut window = composing();
    window.type_text("hello world");
    select_back(&mut window, 5);
    window.click(".bubble [*|aria-label=\"Font\"]");
    window.menu_item("Georgia");
    window.until("the words are in Georgia", |h| {
        run_is(h, "[style*=\"Georgia\"]", "world")
    });
    assert!(run_is(&window.harness, "[style*=\"serif\"]", "world"));

    window.press(&[], Key::End, 1);
    window.click(".c-props [*|data-row=to] .c-pin input");
    window.type_text("ada@example.test\n");
    window.click(".c-title input");
    window.type_text("Type");
    window.click(&super::look::send());
    window.until("the send pill shows", |h| h.count(".ds-send-pill") == 1);
    let sent = queued(&window.store);
    let html = parsed(&sent[0].raw).html.unwrap_or_default();
    assert!(
        html.contains("<span style=\"font-family:'Georgia',serif\">world</span>"),
        "the font is not in the HTML part:\n{html}"
    );
}

/// The `/` menu opens where it stays: quire places a menu it caps to the room below the line at
/// that height from its first frame, so a tall menu low in the window is never drawn higher up
/// for a frame before it drops into place.
#[test]
fn the_slash_menu_opens_where_it_stays() {
    let mut window = composing();
    window.harness.resize_window(ds_blitz::Extent {
        width: 1100,
        height: 640,
    });
    window.harness.advance(ms(200));
    window.type_text("one\ntwo\nthree\nfour\nhello ");
    window.harness.key_pending(Key::Char('/'));
    let mut tops = Vec::new();
    for _ in 0..4 {
        window.harness.step_frame();
        if let Some(menu) = window.harness.rect(".ds-menu") {
            tops.push(menu.origin.y.0);
        }
    }
    assert!(
        tops.windows(2).all(|two| (two[0] - two[1]).abs() < 0.5),
        "the menu moved as it opened: {tops:?}"
    );
}
