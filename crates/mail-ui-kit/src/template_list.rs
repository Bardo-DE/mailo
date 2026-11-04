//! The list of templates, built once: ⌘K's "New from template" and the composer's "Start from a
//! template" both draw these rows, so both list the same templates in the same order, under the
//! same labels, narrowed by the same matcher as every other picker ([`narrowed`]).

use crate::menu::{MenuItem, Right, narrowed};
use ds::prelude::Icon;
use mail_client::Client;
use mail_domain::{Template, TemplateId};
use mail_ui_model::copy;

/// How much of a template's first line its row shows.
const FIRST_LINE: usize = 60;

/// Every template with the address it leaves from, in the order the client lists them, or none
/// when the client cannot say.
pub fn every(client: &Client) -> Vec<(String, Template)> {
    client.template().all().unwrap_or_default()
}

/// The rows of a list of templates, `(account address, template)`, narrowed to `typed`: the
/// name, the address it leaves from and its first line, and a × to delete it. Narrowed on the
/// name, best match first with the matched letters marked, as the folder and label pickers are.
pub fn template_rows(all: &[(String, Template)], typed: &str) -> Vec<MenuItem> {
    let rows: Vec<MenuItem> = all
        .iter()
        .map(|(from, template)| MenuItem {
            help: Some(format!("{from} · {}", first_line(template))),
            right: Right::Remove(copy::aria::delete_template(&template.name)),
            ..MenuItem::icon(
                template.id.to_string(),
                Icon::FilePen,
                template.name.clone(),
            )
        })
        .collect();
    narrowed(&rows, typed)
}

/// The first line a template says, else its subject: what a row recognises it by.
fn first_line(template: &Template) -> String {
    let line = template
        .text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(template.subject.as_str());
    let line = if line.is_empty() { "(empty)" } else { line };
    if line.chars().count() > FIRST_LINE {
        format!("{}…", line.chars().take(FIRST_LINE).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// The template a row's key names.
pub fn named(all: &[(String, Template)], key: &str) -> Option<TemplateId> {
    all.iter()
        .map(|(_, template)| template.id)
        .find(|id| id.to_string() == key)
}
