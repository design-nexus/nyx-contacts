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

/// The editor in the details pane: whether it holds unsaved changes, and how to leave it.
pub struct Editing {
    pub dirty: Rc<dyn Fn() -> bool>,
    pub close: Rc<dyn Fn()>,
}

/// The labels offered for each kind of value; any other can be typed.
fn labels_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "phone" => &["cell", "home", "work", "main", "fax", "pager", "other"],
        "website" => &["home", "work", "blog", "profile", "other"],
        _ => &["home", "work", "school", "other"],
    }
}

/// A group's heading over its card.
fn heading(body: &gtk::Box, title: &str) {
    let l = w::label(title, "edit-heading");
    l.set_xalign(0.0);
    body.append(&l);
}

fn card(body: &gtk::Box) -> gtk::Box {
    let c = w::vbox(0);
    c.add_css_class("field-card");
    body.append(&c);
    c
}

/// A label shown as a dropdown: the usual ones, or one typed in.
fn label_picker(current: &str, labels: &[&'static str]) -> (gtk::MenuButton, Rc<RefCell<String>>) {
    let value = Rc::new(RefCell::new(current.to_string()));
    let button = gtk::MenuButton::new();
    button.add_css_class("label-pick");
    button.set_valign(gtk::Align::Center);
    let face = w::hbox(6);
    let text = gtk::Label::new(Some(if current.is_empty() { "label" } else { current }));
    text.set_hexpand(true);
    text.set_xalign(0.0);
    text.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.set_width_chars(7);
    text.set_max_width_chars(9);
    face.append(&text);
    face.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
    button.set_child(Some(&face));
    let pop = gtk::Popover::new();
    pop.add_css_class("menu-popover");
    let list = w::vbox(1);
    for &l in labels {
        let b = gtk::Button::with_label(l);
        b.add_css_class("flat");
        b.add_css_class("menu-item");
        let (v, t, p) = (value.clone(), text.clone(), pop.clone());
        b.connect_clicked(move |_| {
            *v.borrow_mut() = l.to_string();
            t.set_text(l);
            p.popdown();
        });
        list.append(&b);
    }
    let custom = gtk::Entry::new();
    custom.set_placeholder_text(Some("Another label"));
    let (v, t, p) = (value.clone(), text.clone(), pop.clone());
    custom.connect_activate(move |e| {
        let s = e.text().trim().to_string();
        if !s.is_empty() {
            *v.borrow_mut() = s.clone();
            t.set_text(&s);
        }
        p.popdown();
    });
    list.append(&custom);
    pop.set_child(Some(&list));
    button.set_popover(Some(&pop));
    (button, value)
}

/// A field's rows: the row, its label, its value.
type FieldRows = Rc<RefCell<Vec<(gtk::Box, Rc<RefCell<String>>, gtk::Entry)>>>;

/// Labelled single-line values (emails, phones, websites) in a card, with "Add …" at its foot.
struct Fields {
    rows: FieldRows,
}

impl Fields {
    /// `blank`: start with an empty row to type into (a new contact's first email).
    fn new(card: &gtk::Box, kind: &'static str, values: &[Field], add_text: &str, hint: &'static str, blank: bool) -> Self {
        let rows: FieldRows = Rc::new(RefCell::new(vec![]));
        let list = w::vbox(0);
        card.append(&list);
        let labels = labels_for(kind);
        for f in values {
            add_field(&list, &rows, f, labels, hint);
        }
        if blank && values.is_empty() {
            add_field(&list, &rows, &Field { label: labels[0].into(), value: String::new() }, labels, hint);
        }
        let add = add_row(add_text);
        card.append(&add);
        let (l, r) = (list.clone(), rows.clone());
        add.connect_clicked(move |_| {
            add_field(&l, &r, &Field { label: labels[0].into(), value: String::new() }, labels, hint);
            if let Some((_, _, e)) = r.borrow().last() {
                e.grab_focus();
            }
        });
        Self { rows }
    }

    fn values(&self) -> Vec<Field> {
        self.rows
            .borrow()
            .iter()
            .filter(|(b, _, v)| b.parent().is_some() && !v.text().trim().is_empty())
            .map(|(_, l, v)| Field { label: l.borrow().trim().to_string(), value: v.text().trim().to_string() })
            .collect()
    }
}

fn add_row(text: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("flat");
    b.add_css_class("field-add");
    let face = w::hbox(8);
    face.append(&gtk::Image::from_icon_name("list-add-symbolic"));
    face.append(&gtk::Label::new(Some(text)));
    b.set_child(Some(&face));
    b
}

fn remove_button(row: &gtk::Box, root: &gtk::Box, tip: &str) -> gtk::Button {
    let remove = w::bar_button("window-close-symbolic", tip);
    remove.add_css_class("field-remove");
    remove.set_valign(gtk::Align::Center);
    let (r, b) = (root.clone(), row.clone());
    remove.connect_clicked(move |_| r.remove(&b));
    remove
}

fn add_field(root: &gtk::Box, rows: &FieldRows, f: &Field, labels: &[&'static str], hint: &str) {
    let row = w::hbox(8);
    row.add_css_class("edit-row");
    let (pick, label) = label_picker(&f.label, labels);
    row.append(&pick);
    let value = gtk::Entry::new();
    value.set_hexpand(true);
    value.set_width_chars(8);
    value.set_text(&f.value);
    value.set_placeholder_text(Some(hint));
    row.append(&value);
    row.append(&remove_button(&row, root, "Remove"));
    root.append(&row);
    rows.borrow_mut().push((row, label, value));
}

/// The address editor's rows: the row, its label, the five visible parts, and the two
/// vCard parts (PO box, extended) that are kept as they were.
type AddressRows = Rc<RefCell<Vec<(gtk::Box, Rc<RefCell<String>>, [gtk::Entry; 5], (String, String))>>>;

struct Addresses {
    rows: AddressRows,
}

impl Addresses {
    fn new(card: &gtk::Box, values: &[Field]) -> Self {
        let rows: AddressRows = Rc::new(RefCell::new(vec![]));
        let list = w::vbox(0);
        card.append(&list);
        for f in values {
            add_address(&list, &rows, f);
        }
        let add = add_row("Add an address");
        card.append(&add);
        let (l, r) = (list.clone(), rows.clone());
        add.connect_clicked(move |_| add_address(&l, &r, &Field { label: "home".into(), value: String::new() }));
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
                (label.borrow().trim().to_string(), a)
            })
            .filter(|(_, a)| !a.is_empty())
            .map(|(label, a)| Field { label, value: a.value() })
            .collect()
    }
}

