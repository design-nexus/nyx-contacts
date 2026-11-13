use crate::{
    formats::{self, Import},
    model,
    ui::{self, Ui},
};
use gtk::{gio, prelude::*};
use nyx_ui::widgets as w;
use std::{path::Path, rc::Rc};
pub fn choose_import(ui: &Ui) {
    let picker = gtk::FileDialog::builder().title("Import contacts").modal(true).build();
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("vCard and CSV files"));
    for pattern in ["*.vcf", "*.vcard", "*.csv", "*.VCF", "*.CSV"] {
        filter.add_pattern(pattern);
    }
    picker.set_default_filter(Some(&filter));
    let u = ui.clone();
    picker.open(Some(&ui.window), None::<&gio::Cancellable>, move |result| {
        if let Ok(file) = result
            && let Some(path) = file.path() {
                import_path(&u, &path);
            }
    });
}
pub fn import_path(ui: &Ui, path: &Path) {
    let source = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "the file".into());
    match formats::read(path) {
        Ok((text, csv)) => {
            if csv {
                match formats::csv_data(&text) {
                    Ok(data) => mapping(ui, data, source.clone()),
                    Err(e) => ui::toast(ui, &format!("Could not read CSV: {e}")),
                }
            } else {
                preview(ui, formats::vcard_read(&text), &source);
            }
        }
        Err(e) => ui::toast(ui, &format!("Could not import: {e}")),
    }
}
fn mapping(ui: &Ui, data: formats::CsvData, source: String) {
    let (dialog, card) = w::dialog("Map CSV columns", 560);
    crate::dialog_dismissal::add(&dialog, &card, "Cancel");
    dialog.set_default_height(620);
    card.append(&ui::label("Review the destination for each column. Unmapped columns will be omitted.", "dim"));
    let body = w::vbox(8);
    card.append(&ui::scroll(&body));
    let mut options = vec!["Skip column".to_string()];
    options.extend(formats::COLUMNS.iter().map(|s| s.to_string()));
    let refs: Vec<_> = options.iter().map(String::as_str).collect();
    let defaults = formats::mapping(&data.headers);
    let mut choices = vec![];
    for (header, default) in data.headers.iter().zip(defaults) {
        body.append(&ui::label(header, "dim"));
        let dd = gtk::DropDown::from_strings(&refs);
        dd.set_selected(default.map(|i| i as u32 + 1).unwrap_or(0));
        body.append(&dd);
        choices.push(dd);
    }
    let error = ui::label("", "error");
    card.append(&error);
    let b = ui::button("Preview import", "Review contacts before importing");
    card.append(&b);
    let u = ui.clone();
    let d = dialog.clone();
    b.connect_clicked(move |_| {
        let map: Vec<_> = choices.iter().map(|dd| (dd.selected() as usize).checked_sub(1)).collect();
        let mut used = std::collections::BTreeSet::new();
        if map.iter().flatten().any(|i| !used.insert(*i)) {
            error.set_text("Map each destination field only once.");
            return;
        }
        let parsed = formats::csv_contacts(&data, &map);
        d.close();
        preview(&u, parsed, &source);
    });
    dialog.present();
}
/// The book contact an imported card is a newer copy of: same vCard UID, or a UID
/// that is the contact's own id (a card exported from here).
fn existing<'a>(book: &'a model::Book, c: &model::Contact) -> Option<&'a model::Contact> {
    let uid = formats::uid(c)?;
    book.contacts.iter().find(|o| o.id == uid || formats::uid(o).as_deref() == Some(uid.as_str()))
}

/// What an imported card adds to the contact it updates.
fn what_changes(old: &model::Contact, new: &model::Contact) -> String {
    let adds = |a: &[model::Field], b: &[model::Field]| b.iter().any(|f| !a.iter().any(|o| o.value.eq_ignore_ascii_case(&f.value)));
    let mut parts = vec![];
    if adds(&old.emails, &new.emails) {
        parts.push("adds an email");
    }
    if adds(&old.phones, &new.phones) {
        parts.push("adds a phone");
    }
    if adds(&old.addresses, &new.addresses) {
        parts.push("adds an address");
    }
    if !new.title.is_empty() && new.title != old.title {
        parts.push("new job title");
    }
    if !new.company.is_empty() && new.company != old.company {
        parts.push("new company");
    }
    if parts.is_empty() {
        "no new details".into()
    } else {
        parts.join(", ")
    }
}

