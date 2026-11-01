use crate::{
    editor,
    model::{self, Book, Contact, Store},
    prefs, theme, transfer,
};
use gtk::{gdk, glib, prelude::*};
use nyx_ui::{chrome, widgets as w};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
pub type Ui = Rc<State>;
pub struct State {
    pub window: gtk::ApplicationWindow,
    pub overlay: gtk::Overlay,
    pub book: RefCell<Book>,
    pub store: RefCell<Store>,
    pub section: RefCell<String>,
    pub list: gtk::ListBox,
    pub details: gtk::Box,
    pub search: gtk::SearchEntry,
    nav: gtk::Box,
    /// The sidebar's top, holding its collapse toggle.
    head: gtk::Box,
    /// The header's subtitle: the section you're in.
    subtitle: gtk::Label,
    sidebar: gtk::Box,
    pub list_scroll: gtk::ScrolledWindow,
    detail_scroll: gtk::ScrolledWindow,
    status: gtk::Label,
    toast_box: gtk::Box,
    toast_serial: Cell<u64>,
    pub selected: RefCell<Vec<String>>,
    pub narrow: Cell<bool>,
    pub showing_detail: Cell<bool>,
    pub rebuilding: Cell<bool>,
    pub editor: RefCell<Option<gtk::Window>>,
    eds_books: RefCell<Vec<nyx_pim::contacts::AddressBook>>,
    eds_contacts: RefCell<Vec<Contact>>,
    eds_ready: Cell<bool>,
    eds_loading: Cell<bool>,
    /// Ids of contacts sharing an email or phone with another, worked out once per change
    /// to the book.
    dup_ids: RefCell<Option<std::collections::HashSet<String>>>,
}
thread_local! {static UI:RefCell<Option<Ui>>=const{RefCell::new(None)};}
/// The open window's state, once `present` has run (the control interface uses it).
pub fn current() -> Option<Ui> {
    UI.with(|u| u.borrow().clone())
}
pub fn present(app: &gtk::Application, section: Option<&str>) -> anyhow::Result<Ui> {
    if let Some(ui) = UI.with(|u| u.borrow().clone()) {
        if let Some(s) = section {
            navigate(&ui, s);
        }
        ui.window.present();
        return Ok(ui);
    }
    let mut store = Store::open(&nyx_core::paths::data_dir().join("contacts.db"))?;
    let book = store.load()?;
    theme::install();
    install_icons();
    let window = gtk::ApplicationWindow::builder().application(app).default_width(1120).default_height(800).build();
    window.add_css_class("contacts-window");
    window.set_icon_name(Some("io.github.design_nexus.nyx.Contacts"));
    window.set_titlebar(Some(&w::hbox(0)));
    let overlay = gtk::Overlay::new();
    let frame = w::vbox(0);
    frame.add_css_class("window-frame");
    let body = w::hbox(0);
    body.set_vexpand(true);
    frame.append(&body);
    overlay.set_child(Some(&frame));
    window.set_child(Some(&overlay));
    let sidebar = w::vbox(0);
    sidebar.set_hexpand(false);
    sidebar.add_css_class("settings-navigation");
    // The sidebar's own toggle, at its top right (centred on the narrow rail).
    let head = w::hbox(0);
    head.add_css_class("nav-top");
    let collapse = w::bar_button("sidebar-show-symbolic", "Collapse the sidebar (Ctrl+B)");
    collapse.add_css_class("nav-toggle");
    collapse.set_hexpand(true);
    collapse.set_halign(gtk::Align::End);
    head.append(&collapse);
    sidebar.append(&head);
    let nav = w::vbox(0);
    nav.set_hexpand(false);
    let nav_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(false)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .child(&nav)
        .build();
    sidebar.append(&nav_scroll);
    // The sidebar's foot: how many contacts are shown and selected.
    let foot = w::hbox(0);
    foot.add_css_class("nav-foot");
    let status = w::label("", "status-readout");
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    status.set_hexpand(true);
    status.set_xalign(0.0);
    status.add_css_class("compact-hide");
    foot.append(&status);
    sidebar.append(&foot);
    body.append(&sidebar);
    let page = w::vbox(0);
    page.add_css_class("settings-content");
    page.set_hexpand(true);
    body.append(&page);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search contacts  (Ctrl+F)"));
    search.update_property(&[gtk::accessible::Property::Label("Search contacts")]);
    search.add_css_class("bar-search");
    search.set_width_chars(40);
    let new = w::bar_button("contact-new-symbolic", "New contact (Ctrl+N)");
    new.add_css_class("accent-action");
    let edit = w::bar_button("document-edit-symbolic", "Edit selected contact");
    let menu = gtk::MenuButton::builder().icon_name("view-more-symbolic").tooltip_text("Contact actions").build();
    let pop = gtk::Popover::new();
    pop.add_css_class("menu-popover");
    let actions = w::vbox(4);
    pop.set_child(Some(&actions));
    menu.add_css_class("bar-button");
    menu.set_popover(Some(&pop));
    menu.set_valign(gtk::Align::Center);
    // The header: the app and the section you're in, search, and the contact actions.
    let header = chrome::app_header(&window, "x-office-address-book-symbolic", "Contacts");
    header.center.append(&search);
    header.end.append(&edit);
    header.end.append(&menu);
    header.end.append(&new);
    header.end.append(&chrome::settings_button(settings));
    frame.prepend(&header.root);
    let panes = w::hbox(0);
    panes.set_vexpand(true);
    page.append(&panes);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Multiple);
    list.add_css_class("contact-list");
    let list_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&list)
        .build();
    list_scroll.set_width_request(280);
    panes.append(&list_scroll);
    let details = w::vbox(12);
    details.add_css_class("contact-details");
    let detail_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&details)
        .build();
    panes.append(&detail_scroll);
    let toast_box = w::hbox(10);
    toast_box.add_css_class("toast");
    toast_box.set_halign(gtk::Align::Center);
    toast_box.set_valign(gtk::Align::End);
    toast_box.set_margin_bottom(24);
    toast_box.set_visible(false);
    overlay.add_overlay(&toast_box);
    let start = section.map(str::to_string).unwrap_or_else(|| nyx_theme::start_page("all", &prefs::get().last_section));
    let ui = Rc::new(State {
        window,
        overlay,
        book: RefCell::new(book),
        store: RefCell::new(store),
        section: RefCell::new(start),
        list,
        details,
        search,
        nav,
        head,
        subtitle: header.subtitle.clone(),
        sidebar,
        list_scroll,
        detail_scroll,
        status,
        toast_box,
        toast_serial: Cell::new(0),
        selected: RefCell::new(vec![]),
        narrow: Cell::new(false),
        showing_detail: Cell::new(false),
        rebuilding: Cell::new(false),
        editor: RefCell::new(None),
        eds_books: RefCell::new(vec![]),
        eds_contacts: RefCell::new(vec![]),
        eds_ready: Cell::new(false),
        eds_loading: Cell::new(false),
        dup_ids: RefCell::new(None),
    });
    UI.with(|u| *u.borrow_mut() = Some(ui.clone()));
    let u = ui.clone();
    new.connect_clicked(move |_| editor::open(&u, None));
    let u = ui.clone();
    edit.connect_clicked(move |_| edit_selected(&u));
    let u = ui.clone();
    collapse.connect_clicked(move |_| {
        prefs::update(|p| p.sidebar_collapsed = !p.sidebar_collapsed);
        layout(&u);
    });
    let u = ui.clone();
    ui.search.connect_search_changed(move |_| refresh(&u));
    let u = ui.clone();
    ui.list.connect_selected_rows_changed(move |list| {
        if u.rebuilding.get() {
            return;
        }
        *u.selected.borrow_mut() = list.selected_rows().iter().map(|r| r.widget_name().to_string()).collect();
        u.showing_detail.set(!u.selected.borrow().is_empty());
        self::details(&u);
        layout(&u);
    });
    let u = ui.clone();
    ui.list.connect_row_activated(move |_, _| edit_selected(&u));
    // Lay out again when the window's size changes: a probe over the whole window that
    // reports its own resizes (as Music does), not a per-frame tick.
    let probe = gtk::DrawingArea::new();
    probe.set_can_target(false);
    probe.set_can_focus(false);
    ui.overlay.add_overlay(&probe);
    ui.overlay.set_measure_overlay(&probe, false);
    let u = ui.clone();
    probe.connect_resize(move |_, _, _| {
        let u = u.clone();
        glib::idle_add_local_once(move || layout(&u));
    });
    ui.search.connect_stop_search(|s| {
        // Esc while typing in the search: clear it.
        s.set_text("");
    });
    for (label, run) in [
        ("Merge exact duplicates", 9),
        ("Import contacts…", 0),
        ("Export selected / view as vCard…", 1),
        ("Export selected / view as CSV…", 2),
        ("Add selected to list…", 3),
        ("Remove selected from this list", 4),
        ("Delete selected", 5),
        ("Rename this list…", 6),
        ("Delete this list", 7),
        ("Email this list", 8),
    ] {
        let b = button(label, label);
        b.add_css_class("menu-item");
        b.add_css_class("flat");
        actions.append(&b);
        let u = ui.clone();
        let p = pop.clone();
        b.connect_clicked(move |_| {
            p.popdown();
            match run {
                0 => transfer::choose_import(&u),
                1 => transfer::export(&u, false),
                2 => transfer::export(&u, true),
                3 => membership(&u),
                4 => remove_members(&u),
                5 => delete(&u),
                6 => rename_list(&u),
                7 => delete_list(&u),
                9 => merge_all(&u),
                _ => email_list(&u),
            }
        });
    }
    nyx_ui::keys::install(
        &ui.window,
        nyx_ui::keys::Common { settings: Some(settings), shortcuts: Some(show_shortcuts), close: true, ..Default::default() },
    );
    let keys = gtk::EventControllerKey::new();
    let u = ui.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        match key {
            gdk::Key::f | gdk::Key::F if ctrl => find(&u),
            gdk::Key::n | gdk::Key::N if ctrl => editor::open(&u, None),
            gdk::Key::b | gdk::Key::B if ctrl => {
                prefs::update(|p| p.sidebar_collapsed = !p.sidebar_collapsed);
                layout(&u);
            }
            gdk::Key::F1 => show_shortcuts(),
            gdk::Key::Escape => {
                if nyx_ui::settings_card::is_open() {
                    nyx_ui::settings_card::escape();
                } else {
                    u.search.set_text("");
                    u.showing_detail.set(false);
                    layout(&u);
                }
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    ui.window.add_controller(keys);
    let u = ui.clone();
    ui.window.connect_close_request(move |_| {
        if let Some(e) = u.editor.borrow().as_ref() {
            e.present();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    refresh(&ui);
    let s = ui.section.borrow().clone();
    navigate(&ui, &s);
    load_eds(&ui);
    ui.window.present();
    if prefs::take_broken() {
        toast(&ui, "Settings could not be read. The original is kept as settings.toml.bak.");
    }
    if let Some(path) = std::env::var_os("CONTACTS_SNAPSHOT") {
        ui.window.set_default_size(800, 700);
        let u = ui.clone();
        let app = app.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
            if let Some(child) = u.window.child() {
                let paintable = gtk::WidgetPaintable::new(Some(&child));
                let (width, height) = (child.width(), child.height());
                let snapshot = gtk::Snapshot::new();
                if let Ok(bg) = gdk::RGBA::parse(theme::current_palette().bg.as_str()) {
                    snapshot.append_color(&bg, &gtk::graphene::Rect::new(0.0, 0.0, width as f32, height as f32));
                }
                paintable.snapshot(&snapshot, width as f64, height as f64);
                if let (Some(node), Some(renderer)) = (snapshot.to_node(), u.window.renderer()) {
                    let texture = renderer.render_texture(node, None);
                    match texture.save_to_png(std::path::Path::new(&path)) {
                        Ok(()) => println!(
                            "Contacts snapshot: {width} × {height}, sidebar {} px, list {} px, details {} px",
                            u.sidebar.width(),
                            u.list_scroll.width(),
                            u.detail_scroll.width()
                        ),
                        Err(e) => eprintln!("Snapshot: {e}"),
                    }
                }
            }
            app.quit();
        });
    }
    Ok(ui)
}
fn install_icons() {
    let dir = nyx_core::paths::cache_dir().join("icons");
    let _ = std::fs::create_dir_all(&dir);
    for (name, svg) in [
        ("contact-new-symbolic.svg", include_str!("../data/icons/contact-new-symbolic.svg")),
        ("contact-book-symbolic.svg", include_str!("../data/icons/contact-book-symbolic.svg")),
    ] {
        let _ = std::fs::write(dir.join(name), svg);
    }
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_search_path(&dir);
    }
}
pub fn button(label: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.set_tooltip_text(Some(tip));
    b
}
pub fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}
pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = w::label(text, class);
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_selectable(true);
    l
}
pub fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(child)
        .build()
}
pub fn commit(ui: &Ui, book: Book) -> bool {
    match ui.store.borrow_mut().save(&book) {
        Ok(()) => {
            *ui.book.borrow_mut() = book;
            ui.dup_ids.borrow_mut().take();
            refresh(ui);
            true
        }
        Err(e) => {
            toast(ui, &format!("Could not save: {e:#}"));
            false
        }
    }
}
/// Ids of local contacts that share an email or phone with another.
fn dup_ids(ui: &Ui) -> std::collections::HashSet<String> {
    let mut cached = ui.dup_ids.borrow_mut();
    cached
        .get_or_insert_with(|| model::DupIndex::new(&ui.book.borrow().contacts).flagged().into_iter().map(str::to_string).collect())
        .clone()
}

