use crate::{
    model::{self, Contact, Field},
    prefs,
    ui::{self, Ui},
};
use gtk::{gdk, glib, prelude::*};
use nyx_ui::widgets as w;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Fields {
    rows: FieldRows,
}
/// The labels offered for each kind of value; any other can be typed.
fn labels_for(title: &str) -> &'static [&'static str] {
    match title {
        "PHONE" => &["cell", "home", "work", "main", "fax", "pager", "other"],
        "WEBSITE" => &["home", "work", "blog", "profile", "other"],
        _ => &["home", "work", "school", "other"],
    }
}
/// A heading with an add button at its end.
fn heading(body: &gtk::Box, title: &str, tip: &str) -> gtk::Button {
    let head = w::hbox(6);
    let l = ui::label(title, "contact-heading");
    l.set_hexpand(true);
    head.append(&l);
    let add = w::icon_button("list-add-symbolic", tip);
    head.append(&add);
    body.append(&head);
    add
}
impl Fields {
    fn new(title: &str, values: &[Field], body: &gtk::Box) -> Self {
        let add = heading(body, title, &format!("Add {}", title.to_lowercase()));
        let root = w::vbox(6);
        body.append(&root);
        let rows = Rc::new(RefCell::new(vec![]));
        let labels = labels_for(title);
        for f in values {
            add_field(&root, &rows, f, labels);
        }
        let r = root.clone();
        let rs = rows.clone();
        add.connect_clicked(move |_| add_field(&r, &rs, &Field { label: labels[0].into(), value: String::new() }, labels));
        Self { rows }
    }
    fn values(&self) -> Vec<Field> {
        self.rows
            .borrow()
            .iter()
            .filter(|(b, _, v)| {
                b.parent().is_some()
                    && !v.buffer().text(&v.buffer().start_iter(), &v.buffer().end_iter(), false).trim().is_empty()
            })
            .map(|(_, l, v)| Field {
                label: l.text().trim().to_string(),
                value: v.buffer().text(&v.buffer().start_iter(), &v.buffer().end_iter(), false).trim().to_string(),
            })
            .collect()
    }
}
/// A field editor's rows: the row, its label entry and its value.
type FieldRows = Rc<RefCell<Vec<(gtk::Box, gtk::Entry, gtk::TextView)>>>;
/// The merge card's choices: which field, its picker, and the values offered.
type Choices = Rc<RefCell<Vec<(usize, gtk::DropDown, Vec<String>)>>>;
/// A label entry with a menu of the usual labels beside it.
fn label_picker(row: &gtk::Box, current: &str, labels: &[&'static str]) -> gtk::Entry {
    let name = gtk::Entry::new();
    name.set_width_chars(8);
    name.set_max_width_chars(9);
    name.set_placeholder_text(Some("Label"));
    name.set_text(current);
    name.set_valign(gtk::Align::Start);
    row.append(&name);
    let pick = gtk::MenuButton::new();
    pick.set_icon_name("pan-down-symbolic");
    pick.set_tooltip_text(Some("Choose a label"));
    pick.add_css_class("flat");
    pick.set_valign(gtk::Align::Start);
    let list = w::vbox(0);
    let pop = gtk::Popover::new();
    for &l in labels {
        let b = gtk::Button::with_label(l);
        b.add_css_class("flat");
        let (n, p) = (name.clone(), pop.clone());
        b.connect_clicked(move |_| {
            n.set_text(l);
            p.popdown();
        });
        list.append(&b);
    }
    pop.set_child(Some(&list));
    pick.set_popover(Some(&pop));
    row.append(&pick);
    name
}
fn add_field(root: &gtk::Box, rows: &FieldRows, f: &Field, labels: &[&'static str]) {
    let row = w::hbox(6);
    let name = label_picker(&row, &f.label, labels);
    let value = gtk::TextView::new();
    value.set_hexpand(true);
    value.set_wrap_mode(gtk::WrapMode::WordChar);
    value.set_top_margin(6);
    value.set_bottom_margin(6);
    value.set_left_margin(6);
    value.set_right_margin(6);
    value.buffer().set_text(&f.value);
    value.add_css_class("contact-input");
    row.append(&value);
    let remove = w::bar_button("list-remove-symbolic", "Remove field");
    remove.set_valign(gtk::Align::Start);
    row.append(&remove);
    let r = root.clone();
    let b = row.clone();
    remove.connect_clicked(move |_| r.remove(&b));
    root.append(&row);
    rows.borrow_mut().push((row, name, value));
}
/// The address editor's rows: the row, its label, the five visible parts, and the two
/// vCard parts (PO box, extended) that are kept as they were.
type AddressRows = Rc<RefCell<Vec<(gtk::Box, gtk::Entry, [gtk::Entry; 5], (String, String))>>>;
struct Addresses {
    rows: AddressRows,
}
impl Addresses {
    fn new(values: &[Field], body: &gtk::Box) -> Self {
        let add = heading(body, "ADDRESS", "Add address");
        let root = w::vbox(10);
        body.append(&root);
        let rows: AddressRows = Rc::new(RefCell::new(vec![]));
        for f in values {
            add_address(&root, &rows, f);
        }
        let (r, rs) = (root.clone(), rows.clone());
        add.connect_clicked(move |_| add_address(&r, &rs, &Field { label: "home".into(), value: String::new() }));
        Self { rows }
    }
    fn values(&self) -> Vec<Field> {
        self.rows
            .borrow()
            .iter()
            .filter(|(b, ..)| b.parent().is_some())
            .map(|(_, label, parts, (po_box, extended))| {
                let t = |i: usize| parts[i].text().trim().to_string();
                let a = model::Address {
                    po_box: po_box.clone(),
                    extended: extended.clone(),
                    street: t(0),
                    city: t(1),
                    region: t(2),
                    postal: t(3),
                    country: t(4),
                };
                (label.text().trim().to_string(), a)
            })
            .filter(|(_, a)| !a.is_empty())
            .map(|(label, a)| Field { label, value: a.value() })
            .collect()
    }
}
fn add_address(root: &gtk::Box, rows: &AddressRows, f: &Field) {
    let a = model::Address::parse(&f.value);
    let row = w::hbox(6);
    let label = label_picker(&row, &f.label, &["home", "work", "school", "other"]);
    let grid = gtk::Grid::builder().row_spacing(4).column_spacing(4).hexpand(true).build();
    let entry = |text: &str, hint: &str| {
        let e = gtk::Entry::new();
        e.set_text(text);
        e.set_placeholder_text(Some(hint));
        e.set_hexpand(true);
        e
    };
    let parts = [
        entry(&a.street, "Street"),
        entry(&a.city, "City"),
        entry(&a.region, "State or region"),
        entry(&a.postal, "Postal code"),
        entry(&a.country, "Country"),
    ];
    // Street across; city and region; postal code and country.
    grid.attach(&parts[0], 0, 0, 2, 1);
    grid.attach(&parts[1], 0, 1, 1, 1);
    grid.attach(&parts[2], 1, 1, 1, 1);
    grid.attach(&parts[3], 0, 2, 1, 1);
    grid.attach(&parts[4], 1, 2, 1, 1);
    row.append(&grid);
    let remove = w::bar_button("list-remove-symbolic", "Remove address");
    remove.set_valign(gtk::Align::Start);
    row.append(&remove);
    let (r, b) = (root.clone(), row.clone());
    remove.connect_clicked(move |_| r.remove(&b));
    root.append(&row);
    rows.borrow_mut().push((row, label, parts, (a.po_box, a.extended)));
}
const NAMES: &[&str] =
    &["First name", "Last name", "Middle name", "Prefix", "Suffix", "Display name", "Company", "Job title", "Birthday"];
fn singles(c: &Contact) -> Vec<String> {
    vec![
        c.first.clone(),
        c.last.clone(),
        c.middle.clone(),
        c.prefix.clone(),
        c.suffix.clone(),
        c.display.clone(),
        c.company.clone(),
        c.title.clone(),
        c.birthday.clone(),
    ]
}
fn set_singles(c: &mut Contact, v: &[String]) {
    c.first = v[0].clone();
    c.last = v[1].clone();
    c.middle = v[2].clone();
    c.prefix = v[3].clone();
    c.suffix = v[4].clone();
    c.display = v[5].clone();
    c.company = v[6].clone();
    c.title = v[7].clone();
    c.birthday = v[8].clone();
}
pub fn open(ui: &Ui, contact: Option<Contact>) {
    open_inner(ui, contact, None);
}
fn open_inner(ui: &Ui, contact: Option<Contact>, merge_other: Option<String>) {
    if let Some(e) = ui.editor.borrow().as_ref() {
        e.present();
        return;
    }
    let remote_book = ui::online_book(ui);
    if remote_book.as_ref().is_some_and(|b| !b.writable) {
        ui::toast(ui, "This address book is read-only or needs discovery.");
        return;
    }
    let remote_snapshot = contact.as_ref().and_then(|c| ui::online_snapshot(ui, &c.id));
    let online_saving = Rc::new(Cell::new(false));
    let is_new = contact.is_none();
    let original = contact.unwrap_or_default();
    let c = original.clone();
    let (dialog, card) = w::dialog(
        if merge_other.is_some() {
            "Review merged contact"
        } else if is_new {
            "New contact"
        } else {
            "Edit contact"
        },
        600,
    );
    dialog.set_default_height(720);
    dialog.set_transient_for(Some(&ui.window));
    dialog.set_application(ui.window.application().as_ref());
    *ui.editor.borrow_mut() = Some(dialog.clone());
    let body = w::vbox(8);
    let scroll = ui::scroll(&body);
    scroll.set_min_content_height(320);
    card.append(&scroll);
    let photo = Rc::new(RefCell::new(c.photo.clone()));
    let preview = w::hbox(0);
    preview.append(&ui::avatar(&c, 72));
    body.append(&preview);
    let photo_bar = w::hbox(4);
    let choose = w::icon_button("insert-image-symbolic", "Choose a photo (PNG or JPEG)");
    let remove = w::icon_button("edit-clear-symbolic", "Remove the photo (initials are shown instead)");
    photo_bar.append(&choose);
    photo_bar.append(&remove);
    body.append(&photo_bar);
    let p = photo.clone();
    let box_ = preview.clone();
    let mut no_photo = c.clone();
    no_photo.photo.clear();
    remove.connect_clicked(move |_| {
        p.borrow_mut().clear();
        ui::clear(&box_);
        box_.append(&ui::avatar(&no_photo, 72));
    });
    let p = photo.clone();
    let parent = dialog.clone();
    let preview = preview.clone();
    let c_photo = c.clone();
    let u = ui.clone();
    choose.connect_clicked(move |_| {
        let picker = gtk::FileDialog::builder().title("Choose contact photo").modal(true).build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("PNG and JPEG photos"));
        filter.add_mime_type("image/png");
        filter.add_mime_type("image/jpeg");
        picker.set_default_filter(Some(&filter));
        let p = p.clone();
        let preview = preview.clone();
        let c = c_photo.clone();
        let u = u.clone();
        picker.open(Some(&parent), None::<&gtk::gio::Cancellable>, move |result| {
            if let Ok(file) = result
                && let Some(path) = file.path()
            {
                let load = || -> anyhow::Result<Vec<u8>> {
                    if path.metadata()?.len() > 10 * 1024 * 1024 {
                        anyhow::bail!("Photos must be smaller than 10 MB.");
                    }
                    let bytes = std::fs::read(path)?;
                    let t = gdk::Texture::from_bytes(&glib::Bytes::from(&bytes[..]))?;
                    if t.width() > 8192 || t.height() > 8192 {
                        anyhow::bail!("Photo dimensions must be at most 8192 × 8192.");
                    }
                    Ok(model_photo(&bytes))
                };
                match load() {
                    Ok(bytes) => {
                        *p.borrow_mut() = bytes.clone();
                        let mut c = c.clone();
                        c.photo = bytes;
                        ui::clear(&preview);
                        preview.append(&ui::avatar(&c, 72));
                    }
                    Err(e) => ui::toast(&u, &e.to_string()),
                }
            }
        });
    });
    let entries: Vec<_> = NAMES
        .iter()
        .zip(singles(&c))
        .map(|(name, value)| {
            body.append(&ui::label(name, "dim"));
            let entry = gtk::Entry::new();
            entry.set_text(&value);
            if *name == "Birthday" {
                entry.set_placeholder_text(Some("1985-04-12, or --04-12 without a year"));
            }
            body.append(&entry);
            entry
        })
        .collect();
    let favorite = gtk::CheckButton::with_label("Favorite");
    favorite.set_active(c.favorite);
    body.append(&favorite);
    let emails = Fields::new("EMAIL", &c.emails, &body);
    let phones = Fields::new("PHONE", &c.phones, &body);
    let addresses = Addresses::new(&c.addresses, &body);
    let websites = Fields::new("WEBSITE", &c.websites, &body);
    body.append(&ui::label("NOTES", "contact-heading"));
    let notes = gtk::TextView::new();
    notes.set_wrap_mode(gtk::WrapMode::WordChar);
    notes.set_height_request(100);
    notes.buffer().set_text(&c.notes);
    notes.add_css_class("contact-input");
    body.append(&notes);
    let error = ui::label("", "error");
    error.set_visible(false);
    card.append(&error);
    let buttons = w::hbox(8);
    buttons.set_halign(gtk::Align::End);
    let cancel = ui::button("Cancel", "Close editor (Esc)");
    let save = ui::button("Save", "Save contact (Ctrl+S)");
    save.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&save);
    card.append(&buttons);
    let gather: Rc<dyn Fn() -> Contact> = Rc::new(move || {
        let mut c = c.clone();
        set_singles(&mut c, &entries.iter().map(|e| e.text().trim().to_string()).collect::<Vec<_>>());
        c.favorite = favorite.is_active();
        c.photo = photo.borrow().clone();
        c.emails = emails.values();
        c.phones = phones.values();
        c.addresses = addresses.values();
        c.websites = websites.values();
        c.notes = notes.buffer().text(&notes.buffer().start_iter(), &notes.buffer().end_iter(), false).to_string();
        c
    });
    let saved = Rc::new(Cell::new(false));
    let dirty = Rc::new(Cell::new(false));
    let prompt = Rc::new(Cell::new(false));
    let g = gather.clone();
    let baseline = original.clone();
    let s = saved.clone();
    let u = ui.clone();
    let d = dialog.clone();
    let asking = prompt.clone();
    let discard = dirty.clone();
    let writing = online_saving.clone();
    dialog.connect_close_request(move |_| {
        if writing.get() {
            return glib::Propagation::Stop;
        }
        if !s.get() && !discard.get() && g() != baseline {
            if !asking.replace(true) {
                let ask = gtk::AlertDialog::builder()
                    .message("Discard unsaved changes?")
                    .detail("Your changes have not been saved.")
                    .buttons(["Keep editing", "Discard"])
                    .cancel_button(0)
                    .default_button(0)
                    .modal(true)
                    .build();
                let d = d.clone();
                let disc = discard.clone();
                let asking = asking.clone();
                ask.choose(Some(&d.clone()), None::<&gtk::gio::Cancellable>, move |choice| {
                    asking.set(false);
                    if choice == Ok(1) {
                        disc.set(true);
                        d.close();
                    }
                });
            }
            return glib::Propagation::Stop;
        }
        u.editor.borrow_mut().take();
        glib::Propagation::Proceed
    });
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let u = ui.clone();
    let d = dialog.clone();
    let run: Rc<dyn Fn()> = Rc::new(move || {
        let c = gather();
        if !c.valid() {
            error.set_text("Enter a name, company, email, or phone number.");
            error.set_visible(true);
            return;
        }
        let mut c = c;
        // A birthday imported in a form Contacts doesn't read is kept until it's edited.
        if c.birthday != original.birthday || model::normalize_birthday(&c.birthday).is_some() {
            match model::normalize_birthday(&c.birthday) {
                Some(b) => c.birthday = b,
                None => {
                    error.set_text("Use a birthday like 1985-04-12, or --04-12 without a year.");
                    error.set_visible(true);
                    return;
                }
            }
        }
        if let Some(book) = remote_book.clone() {
            if online_saving.replace(true) {
                return;
            }
            let payload = crate::formats::vcard_write(&[c.clone()]);
            let expected = remote_snapshot.clone();
            let id = book.uid.clone();
            let before = crate::formats::vcard_write(&[original.clone()]);
            let payload = match expected.as_deref() {
                Some(raw) => match nyx_pim::contacts::merge_editor_changes(raw, &before, &payload) {
                    Ok(payload) => payload,
                    Err(failure) => {
                        online_saving.set(false);
                        error.set_text(&failure.to_string());
                        error.set_visible(true);
                        return;
                    }
                },
                None => payload,
            };
            let outgoing = payload.clone();
            let handle = gtk::gio::spawn_blocking(move || nyx_pim::contacts::save_vcard(&id, &outgoing, expected.as_deref()));
            let u = u.clone();
            let d = d.clone();
            let saved = saved.clone();
            let error = error.clone();
            let saving = online_saving.clone();
            glib::spawn_future_local(async move {
                let result = handle.await;
                saving.set(false);
                match result {
                    Ok(Ok(())) => {
                        ui::online_saved(&u, &book.uid, c, payload);
                        saved.set(true);
                        d.close();
                    }
                    Ok(Err(failure)) => {
                        error.set_text(&failure.to_string());
                        error.set_visible(true);
                    }
                    Err(_) => {
                        error.set_text("Could not save the contact. Your editor contents are retained.");
                        error.set_visible(true);
                    }
                }
            });
            return;
        }
        let mut book = u.book.borrow().clone();
        if let Some(other) = &merge_other {
            model::merge(&mut book, c.clone(), other);
        } else {
            if let Some(old) = book.contacts.iter_mut().find(|old| old.id == c.id) {
                // Only what was changed here: anything changed elsewhere while the editor
                // was open (by Nyx Assistant, or a list) stays.
                c = changes_onto(&original, &c, old);
                *old = c.clone();
            } else {
                book.contacts.push(c.clone());
            }
            if is_new {
                let section = u.section.borrow().clone();
                if let Some(list) = book.lists.iter_mut().find(|l| l.id == section) {
                    list.members.insert(c.id.clone());
                }
            }
        }
        if ui::commit(&u, book) {
            *u.selected.borrow_mut() = vec![c.id.clone()];
            u.showing_detail.set(true);
            ui::refresh(&u);
            saved.set(true);
            d.close();
        } else {
            error.set_text("Could not save the contact. Check the main window for details.");
            error.set_visible(true);
        }
    });
    let r = run.clone();
    save.connect_clicked(move |_| r());
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(move |_, key, _, mods| {
        if mods.contains(gdk::ModifierType::CONTROL_MASK) && matches!(key, gdk::Key::s | gdk::Key::S) {
            run();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    dialog.add_controller(keys);
    dialog.present();
}
/// The edits made from `before` to `after`, applied to `now` (the contact as it is now):
/// each field the editor changed takes the new value, the rest keep `now`'s.
fn changes_onto(before: &Contact, after: &Contact, now: &Contact) -> Contact {
    let mut out = now.clone();
    let mut single = singles(now);
    for ((slot, b), a) in single.iter_mut().zip(singles(before)).zip(singles(after)) {
        if a != b {
            *slot = a;
        }
    }
    set_singles(&mut out, &single);
    macro_rules! take {
        ($($f:ident),*) => {$(
            if after.$f != before.$f {
                out.$f = after.$f.clone();
            }
        )*};
    }
    take!(emails, phones, addresses, websites, notes, favorite, photo);
    out
}

/// A chosen photo, at most 512 px on its longer side (as JPEG, or PNG when it has
/// transparency), so it doesn't weigh down every save. The original if it can't be read.
fn model_photo(bytes: &[u8]) -> Vec<u8> {
    use gtk::gdk_pixbuf::{InterpType, Pixbuf};
    let Ok(pb) =
        Pixbuf::from_stream(&gtk::gio::MemoryInputStream::from_bytes(&glib::Bytes::from(bytes)), None::<&gtk::gio::Cancellable>)
    else {
        return bytes.to_vec();
    };
    let (w, h) = (pb.width(), pb.height());
    let scale = 512.0 / w.max(h) as f64;
    if scale >= 1.0 {
        return bytes.to_vec();
    }
    let Some(small) =
        pb.scale_simple(((w as f64 * scale) as i32).max(1), ((h as f64 * scale) as i32).max(1), InterpType::Bilinear)
    else {
        return bytes.to_vec();
    };
    let encoded =
        if small.has_alpha() { small.save_to_bufferv("png", &[]) } else { small.save_to_bufferv("jpeg", &[("quality", "90")]) };
    encoded.unwrap_or_else(|_| bytes.to_vec())
}

pub fn review_merge(ui: &Ui, a: &Contact, b: &Contact) {
    let (dialog, card) = w::dialog("Review duplicate", 580);
    crate::dialog_dismissal::add(&dialog, &card, "Cancel");
    dialog.set_default_height(640);
    card.append(&ui::label("Choose the primary card, then review conflicting values. Distinct emails, phones, addresses, websites, notes, and list memberships will be kept.","dim"));
    let primary = gtk::DropDown::from_strings(&[&format!("Primary: {}", a.name(false)), &format!("Primary: {}", b.name(false))]);
    card.append(&primary);
    let form = w::vbox(8);
    card.append(&ui::scroll(&form));
    let choices: Choices = Rc::new(RefCell::new(vec![]));
    let build: Rc<dyn Fn(bool)> = {
        let form = form.clone();
        let choices = choices.clone();
        let a = a.clone();
        let b = b.clone();
        Rc::new(move |reverse| {
            ui::clear(&form);
            choices.borrow_mut().clear();
            let (p, o) = if reverse { (&b, &a) } else { (&a, &b) };
            for (i, (x, y)) in singles(p).into_iter().zip(singles(o)).enumerate() {
                if !x.is_empty() && !y.is_empty() && x != y {
                    form.append(&ui::label(NAMES[i], "dim"));
                    let values = vec![x, y];
                    let dd = gtk::DropDown::from_strings(&values.iter().map(String::as_str).collect::<Vec<_>>());
                    form.append(&dd);
                    choices.borrow_mut().push((i, dd, values));
                }
            }
        })
    };
    build(false);
    let f = build.clone();
    primary.connect_selected_notify(move |p| f(p.selected() == 1));
    let photo_choice = gtk::DropDown::from_strings(&["Use primary photo", "Use other photo"]);
    card.append(&photo_choice);
    let button = ui::button("Continue to editor", "Review the combined contact before saving");
    card.append(&button);
    let u = ui.clone();
    let a = a.clone();
    let b = b.clone();
    let d = dialog.clone();
    button.connect_clicked(move |_| {
        let (mut p, o) = if primary.selected() == 1 { (b.clone(), a.clone()) } else { (a.clone(), b.clone()) };
        let mut single = singles(&p);
        for (x, y) in single.iter_mut().zip(singles(&o)) {
            if x.is_empty() {
                *x = y;
            }
        }
        for (i, dd, values) in choices.borrow().iter() {
            single[*i] = values[dd.selected() as usize].clone();
        }
        set_singles(&mut p, &single);
        model::union_fields(&mut p.emails, &o.emails);
        model::union_fields(&mut p.phones, &o.phones);
        model::union_fields(&mut p.addresses, &o.addresses);
        model::union_fields(&mut p.websites, &o.websites);
        if !o.notes.is_empty() && p.notes != o.notes {
            if !p.notes.is_empty() {
                p.notes.push_str("\n\n");
            }
            p.notes.push_str(&o.notes);
        }
        p.favorite |= o.favorite;
        if photo_choice.selected() == 1 || p.photo.is_empty() {
            p.photo = o.photo.clone();
        }
        for extra in &o.extra {
            if !p.extra.contains(extra) && !extra.to_uppercase().starts_with("UID:") {
                p.extra.push(extra.clone());
            }
        }
        d.close();
        open_inner(&u, Some(p), Some(o.id));
    });
    dialog.present();
}

pub fn settings(ui: &Ui) {
    let p = prefs::get();
    let body = w::vbox(0);
    body.add_css_class("settings-page");
    body.set_hexpand(true);
    let root = ui::scroll(&body);
    fn group(body: &gtk::Box, name: &str) -> gtk::Box {
        let outer = w::vbox(0);
        outer.add_css_class("settings-group");
        outer.set_widget_name(name);
        outer.append(&ui::label(&name.to_uppercase(), "group-title"));
        let rows = w::vbox(0);
        rows.add_css_class("group-list");
        outer.append(&rows);
        body.append(&outer);
        rows
    }
    let g = group(&body, "Contacts");
    for (title, desc, value, sort) in [
        ("Display last name first", "Show family names before given names.", p.last_first, false),
        ("Sort by last name", "Order the contact list by family name.", p.sort_last, true),
    ] {
        let u = ui.clone();
        let (r, _) = w::switch_row(title, desc, value, move |on| {
            prefs::update(|p| if sort { p.sort_last = on } else { p.last_first = on });
            ui::refresh(&u);
        });
        g.append(&r);
    }
    nyx_ui::settings_card::open_page(
        &ui.overlay,
        &ui.window,
        nyx_ui::settings_card::Spec {
            groups: &[("Contacts", "system-users-symbolic", "Names and sorting")],
            search_text: None,
            standard: Some(nyx_ui::settings_card::Standard {
                app: "contacts",
                about: nyx_ui::settings_card::About {
                    name: "Contacts",
                    icon: "io.github.design_nexus.nyx.Contacts",
                    version: env!("CARGO_PKG_VERSION"),
                    description: "An address book kept on this computer.",
                    licence: env!("CARGO_PKG_LICENSE"),
                    website: "https://github.com/design-nexus/nyx-contacts",
                    issues: "https://github.com/design-nexus/nyx-contacts/issues",
                    credits: &[],
                },
                shortcuts: ui::SHORTCUTS,
                reduce_motion: || prefs::get().reduce_motion,
                set_reduce_motion: |on| prefs::update(|p| p.reduce_motion = on),
            }),
        },
        root.upcast_ref(),
        &body,
    );
}