fn add_address(root: &gtk::Box, rows: &AddressRows, f: &Field) {
    let a = model::Address::parse(&f.value);
    let row = w::hbox(8);
    row.add_css_class("edit-row");
    let (pick, label) = label_picker(&f.label, &["home", "work", "school", "other"]);
    pick.set_valign(gtk::Align::Start);
    row.append(&pick);
    let grid = gtk::Grid::builder().row_spacing(4).column_spacing(4).hexpand(true).build();
    let entry = |text: &str, hint: &str| {
        let e = gtk::Entry::new();
        e.set_text(text);
        e.set_placeholder_text(Some(hint));
        e.set_hexpand(true);
        e.set_width_chars(6);
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
    let remove = remove_button(&row, root, "Remove the address");
    remove.set_valign(gtk::Align::Start);
    row.append(&remove);
    root.append(&row);
    rows.borrow_mut().push((row, label, parts, (a.po_box, a.extended)));
}

const NAMES: &[&str] =
    &["First name", "Last name", "Middle name", "Prefix", "Suffix", "Shown as", "Company", "Job title", "Birthday"];
pub fn singles(c: &Contact) -> Vec<String> {
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
pub fn set_singles(c: &mut Contact, v: &[String]) {
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
/// A birthday picker: a date, with or without its year, or none; and what clears it.
fn birthday_picker(value: Rc<RefCell<String>>) -> (gtk::MenuButton, Rc<dyn Fn()>) {
    let button = gtk::MenuButton::new();
    button.add_css_class("date-pick");
    let face = w::hbox(8);
    face.append(&gtk::Image::from_icon_name("x-office-calendar-symbolic"));
    let text = gtk::Label::new(None);
    text.set_xalign(0.0);
    text.set_hexpand(true);
    face.append(&text);
    button.set_child(Some(&face));
    let show = {
        let text = text.clone();
        move |v: &str| {
            if v.is_empty() {
                text.set_text("Add a birthday");
                text.add_css_class("dim");
            } else {
                text.set_text(&ui::birthday_text(v));
                text.remove_css_class("dim");
            }
        }
    };
    show(&value.borrow());
    let pop = gtk::Popover::new();
    let col = w::vbox(8);
    let calendar = gtk::Calendar::new();
    let today = glib::DateTime::now_local().ok();
    let (m, d) = model::birthday_md(&value.borrow()).unwrap_or_else(|| {
        today.as_ref().map(|t| (t.month() as u32, t.day_of_month() as u32)).unwrap_or((1, 1))
    });
    let year = value.borrow().get(..4).filter(|_| !value.borrow().starts_with("--")).and_then(|y| y.parse::<i32>().ok());
    if let Ok(dt) = glib::DateTime::from_local(year.unwrap_or(1990), m as i32, d as i32, 0, 0, 0.0) {
        calendar.select_day(&dt);
    }
    col.append(&calendar);
    let no_year = gtk::CheckButton::with_label("No year");
    no_year.set_active(!value.borrow().is_empty() && year.is_none());
    col.append(&no_year);
    let bar = w::hbox(8);
    let clear = gtk::Button::with_label("No birthday");
    clear.add_css_class("flat");
    let done = gtk::Button::with_label("Done");
    done.add_css_class("suggested-action");
    let spacer = w::hbox(0);
    spacer.set_hexpand(true);
    bar.append(&clear);
    bar.append(&spacer);
    bar.append(&done);
    col.append(&bar);
    pop.set_child(Some(&col));
    button.set_popover(Some(&pop));
    let set = {
        let (value, show, calendar, no_year) = (value.clone(), show.clone(), calendar.clone(), no_year.clone());
        move || {
            let d = calendar.date();
            let v = if no_year.is_active() {
                format!("--{:02}-{:02}", d.month(), d.day_of_month())
            } else {
                format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day_of_month())
            };
            *value.borrow_mut() = v.clone();
            show(&v);
        }
    };
    {
        let (set, p) = (set.clone(), pop.clone());
        done.connect_clicked(move |_| {
            set();
            p.popdown();
        });
    }
    calendar.connect_day_selected(move |_| set());
    let wipe: Rc<dyn Fn()> = {
        let v = value.clone();
        Rc::new(move || {
            v.borrow_mut().clear();
            show("");
        })
    };
    let (p, w2) = (pop.clone(), wipe.clone());
    clear.connect_clicked(move |_| {
        w2();
        p.popdown();
    });
    (button, wipe)
}

pub fn open(ui: &Ui, contact: Option<Contact>) {
    if ui.editing.borrow().is_some() {
        return;
    }
    let remote_book = ui::online_book(ui);
    if remote_book.as_ref().is_some_and(|b| !b.writable) {
        ui::toast(ui, "This address book is read only. Copy someone to On this computer to change them.");
        return;
    }
    let remote_snapshot = contact.as_ref().and_then(|c| ui::online_snapshot(ui, &c.id));
    let online_saving = Rc::new(Cell::new(false));
    let is_new = contact.is_none();
    let original = contact.unwrap_or_default();
    let c = original.clone();
    let body = ui.details.clone();
    ui::clear(&body);
    body.add_css_class("editing");

    // The photo and the names first.
    let head = w::hbox(18);
    head.add_css_class("edit-head");
    let photo = Rc::new(RefCell::new(c.photo.clone()));
    let photo_box = gtk::Overlay::new();
    let preview = w::hbox(0);
    preview.append(&ui::avatar(&c, 88));
    photo_box.set_child(Some(&preview));
    let camera = w::bar_button("camera-photo-symbolic", "Change the photo");
    camera.add_css_class("photo-pick");
    camera.set_halign(gtk::Align::End);
    camera.set_valign(gtk::Align::End);
    photo_box.add_overlay(&camera);
    photo_box.set_valign(gtk::Align::Start);
    head.append(&photo_box);
    let show: Rc<dyn Fn(Vec<u8>)> = {
        let (p, prev, c) = (photo.clone(), preview.clone(), c.clone());
        Rc::new(move |bytes: Vec<u8>| {
            *p.borrow_mut() = bytes.clone();
            let mut c = c.clone();
            c.photo = bytes;
            ui::clear(&prev);
            prev.append(&ui::avatar(&c, 88));
        })
    };
    let choose: Rc<dyn Fn()> = {
        let (show, u) = (show.clone(), ui.clone());
        Rc::new(move || {
            let picker = gtk::FileDialog::builder().title("Choose a photo").modal(true).build();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("PNG and JPEG photos"));
            filter.add_mime_type("image/png");
            filter.add_mime_type("image/jpeg");
            picker.set_default_filter(Some(&filter));
            let (show, u2) = (show.clone(), u.clone());
            picker.open(Some(&u.window), None::<&gtk::gio::Cancellable>, move |result| {
                let Some(path) = result.ok().and_then(|f| f.path()) else { return };
                let load = || -> anyhow::Result<Vec<u8>> {
                    if path.metadata()?.len() > 10 * 1024 * 1024 {
                        anyhow::bail!("Photos must be smaller than 10 MB.");
                    }
                    let bytes = std::fs::read(&path)?;
                    let t = gdk::Texture::from_bytes(&glib::Bytes::from(&bytes[..]))?;
                    if t.width() > 8192 || t.height() > 8192 {
                        anyhow::bail!("Photos must be at most 8192 × 8192.");
                    }
                    Ok(model_photo(&bytes))
                };
                match load() {
                    Ok(bytes) => show(bytes),
                    Err(e) => ui::toast(&u2, &e.to_string()),
                }
            });
        })
    };
    {
        let (photo, show) = (photo.clone(), show.clone());
        camera.connect_clicked(move |b| {
            let (choose, show) = (choose.clone(), show.clone());
            crate::menu::below(
                b,
                vec![
                    crate::menu::item("camera-photo-symbolic", "Choose a photo…", move || choose()),
                    crate::menu::item("user-trash-symbolic", "Remove the photo", move || show(Vec::new()))
                        .enabled(!photo.borrow().is_empty()),
                ],
            );
        });
    }
    let names = gtk::Grid::builder().row_spacing(8).column_spacing(8).hexpand(true).build();
    let s = singles(&c);
    let name_entry = |i: usize| {
        let e = gtk::Entry::new();
        e.set_text(&s[i]);
        e.set_placeholder_text(Some(NAMES[i]));
        e.set_hexpand(true);
        e.set_width_chars(8);
        e.update_property(&[gtk::accessible::Property::Label(NAMES[i])]);
        e
    };
    // First, last, company and job title show; middle, prefix, suffix and "shown as" fold away.
    let entries: Vec<gtk::Entry> = (0..8).map(name_entry).collect();
    names.attach(&entries[0], 0, 0, 1, 1);
    names.attach(&entries[1], 1, 0, 1, 1);
    names.attach(&entries[6], 0, 1, 1, 1);
    names.attach(&entries[7], 1, 1, 1, 1);
    let more = gtk::Expander::new(None);
    let more_label = gtk::Label::new(Some("More name fields (middle, prefix, suffix, shown as)"));
    more_label.set_wrap(true);
    more_label.set_xalign(0.0);
    more.set_label_widget(Some(&more_label));
    more.add_css_class("more-names");
    let extra = gtk::Grid::builder().row_spacing(8).column_spacing(8).margin_top(8).build();
    extra.attach(&entries[2], 0, 0, 1, 1);
    extra.attach(&entries[3], 1, 0, 1, 1);
    extra.attach(&entries[4], 0, 1, 1, 1);
    extra.attach(&entries[5], 1, 1, 1, 1);
    more.set_child(Some(&extra));
    more.set_expanded((2..6).any(|i| !s[i].is_empty()));
    names.attach(&more, 0, 2, 2, 1);
    head.append(&names);
    body.append(&head);

    heading(&body, "Email");
    let emails = Fields::new(&card(&body), "email", &c.emails, "Add an email", "name@example.com", is_new);
    heading(&body, "Phone");
    let phones = Fields::new(&card(&body), "phone", &c.phones, "Add a phone", "+1 555 123 4567", false);
    heading(&body, "More");
    let more_card = card(&body);
    let birthday = Rc::new(RefCell::new(c.birthday.clone()));
    let brow = w::hbox(8);
    brow.add_css_class("edit-row");
    let blabel = w::label("birthday", "label-static");
    blabel.set_xalign(0.0);
    blabel.set_valign(gtk::Align::Center);
    brow.append(&blabel);
    let (bpick, clear_birthday) = birthday_picker(birthday.clone());
    bpick.set_hexpand(true);
    brow.append(&bpick);
    let bremove = w::bar_button("window-close-symbolic", "Remove the birthday");
    bremove.add_css_class("field-remove");
    bremove.set_valign(gtk::Align::Center);
    bremove.connect_clicked(move |_| clear_birthday());
    brow.append(&bremove);
    more_card.append(&brow);
    let addresses = Addresses::new(&more_card, &c.addresses);
    let websites = Fields::new(&more_card, "website", &c.websites, "Add a website", "example.com", false);

    // Lists, as chips to switch on and off (this computer's book only).
    let list_chips: Rc<RefCell<Vec<(String, gtk::ToggleButton)>>> = Rc::new(RefCell::new(vec![]));
    if remote_book.is_none() {
        let lists = ui.book.borrow().lists.clone();
        if !lists.is_empty() {
            heading(&body, "Lists");
            let flow = gtk::FlowBox::new();
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_max_children_per_line(8);
            flow.add_css_class("list-chips");
            let section = ui.section.borrow().clone();
            for l in lists {
                let t = gtk::ToggleButton::with_label(&l.name);
                t.add_css_class("chip");
                t.set_active(l.members.contains(&c.id) || (is_new && l.id == section));
                flow.insert(&t, -1);
                if is_new && l.id == section {
                    let hint = w::label(&format!("Added because {} is open", l.name), "chip-hint");
                    hint.set_valign(gtk::Align::Center);
                    flow.insert(&hint, -1);
                }
                list_chips.borrow_mut().push((l.id.clone(), t));
            }
            body.append(&flow);
        }
    }
    heading(&body, "Notes");
    let notes = gtk::TextView::new();
    notes.set_wrap_mode(gtk::WrapMode::WordChar);
    notes.set_height_request(90);
    notes.buffer().set_text(&c.notes);
    notes.add_css_class("contact-input");
    notes.update_property(&[gtk::accessible::Property::Label("Notes")]);
    body.append(&notes);

    // The bar under the form: where it saves, then Delete, Cancel and Save.
    let bar = ui.edit_bar.clone();
    ui::clear(&bar);
    let status = w::label(
        &match (&remote_book, is_new) {
            (Some(b), true) => format!("New contact in {}", b.label()),
            (Some(b), false) => format!("Editing · {}", b.label()),
            (None, true) => "New contact on this computer".to_string(),
            (None, false) => "Editing · On this computer".to_string(),
        },
        "edit-status",
    );
    status.set_hexpand(true);
    status.set_xalign(0.0);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    bar.append(&status);
    if !is_new {
        let delete = gtk::Button::new();
        let face = w::hbox(6);
        face.append(&gtk::Image::from_icon_name("user-trash-symbolic"));
        face.append(&gtk::Label::new(Some("Delete")));
        delete.set_child(Some(&face));
        delete.add_css_class("danger-text-button");
        let u = ui.clone();
        delete.connect_clicked(move |_| {
            let close = u.editing.borrow().as_ref().map(|e| e.close.clone());
            if let Some(close) = close {
                close();
            }
            ui::delete(&u);
        });
        bar.append(&delete);
    }
    let cancel = ui::button("Cancel", "Leave without saving (Esc)");
    cancel.add_css_class("flat");
    let save = ui::button(if is_new { "Add contact" } else { "Save" }, "Save (Ctrl+S)");
    save.add_css_class("suggested-action");
    bar.append(&cancel);
    bar.append(&save);
    let mut child = bar.first_child();
    while let Some(c) = child {
        c.set_valign(gtk::Align::Center);
        child = c.next_sibling();
    }

    let gather: Rc<dyn Fn() -> Contact> = {
        let c = c.clone();
        Rc::new(move || {
            let mut c = c.clone();
            let mut v: Vec<String> = entries.iter().map(|e| e.text().trim().to_string()).collect();
            v.push(birthday.borrow().clone());
            set_singles(&mut c, &v);
            c.photo = photo.borrow().clone();
            c.emails = emails.values();
            c.phones = phones.values();
            c.addresses = addresses.values();
            c.websites = websites.values();
            c.notes = notes.buffer().text(&notes.buffer().start_iter(), &notes.buffer().end_iter(), false).to_string();
            c
        })
    };
    let chosen_lists = {
        let chips = list_chips.clone();
        move || chips.borrow().iter().filter(|(_, t)| t.is_active()).map(|(id, _)| id.clone()).collect::<Vec<String>>()
    };
    let lists_before: Vec<String> = chosen_lists();
    let dirty: Rc<dyn Fn() -> bool> = {
        let (g, baseline, chosen) = (gather.clone(), original.clone(), chosen_lists.clone());
        Rc::new(move || g() != baseline || chosen() != lists_before)
    };
    let close: Rc<dyn Fn()> = {
        let u = ui.clone();
        Rc::new(move || {
            u.editing.borrow_mut().take();
            u.details.remove_css_class("editing");
            u.edit_bar.set_visible(false);
            ui::set_editing(&u, false);
            ui::details(&u);
            ui::layout(&u);
        })
    };
    *ui.editing.borrow_mut() = Some(Editing { dirty: dirty.clone(), close: close.clone() });
    ui::set_editing(ui, true);
    bar.set_visible(true);
    {
        let (close, dirty, u) = (close.clone(), dirty.clone(), ui.clone());
        cancel.connect_clicked(move |_| ask_discard(&u, dirty.clone(), close.clone()));
    }
    let run: Rc<dyn Fn()> = {
        let (u, close) = (ui.clone(), close.clone());
        Rc::new(move || {
            let c = gather();
            if !c.valid() {
                status.set_text("Enter a name, company, email, or phone number.");
                status.add_css_class("error");
                return;
            }
            let mut c = c;
            // A birthday imported in a form Contacts doesn't read is kept until it's edited.
            if c.birthday != original.birthday || model::normalize_birthday(&c.birthday).is_some() {
                match model::normalize_birthday(&c.birthday) {
                    Some(b) => c.birthday = b,
                    None => {
                        status.set_text("That birthday can't be read; pick it again.");
                        status.add_css_class("error");
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
                            status.set_text(&failure.to_string());
                            status.add_css_class("error");
                            return;
                        }
                    },
                    None => payload,
                };
                let outgoing = payload.clone();
                let handle = gtk::gio::spawn_blocking(move || nyx_pim::contacts::save_vcard(&id, &outgoing, expected.as_deref()));
                let (u, close, status, saving) = (u.clone(), close.clone(), status.clone(), online_saving.clone());
                glib::spawn_future_local(async move {
                    let result = handle.await;
                    saving.set(false);
                    match result {
                        Ok(Ok(())) => {
                            close();
                            ui::online_saved(&u, &book.uid, c, payload);
                        }
                        Ok(Err(failure)) => {
                            status.set_text(&failure.to_string());
                            status.add_css_class("error");
                        }
                        Err(_) => {
                            status.set_text("Could not save the contact. What you typed is kept.");
                            status.add_css_class("error");
                        }
                    }
                });
                return;
            }
            let mut book = u.book.borrow().clone();
            if let Some(old) = book.contacts.iter_mut().find(|old| old.id == c.id) {
                // Only what was changed here: anything changed elsewhere while the editor
                // was open (by Nyx Assistant, or a list) stays.
                c = changes_onto(&original, &c, old);
                *old = c.clone();
            } else {
                book.contacts.push(c.clone());
            }
            let chosen = chosen_lists();
            for l in &mut book.lists {
                if chosen.contains(&l.id) {
                    l.members.insert(c.id.clone());
                } else if !list_chips.borrow().is_empty() {
                    l.members.remove(&c.id);
                }
            }
            if ui::commit(&u, book) {
                *u.selected.borrow_mut() = vec![c.id.clone()];
                u.showing_detail.set(true);
                close();
                ui::refresh(&u);
            } else {
                status.set_text("Could not save the contact. Check the window for details.");
                status.add_css_class("error");
            }
        })
    };
    let r = run.clone();
    save.connect_clicked(move |_| r());
    let keys = gtk::EventControllerKey::new();
    let (u, d2, c2) = (ui.clone(), dirty.clone(), close.clone());
    keys.connect_key_pressed(move |_, key, _, mods| {
        if mods.contains(gdk::ModifierType::CONTROL_MASK) && matches!(key, gdk::Key::s | gdk::Key::S) {
            run();
            glib::Propagation::Stop
        } else if key == gdk::Key::Escape {
            ask_discard(&u, d2.clone(), c2.clone());
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    body.add_controller(keys);
    ui::layout(ui);
    if let Some(first) = body.first_child() {
        first.child_focus(gtk::DirectionType::TabForward);
    }
}

/// Leaves the editor, asking first when something would be lost.
pub fn ask_discard(ui: &Ui, dirty: Rc<dyn Fn() -> bool>, close: Rc<dyn Fn()>) {
    if !dirty() {
        close();
        return;
    }
    let ask = gtk::AlertDialog::builder()
        .message("Discard unsaved changes?")
        .detail("What you changed here hasn't been saved.")
        .buttons(["Keep editing", "Discard"])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    ask.choose(Some(&ui.window), None::<&gtk::gio::Cancellable>, move |choice| {
        if choice == Ok(1) {
            close();
        }
    });
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
    // Sources: show, hide or rename each, and remove a leftover import.
    let g = group(&body, "Sources");
    for row in ui::source_rows(ui) {
        let (u, key) = (ui.clone(), row.key.clone());
        let shown = !prefs::get().hidden_sources.contains(&row.key);
        let (r, _) = w::switch_row(&row.title, &row.desc, shown, move |on| ui::set_source_shown(&u, &key, on));
        // A leftover import can go: Remove, beside its switch.
        if row.removable {
            let rm = gtk::Button::with_label("Remove");
            rm.set_valign(gtk::Align::Center);
            rm.set_tooltip_text(Some("Remove it and its cards from Contacts"));
            let (u, key, line) = (ui.clone(), row.key.clone(), r.clone());
            rm.connect_clicked(move |_| {
                let line = line.clone();
                ui::remove_source(&u, &key, move || line.set_visible(false));
            });
            r.insert_child_after(&rm, r.first_child().as_ref());
        }
        if row.key != "local" {
            let rename = gtk::Button::with_label("Rename");
            rename.set_valign(gtk::Align::Center);
            rename.set_tooltip_text(Some("Change the name Contacts shows for it"));
            let title = r.first_child().and_then(|t| t.first_child()).and_downcast::<gtk::Label>();
            let (u, key) = (ui.clone(), row.key.clone());
            rename.connect_clicked(move |_| {
                let title = title.clone();
                ui::rename_source(&u, &key, move |name| {
                    if let Some(t) = &title {
                        t.set_text(&name);
                    }
                });
            });
            r.insert_child_after(&rename, r.first_child().as_ref());
        }
        g.append(&r);
    }
    nyx_ui::settings_card::open_page(
        &ui.overlay,
        &ui.window,
        nyx_ui::settings_card::Spec {
            groups: &[
                ("Contacts", "system-users-symbolic", "Names and sorting"),
                ("Sources", "folder-remote-symbolic", "Which address books show"),
            ],
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