/// Fold every group of contacts that share an exact email or phone into one card (the
/// one with the most details first), with Undo.
fn merge_all(ui: &Ui) {
    let before = ui.book.borrow().clone();
    let mut book = before.clone();
    let index = model::DupIndex::new(&before.contacts);
    let groups = index.groups();
    if groups.is_empty() {
        toast(ui, "No contacts share an email or phone.");
        return;
    }
    let weight = |c: &Contact| c.emails.len() + c.phones.len() + c.addresses.len() + c.websites.len() + usize::from(!c.notes.is_empty());
    let mut merged = 0;
    for group in &groups {
        let mut cards: Vec<Contact> = group.iter().filter_map(|id| before.contacts.iter().find(|c| c.id == *id).cloned()).collect();
        cards.sort_by_key(|c| std::cmp::Reverse(weight(c)));
        let mut primary = cards.remove(0);
        for other in &cards {
            primary = model::combine(primary, other);
        }
        for other in &cards {
            model::merge(&mut book, primary.clone(), &other.id);
            merged += 1;
        }
    }
    if commit(ui, book) {
        let u = ui.clone();
        toast_action(
            ui,
            &format!("Merged {merged} duplicate cards into {}.", groups.len()),
            Some(("Undo", Box::new(move || {
                commit(&u, before.clone());
            }))),
        );
    }
}