fn preview(ui: &Ui, import: Import, source: &str) {
    let n = import.contacts.len();
    let title = if n == 1 { format!("Import 1 contact from {source}") } else { format!("Import {n} contacts from {source}") };
    let (dialog, card) = w::dialog(&title, 520);
    dialog.add_css_class("import-dialog");
    dialog.set_default_height(560);
    let book = ui.book.borrow().clone();
    // Duplicates among the new cards, and against the book, through one index.
    let mut all = book.contacts.clone();
    all.extend(import.contacts.iter().filter(|c| existing(&book, c).is_none()).cloned());
    let index = model::DupIndex::new(&all);
    let flagged: std::collections::HashSet<String> = index.flagged().into_iter().map(str::to_string).collect();
    let updates = import.contacts.iter().filter(|c| existing(&book, c).is_some()).count();
    let duplicates = all[book.contacts.len()..].iter().filter(|c| flagged.contains(&c.id)).count();
    let section = ui.section.borrow().clone();
    let into = match book.lists.iter().find(|l| l.id == section) {
        Some(l) => format!("Into On this computer and the {} list", l.name),
        None => "Into On this computer".to_string(),
    };
    let mut summary = vec![into];
    match updates {
        0 => {}
        1 => summary.push("1 updates someone you have".into()),
        n => summary.push(format!("{n} update people you have")),
    }
    match duplicates {
        0 => {}
        1 => summary.push("1 looks like a duplicate".into()),
        n => summary.push(format!("{n} look like duplicates")),
    }
    let sub = ui::label(&summary.join(" · "), "dim");
    sub.set_selectable(false);
    card.append(&sub);
    for warning in &import.warnings {
        card.append(&ui::label(warning, "error"));
    }
    let rows = w::vbox(0);
    rows.add_css_class("field-card");
    rows.add_css_class("import-rows");
    let scroll = ui::scroll(&rows);
    scroll.set_min_content_height(200);
    card.append(&scroll);
    let checks: std::rc::Rc<Vec<gtk::CheckButton>> = std::rc::Rc::new(
        import
            .contacts
            .iter()
            .map(|c| {
                let row = w::hbox(12);
                row.add_css_class("import-row");
                let check = gtk::CheckButton::new();
                check.add_css_class("row-check");
                row.append(&check);
                row.append(&ui::avatar(c, 28));
                let text = w::vbox(1);
                text.set_hexpand(true);
                let name = w::label(&c.name(false), "import-name");
                name.set_xalign(0.0);
                text.append(&name);
                let old = existing(&book, c);
                let twin = (old.is_none() && flagged.contains(&c.id))
                    .then(|| all.iter().find(|o| o.id != c.id && model::duplicates(c, o)))
                    .flatten();
                let line = match (old, twin) {
                    (Some(o), _) => what_changes(o, c),
                    (None, Some(t)) => format!("same {} as {}", model::shared(c, t).unwrap_or("details"), t.name(false)),
                    _ => c.emails.first().or(c.phones.first()).map(|f| f.value.clone()).unwrap_or_else(|| c.company.clone()),
                };
                let l = w::label(&line, "import-sub");
                l.set_xalign(0.0);
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                text.append(&l);
                row.append(&text);
                if old.is_some() {
                    let t = w::label("updates", "import-tag");
                    t.set_valign(gtk::Align::Center);
                    row.append(&t);
                } else if twin.is_some() {
                    let t = w::label("duplicate?", "import-tag");
                    t.add_css_class("warn");
                    t.set_valign(gtk::Align::Center);
                    row.append(&t);
                }
                check.set_active(twin.is_none());
                let r = row.clone();
                check.connect_toggled(move |c| {
                    if c.is_active() {
                        r.remove_css_class("off")
                    } else {
                        r.add_css_class("off")
                    }
                });
                if twin.is_some() {
                    row.add_css_class("off");
                }
                rows.append(&row);
                check
            })
            .collect(),
    );
    let actions = w::hbox(8);
    actions.set_halign(gtk::Align::End);
    actions.set_margin_top(12);
    let cancel = ui::button("Cancel", "Cancel import");
    cancel.add_css_class("flat");
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    actions.append(&cancel);
    let save = ui::button("", "Add the checked contacts to the address book");
    save.add_css_class("suggested-action");
    actions.append(&save);
    card.append(&actions);
    let count = {
        let (checks, save) = (checks.clone(), save.clone());
        move || {
            let k = checks.iter().filter(|c| c.is_active()).count();
            save.set_label(&format!("Import {k}"));
            save.set_sensitive(k > 0);
        }
    };
    count();
    for c in checks.iter() {
        let count = count.clone();
        c.connect_toggled(move |_| count());
    }
    dialog.set_default_widget(Some(&save));
    let u = ui.clone();
    let d = dialog.clone();
    save.connect_clicked(move |_| {
        let mut book = u.book.borrow().clone();
        let section = u.section.borrow().clone();
        let mut added = 0;
        for (c, check) in import.contacts.iter().zip(checks.iter()) {
            if !check.is_active() {
                continue;
            }
            added += 1;
            let mut c = c.clone();
            // A newer copy of a card already here replaces it, keeping its id and lists.
            if let Some(id) = existing(&book, &c).map(|o| o.id.clone()) {
                c.id = id.clone();
                if let Some(old) = book.contacts.iter_mut().find(|o| o.id == id) {
                    c.favorite |= old.favorite;
                    *old = c;
                }
                continue;
            }
            c.id = model::id();
            if let Some(l) = book.lists.iter_mut().find(|l| l.id == section) {
                l.members.insert(c.id.clone());
            }
            book.contacts.push(c);
        }
        if ui::commit(&u, book) {
            d.close();
            ui::toast(&u, &if added == 1 { "Imported 1 contact.".to_string() } else { format!("Imported {added} contacts.") });
        }
    });
    dialog.present();
    save.grab_focus();
}
pub fn export(ui: &Ui, contacts: Vec<model::Contact>, csv: bool) {
    if contacts.is_empty() {
        ui::toast(ui, "There are no contacts to export.");
        return;
    }
    if csv {
        let (dialog, card) = w::dialog("Export CSV", 520);
        crate::dialog_dismissal::add(&dialog, &card, "Cancel");
        card.append(&ui::label("CSV includes names, company, job title, birthday, notes, up to three emails, phone and street address columns, and the first website. Photos, list membership, custom properties, and additional websites are omitted. Use vCard for richer contact details.","dim"));
        let b = ui::button("Choose destination…", "Save an Outlook-compatible CSV");
        card.append(&b);
        let u = ui.clone();
        let contacts = contacts.clone();
        let d = dialog.clone();
        b.connect_clicked(move |_| {
            d.close();
            save_export(&u, contacts.clone(), true);
        });
        dialog.present();
    } else {
        save_export(ui, contacts, false);
    }
}
fn save_export(ui: &Ui, contacts: Vec<model::Contact>, csv: bool) {
    let data = if csv { formats::csv_write(&contacts) } else { Ok(formats::vcard_write(&contacts).into_bytes()) };
    let bytes = match data {
        Ok(bytes) => Rc::new(bytes),
        Err(e) => {
            ui::toast(ui, &format!("Could not export: {e}"));
            return;
        }
    };
    let picker = gtk::FileDialog::builder()
        .title("Export contacts")
        .initial_name(if csv { "Contacts.csv" } else { "Contacts.vcf" })
        .modal(true)
        .build();
    let u = ui.clone();
    picker.save(Some(&ui.window), None::<&gio::Cancellable>, move |result| {
        if let Ok(file) = result
            && let Some(path) = file.path() {
                match nyx_core::cmd::atomic_write(&path, &*bytes) {
                    Ok(()) => ui::toast(&u, &format!("Exported {} contacts.", contacts.len())),
                    Err(e) => ui::toast(&u, &format!("Could not export: {e}")),
                }
            }
    });
}