pub fn is_remote(ui: &Ui) -> bool {
    ui.section.borrow().starts_with("eds:")
}

fn eds_uid(section: &str) -> Option<&str> {
    section.strip_prefix("eds:")
}




fn load_eds(ui: &Ui) {
    let u = ui.clone();
    let handle = gtk::gio::spawn_blocking(|| {
        if let Ok(registry) = nyx_pim::Registry::connect() { let _ = registry.refresh_accounts(nyx_accounts_native::Service::Contacts); }
        nyx_pim::contacts::list_address_books()
    });
    glib::spawn_future_local(async move {
        match handle.await {
            Ok(Ok(books)) => {
                u.eds_ready.set(true);
                *u.eds_books.borrow_mut() = books;
            }
            Ok(Err(nyx_pim::Error::Unavailable(_))) => {
                u.eds_ready.set(false);
                u.eds_books.borrow_mut().clear();
            }
            _ => {
                u.eds_ready.set(false);
            }
        }
        let section = u.section.borrow().clone();
        if let Some(uid) = eds_uid(&section)
            && !u.eds_books.borrow().iter().any(|b| b.uid == uid)
        {
            drop(section);
            navigate(&u, "all");
            return;
        }
        nav(&u);
        layout(&u);
    });
}

fn load_eds_contacts(ui: &Ui, uid: String) {
    ui.eds_loading.set(true);
    ui.eds_contacts.borrow_mut().clear();
    let u = ui.clone();
    let handle = gtk::gio::spawn_blocking(move || nyx_pim::contacts::list_vcards(&uid));
    glib::spawn_future_local(async move {
        let mut contacts = Vec::new();
        if let Ok(Ok(vcards)) = handle.await {
            for card in vcards {
                let mut imported = crate::formats::vcard_read(&card);
                if let Some(mut c) = imported.contacts.pop() {
                    if let Some(id) = vcard_uid(&card) {
                        c.id = id;
                    }
                    contacts.push(c);
                }
            }
        }
        u.eds_loading.set(false);
        *u.eds_contacts.borrow_mut() = contacts;
        if is_remote(&u) {
            refresh(&u);
        }
    });
}

fn vcard_uid(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        line.get(..4).filter(|p| p.eq_ignore_ascii_case("uid:")).map(|_| line[4..].trim().to_string())
    })
}

fn lookup(ui: &Ui, id: &str) -> Option<Contact> {
    if is_remote(ui) {
        ui.eds_contacts.borrow().iter().find(|c| c.id == id).cloned()
    } else {
        ui.book.borrow().contacts.iter().find(|c| c.id == id).cloned()
    }
}

pub fn navigate(ui: &Ui, section: &str) {
    if section == "settings" {
        settings();
        return;
    }
    let eds_ok = eds_uid(section).is_some_and(|uid| ui.eds_books.borrow().iter().any(|b| b.uid == uid));
    let valid = matches!(section, "all" | "favorites" | "birthdays" | "duplicates" | "online")
        || ui.book.borrow().lists.iter().any(|l| l.id == section)
        || eds_ok;
    *ui.section.borrow_mut() = if valid { section.into() } else { "all".into() };
    ui.selected.borrow_mut().clear();
    ui.showing_detail.set(false);
    if !is_remote(ui) {
        ui.eds_contacts.borrow_mut().clear();
        prefs::update(|p| p.last_section = ui.section.borrow().clone());
    }
    if let Some(uid) = eds_uid(&ui.section.borrow()) {
        load_eds_contacts(ui, uid.to_string());
    }
    refresh(ui);
}
pub fn visible(ui: &Ui) -> Vec<Contact> {
    let book = ui.book.borrow();
    let section = ui.section.borrow();
    let query = ui.search.text();
    if section.as_str() == "online" {
        return vec![];
    }
    if let Some(_uid) = eds_uid(section.as_str()) {
        let mut result: Vec<_> = ui.eds_contacts.borrow().iter().filter(|c| c.matches(&query)).cloned().collect();
        let p = prefs::get();
        result.sort_by_key(|c| {
            let key = if p.sort_last { format!("{} {}", c.last, c.first) } else { format!("{} {}", c.first, c.last) };
            if key.trim().is_empty() { c.name(p.last_first).to_lowercase() } else { key.to_lowercase() }
        });
        return result;
    }
    if section.as_str() == "birthdays" {
        // Soonest first, through the coming year.
        return crate::control::upcoming(&book, 366).into_iter().map(|(_, c)| c).filter(|c| c.matches(&query)).collect();
    }
    let dups = if section.as_str() == "duplicates" { dup_ids(ui) } else { Default::default() };
    let mut result: Vec<_> = book
        .contacts
        .iter()
        .filter(|c| {
            (match section.as_str() {
                "all" => true,
                "favorites" => c.favorite,
                "duplicates" => dups.contains(&c.id),
                id => book.lists.iter().find(|l| l.id == id).is_some_and(|l| l.members.contains(&c.id)),
            }) && c.matches(&query)
        })
        .cloned()
        .collect();
    let p = prefs::get();
    result.sort_by_key(|c| {
        let key = if p.sort_last { format!("{} {}", c.last, c.first) } else { format!("{} {}", c.first, c.last) };
        if key.trim().is_empty() { c.name(p.last_first).to_lowercase() } else { key.to_lowercase() }
    });
    result
}
pub fn chosen(ui: &Ui) -> Vec<Contact> {
    let selected = ui.selected.borrow();
    if selected.is_empty() {
        visible(ui)
    } else {
        ui.book.borrow().contacts.iter().filter(|c| selected.contains(&c.id)).cloned().collect()
    }
}
pub fn refresh(ui: &Ui) {
    ui.rebuilding.set(true);
    while let Some(c) = ui.list.first_child() {
        ui.list.remove(&c);
    }
    let contacts = visible(ui);
    let p = prefs::get();
    let birthdays = ui.section.borrow().as_str() == "birthdays";
    let soon: std::collections::HashMap<String, i64> = if birthdays {
        crate::control::upcoming(&ui.book.borrow(), 366).into_iter().map(|(n, c)| (c.id, n)).collect()
    } else {
        Default::default()
    };
    let selected = ui.selected.borrow().clone();
    let alive: Vec<_> = selected.into_iter().filter(|id| contacts.iter().any(|c| c.id == *id)).collect();
    *ui.selected.borrow_mut() = alive.clone();
    for c in &contacts {
        let row = gtk::ListBoxRow::new();
        row.set_widget_name(&c.id);
        let b = w::vbox(2);
        b.set_margin_top(8);
        b.set_margin_bottom(8);
        b.set_margin_start(14);
        b.set_margin_end(14);
        let heading = w::hbox(8);
        let name = gtk::Label::new(Some(&c.name(p.last_first)));
        name.set_hexpand(true);
        name.add_css_class("contact-row-name");
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        heading.append(&name);
        if c.favorite {
            let star = gtk::Image::from_icon_name("starred-symbolic");
            star.add_css_class("accent");
            heading.append(&star);
        }
        b.append(&heading);
        let when = soon.get(&c.id).map(|&n| {
            let day = birthday_text(&c.birthday);
            match n {
                0 => format!("Today · {day}"),
                1 => format!("Tomorrow · {day}"),
                n => format!("In {n} days · {day}"),
            }
        });
        let sub = gtk::Label::new(Some(when.as_deref().unwrap_or(if c.company.is_empty() {
            c.emails.first().map(|f| f.value.as_str()).unwrap_or("")
        } else {
            &c.company
        })));
        sub.add_css_class("dim");
        sub.add_css_class("contact-row-description");
        sub.set_xalign(0.0);
        sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
        b.append(&sub);
        row.set_child(Some(&b));
        ui.list.append(&row);
        if alive.contains(&c.id) {
            ui.list.select_row(Some(&row));
        }
    }
    if contacts.is_empty() {
        let empty = w::vbox(12);
        empty.add_css_class("contact-empty");
        empty.set_margin_top(80);
        empty.set_halign(gtk::Align::Center);
        let icon = gtk::Image::from_icon_name("contact-book-symbolic");
        icon.set_pixel_size(32);
        icon.add_css_class("dim");
        icon.set_halign(gtk::Align::Center);
        empty.append(&icon);
        empty.set_margin_start(20);
        empty.set_margin_end(20);
        empty.append(&label(&empty_copy(ui), "dim"));
        if !is_remote(ui) && ui.section.borrow().as_str() != "online" {
            let b = button("New contact", "New contact (Ctrl+N)");
            let u = ui.clone();
            b.connect_clicked(move |_| editor::open(&u, None));
            empty.append(&b);
            let b = button("Import contacts…", "Import vCard or CSV contacts");
            let u = ui.clone();
            b.connect_clicked(move |_| transfer::choose_import(&u));
            empty.append(&b);
        }
        let row = gtk::ListBoxRow::new();
        row.add_css_class("contact-empty-row");
        row.set_selectable(false);
        row.set_activatable(false);
        row.set_child(Some(&empty));
        ui.list.append(&row);
    }
    ui.rebuilding.set(false);
    nav(ui);
    details(ui);
    ui.status.set_text(&format!("{} contacts · {} selected", contacts.len(), alive.len()));
    layout(ui);
}
fn empty_copy(ui: &Ui) -> String {
    if !ui.search.text().is_empty() {
        return "No matching contacts.".into();
    }
    if ui.section.borrow().as_str() == "online" {
        return if ui.eds_ready.get() {
            "Connect an account in Settings → Online Accounts to see those contacts here.".into()
        } else {
            "Install evolution-data-server to sync contacts.".into()
        };
    }
    if is_remote(ui) {
        return if ui.eds_loading.get() {
            "Looking for contacts…".into()
        } else {
            "No contacts in this account book. They stay on the account; this computer's book is All contacts.".into()
        };
    }
    "No contacts here yet.".into()
}
// Match PDF's navigation structure; shared CSS owns the sidebar dimensions.
fn nav_button(icon: &str, title: &str, tooltip: &str) -> (gtk::Button, gtk::Label) {
    let button = gtk::Button::new();
    button.add_css_class("nav-item");
    let content = w::hbox(12);
    content.append(&gtk::Image::from_icon_name(icon));
    let text = w::label(title, "nav-label");
    text.set_hexpand(true);
    text.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.add_css_class("compact-hide");
    content.append(&text);
    button.set_child(Some(&content));
    button.set_tooltip_text(Some(tooltip));
    (button, text)
}
fn nav_group(ui: &Ui, title: &str, first: bool) {
    let heading = w::label(title, "nav-group");
    heading.add_css_class("compact-hide");
    if first {
        heading.add_css_class("first");
    }
    ui.nav.append(&heading);
}
fn nav(ui: &Ui) {
    clear(&ui.nav);
    let book = ui.book.borrow();
    let mut items = vec![
        ("all".into(), "All contacts".into(), "contact-book-symbolic", book.contacts.len()),
        ("favorites".into(), "Favorites".into(), "starred-symbolic", book.contacts.iter().filter(|c| c.favorite).count()),
        (
            "birthdays".into(),
            "Birthdays".into(),
            "x-office-calendar-symbolic",
            crate::control::upcoming(&book, 30).len(),
        ),
        ("duplicates".into(), "Duplicates".into(), "edit-copy-symbolic", dup_ids(ui).len()),
    ];
    for l in &book.lists {
        items.push((l.id.clone(), l.name.clone(), "view-list-symbolic", l.members.len()));
    }
    nav_group(ui, "CONTACTS", true);
    for (i, (id, name, icon, count)) in items.into_iter().enumerate() {
        if i == 4 {
            nav_group(ui, "LISTS", false);
        }
        let (b, _) = nav_button(icon, &name, &name);
        let count = w::label(&count.to_string(), "nav-readout");
        count.add_css_class("compact-hide");
        if let Some(line) = b.child().and_downcast::<gtk::Box>() {
            line.append(&count);
        }
        if id == *ui.section.borrow() {
            b.add_css_class("active");
            set_page_title(ui, &name);
        }
        let u = ui.clone();
        b.connect_clicked(move |_| navigate(&u, &id));
        ui.nav.append(&b);
    }
    if book.lists.is_empty() {
        nav_group(ui, "LISTS", false);
    }
    let books = ui.eds_books.borrow().clone();
    nav_group(ui, "ONLINE", false);
    if books.is_empty() {
        let (b, _) = nav_button("network-workgroup-symbolic", "Online books", "Contacts from connected accounts");
        if ui.section.borrow().as_str() == "online" {
            b.add_css_class("active");
            set_page_title(ui, "Online books");
        }
        let u = ui.clone();
        b.connect_clicked(move |_| navigate(&u, "online"));
        ui.nav.append(&b);
    } else {
        for book in &books {
            let id = format!("eds:{}", book.uid);
            let (b, _) = nav_button("network-workgroup-symbolic", book.label(), book.label());
            if id == *ui.section.borrow() {
                b.add_css_class("active");
                set_page_title(ui, book.label());
                let count = w::label(&ui.eds_contacts.borrow().len().to_string(), "nav-readout");
                count.add_css_class("compact-hide");
                if let Some(line) = b.child().and_downcast::<gtk::Box>() {
                    line.append(&count);
                }
            }
            let u = ui.clone();
            b.connect_clicked(move |_| navigate(&u, &id));
            ui.nav.append(&b);
        }
    }
    let (add, _) = nav_button("list-add-symbolic", "New list", "Create a contact list");
    add.add_css_class("nav-add");
    let u = ui.clone();
    add.connect_clicked(move |_| {
        let u = u.clone();
        w::ask_text("New list", "Organize contacts without making copies.", "", "Create", move |name| {
            let mut book = u.book.borrow().clone();
            let id = model::id();
            book.lists.push(model::List { id: id.clone(), name, members: Default::default() });
            if commit(&u, book) {
                navigate(&u, &id);
            }
        });
    });
    ui.nav.append(&add);
}
/// The section's name in the window title and the header's subtitle.
fn set_page_title(ui: &Ui, name: &str) {
    chrome::set_title(&ui.window, "Contacts", name);
    ui.subtitle.set_text(name);
    ui.subtitle.set_visible(!name.is_empty());
}
fn layout(ui: &Ui) {
    let width = if ui.window.width() > 0 { ui.window.width() } else { ui.window.default_width() };
    let narrow = width > 0 && width < 980;
    nyx_ui::settings_card::fit(&ui.window);
    ui.narrow.set(narrow);
    let compact = narrow || prefs::get().sidebar_collapsed;
    if compact {
        ui.sidebar.add_css_class("compact");
    } else {
        ui.sidebar.remove_css_class("compact");
    }
    if let Some(toggle) = ui.head.first_child() {
        toggle.set_halign(if compact { gtk::Align::Center } else { gtk::Align::End });
    }
    fn hide(root: &gtk::Widget, compact: bool) {
        if root.has_css_class("nav-item")
            && let Some(content) = root.downcast_ref::<gtk::Button>().and_then(|b| b.child()) {
                content.set_halign(if compact { gtk::Align::Center } else { gtk::Align::Fill });
            }
        if root.has_css_class("compact-hide") {
            root.set_visible(!compact);
        }
        let mut child = root.first_child();
        while let Some(c) = child {
            hide(&c, compact);
            child = c.next_sibling();
        }
    }
    hide(ui.sidebar.upcast_ref(), compact);
    let empty = visible(ui).is_empty();
    ui.list_scroll.set_hexpand(narrow || empty);
    ui.list_scroll.set_visible(empty || !narrow || !ui.showing_detail.get());
    ui.detail_scroll.set_visible(!empty && (!narrow || ui.showing_detail.get()));
    if let Some(back) = ui.details.first_child() {
        back.set_visible(narrow);
    }
    ui.list_scroll.set_width_request(if narrow || empty { -1 } else { 280 });
}
fn details(ui: &Ui) {
    clear(&ui.details);
    let ids = ui.selected.borrow().clone();
    {
        let b = w::labeled_button("go-previous-symbolic", "Contacts");
        b.set_tooltip_text(Some("Back to contacts (Esc)"));
        b.set_halign(gtk::Align::Start);
        b.set_visible(ui.narrow.get());
        let u = ui.clone();
        b.connect_clicked(move |_| {
            u.showing_detail.set(false);
            layout(&u);
        });
        ui.details.append(&b);
    }
    if ids.len() != 1 {
        let empty = w::vbox(12);
        empty.add_css_class("contact-empty");
        empty.set_valign(gtk::Align::Center);
        empty.set_halign(gtk::Align::Center);
        empty.set_vexpand(true);
        let icon = gtk::Image::from_icon_name("contact-book-symbolic");
        icon.set_pixel_size(40);
        icon.add_css_class("dim");
        empty.append(&icon);
        let message = label(
            if ids.is_empty() {
                "Select a contact to see their details."
            } else {
                "Multiple contacts selected. Use the actions menu to export, organize, or delete them."
            },
            "dim",
        );
        message.set_max_width_chars(36);
        message.set_justify(gtk::Justification::Center);
        empty.append(&message);
        ui.details.append(&empty);
        return;
    }
    let Some(c) = lookup(ui, &ids[0]) else { return };
    let header = w::hbox(14);
    header.append(&avatar(&c, 48));
    let identity = w::vbox(4);
    identity.set_hexpand(true);
    identity.append(&label(&c.name(prefs::get().last_first), "contact-name"));
    header.append(&identity);
    ui.details.append(&header);
    if !c.company.is_empty() || !c.title.is_empty() {
        identity.append(&label(
            &[c.title.as_str(), c.company.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
            "dim",
        ));
    }
    let bar = w::hbox(6);
    if is_remote(ui) {
        identity.append(&label("On this account · stays on the provider", "dim"));
    }
    let favorite = w::icon_button(
        if c.favorite { "starred-symbolic" } else { "non-starred-symbolic" },
        if c.favorite { "Remove from favorites" } else { "Add to favorites" },
    );
    if c.favorite {
        favorite.add_css_class("suggested-action");
    }
    let u = ui.clone();
    let id = c.id.clone();
    favorite.connect_clicked(move |_| {
        let mut book = u.book.borrow().clone();
        if let Some(c) = book.contacts.iter_mut().find(|c| c.id == id) {
            c.favorite = !c.favorite;
        }
        commit(&u, book);
    });
    if !is_remote(ui) {
        bar.append(&favorite);
        let edit = w::icon_button("document-edit-symbolic", "Edit contact (Enter)");
        let u = ui.clone();
        edit.connect_clicked(move |_| edit_selected(&u));
        bar.append(&edit);
    }
    ui.details.append(&bar);
    for (title, fields, scheme) in [
        ("EMAIL", &c.emails, "mailto:"),
        ("PHONE", &c.phones, "tel:"),
        ("ADDRESS", &c.addresses, ""),
        ("WEBSITE", &c.websites, ""),
    ] {
        if fields.is_empty() {
            continue;
        }
        ui.details.append(&label(title, "group-title"));
        let rows = w::vbox(0);
        rows.add_css_class("group-list");
        ui.details.append(&rows);
        for f in fields {
            let actions = w::hbox(4);
            // Addresses show (and copy) as lines, not their stored parts.
            let shown = if title == "ADDRESS" { model::Address::parse(&f.value).display() } else { f.value.clone() };
            let copy = w::bar_button("edit-copy-symbolic", "Copy value");
            let value = shown.clone();
            let win = ui.window.clone();
            copy.connect_clicked(move |_| win.clipboard().set_text(&value));
            actions.append(&copy);
            if title == "ADDRESS" {
                let map = w::bar_button("mark-location-symbolic", "Show on a map");
                let query = shown.replace('\n', ", ");
                let u = ui.clone();
                map.connect_clicked(move |_| {
                    let q = glib::uri_escape_string(&query, None::<&str>, false);
                    launch(&u, &format!("https://www.openstreetmap.org/search?query={q}"));
                });
                actions.append(&map);
            }
            if title != "ADDRESS" {
                let open = w::bar_button("external-link-symbolic", "Open using the default app");
                let value = f.value.clone();
                let u = ui.clone();
                open.connect_clicked(move |_| {
                    let uri = if title == "WEBSITE" {
                        if value.starts_with("https://") || value.starts_with("http://") {
                            value.clone()
                        } else {
                            format!("https://{value}")
                        }
                    } else {
                        format!(
                            "{scheme}{}",
                            if scheme == "mailto:" {
                                glib::uri_escape_string(&value, Some("@+"), false).to_string()
                            } else {
                                value.clone()
                            }
                        )
                    };
                    launch(&u, &uri);
                });
                actions.append(&open);
            }
            rows.append(&w::row(&glib::markup_escape_text(&shown), &glib::markup_escape_text(&f.label), Some(actions.upcast_ref())));
        }
    }
    let birthday = birthday_text(&c.birthday);
    for (title, text) in [("BIRTHDAY", &birthday), ("NOTES", &c.notes)] {
        if !text.is_empty() {
            ui.details.append(&label(title, "contact-heading"));
            ui.details.append(&label(text, ""));
        }
    }
    let lists = ui
        .book
        .borrow()
        .lists
        .iter()
        .filter(|l| l.members.contains(&c.id))
        .map(|l| l.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    if !lists.is_empty() {
        ui.details.append(&label("LISTS", "contact-heading"));
        ui.details.append(&label(&lists, "dim"));
    }
    if *ui.section.borrow() == "duplicates" {
        let others: Vec<_> =
            ui.book.borrow().contacts.iter().filter(|o| o.id != c.id && model::duplicates(&c, o)).cloned().collect();
        ui.details.append(&label("POSSIBLE DUPLICATES", "contact-heading"));
        let rows = w::vbox(0);
        rows.add_css_class("group-list");
        for other in others {
            let b = w::icon_button("view-dual-symbolic", &format!("Review a merge with {}", other.name(false)));
            let u = ui.clone();
            let a = c.clone();
            let o = other.clone();
            b.connect_clicked(move |_| editor::review_merge(&u, &a, &o));
            let what = other.emails.first().or(other.phones.first()).map(|f| f.value.clone()).unwrap_or_default();
            rows.append(&w::row(&glib::markup_escape_text(&other.name(false)), &glib::markup_escape_text(&what), Some(b.upcast_ref())));
        }
        ui.details.append(&rows);
    }
}

/// A birthday for reading: "12 April 1985", or "12 April" without a year. Anything
/// Contacts doesn't read is shown as it is.
fn birthday_text(b: &str) -> String {
    const MONTHS: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let Some((m, d)) = model::birthday_md(b).filter(|(m, _)| (1..=12).contains(m)) else { return b.to_string() };
    let month = MONTHS[m as usize - 1];
    match b.get(..4).filter(|_| !b.starts_with("--")) {
        Some(y) => format!("{d} {month} {y}"),
        None => format!("{d} {month}"),
    }
}
pub fn avatar(c: &Contact, size: i32) -> gtk::Widget {
    if !c.photo.is_empty()
        && let Ok(t) = gdk::Texture::from_bytes(&glib::Bytes::from(&c.photo[..])) {
            let p = gtk::Picture::for_paintable(&t);
            p.set_size_request(size, size);
            p.set_content_fit(gtk::ContentFit::Contain);
            p.set_halign(gtk::Align::Start);
            return p.upcast();
        }
    let initials = c.name(false).split_whitespace().take(2).filter_map(|s| s.chars().next()).collect::<String>().to_uppercase();
    let l = gtk::Label::new(Some(&initials));
    l.add_css_class("contact-avatar");
    l.set_size_request(size, size);
    l.set_halign(gtk::Align::Start);
    l.upcast()
}
fn find(ui: &Ui) {
    ui.search.grab_focus();
}
fn edit_selected(ui: &Ui) {
    if is_remote(ui) {
        toast(ui, "This book is on the account. Add or edit contacts in All contacts.");
        return;
    }
    let id = ui.selected.borrow().first().cloned();
    if let Some(id) = id {
        let c = lookup(ui, &id);
        editor::open(ui, c);
    }
}
pub fn toast(ui: &Ui, text: &str) {
    toast_action(ui, text, None);
}
/// A toast's button: its label and what it does.
pub type ToastAction<'a> = Option<(&'a str, Box<dyn Fn()>)>;
pub fn toast_action(ui: &Ui, text: &str, action: ToastAction<'_>) {
    clear(&ui.toast_box);
    let l = label(text, "");
    l.set_max_width_chars(65);
    ui.toast_box.append(&l);
    if let Some((name, run)) = action {
        let b = button(name, name);
        let u = ui.clone();
        b.connect_clicked(move |_| {
            run();
            u.toast_box.set_visible(false);
        });
        ui.toast_box.append(&b);
    }
    ui.toast_box.set_visible(true);
    let serial = ui.toast_serial.get() + 1;
    ui.toast_serial.set(serial);
    let weak = Rc::downgrade(ui);
    glib::timeout_add_local_once(std::time::Duration::from_secs(8), move || {
        if let Some(u) = weak.upgrade()
            && u.toast_serial.get() == serial {
                u.toast_box.set_visible(false);
            }
    });
}
fn delete(ui: &Ui) {
    if is_remote(ui) {
        toast(ui, "This book is on the account. Disconnect it in Settings → Online Accounts.");
        return;
    }
    let ids = ui.selected.borrow().clone();
    if ids.is_empty() {
        toast(ui, "Select contacts to delete.");
        return;
    }
    let old = ui.book.borrow().clone();
    let mut next = old.clone();
    next.contacts.retain(|c| !ids.contains(&c.id));
    for l in &mut next.lists {
        l.members.retain(|id| !ids.contains(id));
    }
    if commit(ui, next) {
        let u = ui.clone();
        toast_action(
            ui,
            &format!("Deleted {} contacts.", ids.len()),
            Some((
                "Undo",
                Box::new(move || {
                    let mut next = u.book.borrow().clone();
                    for c in old.contacts.iter().filter(|c| ids.contains(&c.id)) {
                        if !next.contacts.iter().any(|x| x.id == c.id) {
                            next.contacts.push(c.clone());
                        }
                    }
                    for l in &mut next.lists {
                        if let Some(prior) = old.lists.iter().find(|p| p.id == l.id) {
                            for id in prior.members.iter().filter(|id| ids.contains(id)) {
                                l.members.insert(id.clone());
                            }
                        }
                    }
                    commit(&u, next);
                }),
            )),
        )
    }
}
fn membership(ui: &Ui) {
    let ids = ui.selected.borrow().clone();
    if ids.is_empty() {
        toast(ui, "Select contacts to add to a list.");
        return;
    }
    let (dialog, card) = w::dialog("Add to list", 400);
    crate::dialog_dismissal::add(&dialog, &card, "Cancel");
    let lists = ui.book.borrow().lists.clone();
    if lists.is_empty() {
        card.append(&label("Create a list using the + button in the sidebar first.", "dim"));
    }
    for l in lists {
        let b = button(&l.name, "Add selected contacts to this list");
        card.append(&b);
        let u = ui.clone();
        let d = dialog.clone();
        let ids = ids.clone();
        b.connect_clicked(move |_| {
            let mut book = u.book.borrow().clone();
            if let Some(list) = book.lists.iter_mut().find(|x| x.id == l.id) {
                list.members.extend(ids.clone());
            }
            if commit(&u, book) {
                d.close();
            }
        });
    }
    dialog.present();
}
fn remove_members(ui: &Ui) {
    let ids = ui.selected.borrow().clone();
    let section = ui.section.borrow().clone();
    let mut book = ui.book.borrow().clone();
    if let Some(l) = book.lists.iter_mut().find(|l| l.id == section) {
        l.members.retain(|id| !ids.contains(id));
        commit(ui, book);
    } else {
        toast(ui, "Open a contact list first.");
    }
}
fn rename_list(ui: &Ui) {
    let section = ui.section.borrow().clone();
    let list = ui.book.borrow().lists.iter().find(|l| l.id == section).cloned();
    if let Some(l) = list {
        let u = ui.clone();
        w::ask_text("Rename list", "", &l.name, "Rename", move |name| {
            let mut book = u.book.borrow().clone();
            if let Some(l) = book.lists.iter_mut().find(|l| l.id == section) {
                l.name = name;
            }
            commit(&u, book);
        });
    } else {
        toast(ui, "Open a contact list first.");
    }
}
fn delete_list(ui: &Ui) {
    let section = ui.section.borrow().clone();
    let mut book = ui.book.borrow().clone();
    let Some(l) = book.lists.iter().find(|l| l.id == section).cloned() else {
        toast(ui, "Open a contact list first.");
        return;
    };
    book.lists.retain(|l| l.id != section);
    if commit(ui, book) {
        navigate(ui, "all");
        let u = ui.clone();
        toast_action(
            ui,
            "List deleted. Contacts kept.",
            Some((
                "Undo",
                Box::new(move || {
                    let mut book = u.book.borrow().clone();
                    let mut l = l.clone();
                    l.members.retain(|id| book.contacts.iter().any(|c| c.id == *id));
                    book.lists.push(l);
                    commit(&u, book);
                }),
            )),
        )
    }
}
fn email_list(ui: &Ui) {
    let section = ui.section.borrow().clone();
    let book = ui.book.borrow();
    let Some(list) = book.lists.iter().find(|l| l.id == section) else {
        toast(ui, "Open a contact list first.");
        return;
    };
    let mut emails = std::collections::BTreeSet::new();
    for c in book.contacts.iter().filter(|c| list.members.contains(&c.id)) {
        if let Some(e) = c.emails.first() {
            emails.insert(glib::uri_escape_string(&e.value, Some("@+"), false).to_string());
        }
    }
    if emails.is_empty() {
        toast(ui, "This list has no email addresses.");
    } else {
        launch(ui, &format!("mailto:{}", emails.into_iter().collect::<Vec<_>>().join(",")));
    }
}
pub fn launch(ui: &Ui, uri: &str) {
    if let Err(e) = gtk::gio::AppInfo::launch_default_for_uri(uri, None::<&gtk::gio::AppLaunchContext>) {
        toast(ui, &format!("Could not open: {e}"));
    }
}
pub const SHORTCUTS: &[(&[&str], &str)] = &[
    (&["Ctrl", "N"], "New contact"),
    (&["Ctrl", "S"], "Save contact in the editor"),
    (&["Ctrl", "F"], "Search contacts"),
    (&["Ctrl", "B"], "Collapse sidebar"),
    (&["Ctrl", ","], "Settings"),
    (&["Ctrl", "Shift", "P"], "Command palette"),
    (&["Esc"], "Close search, card, or return to contacts"),
    (&["F1"], "Show these shortcuts (or ?)"),
    (&["Ctrl", "Q"], "Quit Contacts"),
    (&["Ctrl", "W"], "Close window"),
];

pub fn show_shortcuts() {
    let (dialog, card) = w::dialog("Keyboard shortcuts", 520);
    let list = w::vbox(0);
    list.add_css_class("group-list");
    for (keys, action) in SHORTCUTS {
        list.append(&w::row(action, "", Some(w::key_caps(keys).upcast_ref())));
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .propagate_natural_height(true)
        .max_content_height(560)
        .child(&list)
        .build();
    card.append(&scroll);
    let close = button("Close", "Close shortcuts (Esc)");
    close.set_halign(gtk::Align::End);
    let d = dialog.clone();
    close.connect_clicked(move |_| d.close());
    card.append(&close);
    dialog.present();
}
pub fn settings() {
    let Some(ui) = UI.with(|u| u.borrow().clone()) else { return };
    super::editor::settings(&ui);
}
