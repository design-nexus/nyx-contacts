use crate::{
    editor, menu,
    model::{self, Book, Contact, Store},
    people::{self, Origin, Person},
    prefs, theme, transfer,
};
use gtk::{gdk, glib, prelude::*};
use nyx_ui::{chrome, widgets as w};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};
pub type Ui = Rc<State>;
/// Two contacts Duplicates shows side by side, and what they share.
type Pair = (String, String, &'static str);
pub struct State {
    pub window: gtk::ApplicationWindow,
    pub overlay: gtk::Overlay,
    pub book: RefCell<Book>,
    pub store: RefCell<Store>,
    pub section: RefCell<String>,
    pub list: gtk::ListBox,
    /// The details pane's content, or the editor while editing.
    pub details: gtk::Box,
    /// The editor's bar under the details: where it saves, Delete, Cancel and Save.
    pub edit_bar: gtk::Box,
    pub search: gtk::SearchEntry,
    nav: gtk::Box,
    sidebar: gtk::Box,
    foot_icon: gtk::Image,
    status: gtk::Label,
    /// The list and details ("panes"), or a page of its own ("full").
    views: gtk::Stack,
    list_col: gtk::Box,
    list_banner: gtk::Box,
    list_title: gtk::Label,
    list_count: gtk::Label,
    sort_label: gtk::Label,
    pane_line: gtk::Separator,
    detail_col: gtk::Box,
    back_bar: gtk::Box,
    page_bar: gtk::Box,
    page_title: gtk::Label,
    page_sub: gtk::Label,
    page_actions: gtk::Box,
    page_body: gtk::Box,
    selbar: gtk::Box,
    selbar_count: gtk::Label,
    selbar_remove: gtk::Button,
    selbar_add: gtk::Button,
    toast_box: gtk::Box,
    toast_serial: Cell<u64>,
    pub selected: RefCell<Vec<String>>,
    pub narrow: Cell<bool>,
    pub showing_detail: Cell<bool>,
    pub rebuilding: Cell<bool>,
    /// How many contacts the list shows.
    shown: Cell<usize>,
    pub editing: RefCell<Option<editor::Editing>>,
    eds_books: RefCell<Vec<nyx_pim::contacts::AddressBook>>,
    /// The book's own name, for books you renamed (`prefs.source_names`), by book id.
    book_names: RefCell<HashMap<String, String>>,
    /// Each account book's cards, and their vCards as stored, by book.
    cards: RefCell<HashMap<String, BookCards>>,
    /// Books whose cards are on their way.
    loading: RefCell<HashSet<String>>,
    /// Everyone in the shown sources, combined (All and Favorites), until something changes.
    people: RefCell<Option<Vec<Person>>>,
    /// The copy being edited or deleted, when it isn't the page's own (a person in All).
    pub target: RefCell<Option<Origin>>,
    eds_ready: Cell<bool>,
    eds_syncing: Cell<bool>,
    sync_error: RefCell<Option<String>>,
    native_watch: RefCell<Option<nyx_pim::Watch>>,
    /// Pairs of local contacts sharing an email or phone, worked out once per change to
    /// the book.
    dup_pairs: RefCell<Option<Vec<Pair>>>,
}
thread_local! {static UI:RefCell<Option<Ui>>=const{RefCell::new(None)};}
/// The open window's state, once `present` has run (the control interface uses it).
pub fn current() -> Option<Ui> {
    UI.with(|u| u.borrow().clone())
}

/// The swatches lists and initials are drawn in.
pub const SWATCHES: [&str; 8] = ["Blue", "Purple", "Pink", "Red", "Orange", "Yellow", "Green", "Teal"];

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
    let window = gtk::ApplicationWindow::builder().application(app).default_width(1120).default_height(800).build();
    window.add_css_class("contacts-window");
    window.set_icon_name(Some("io.github.design_nexus.nyx.Contacts"));
    window.set_titlebar(Some(&w::hbox(0)));

    // The sidebar: people, lists, accounts and the tidy-up pages, with sync at its foot.
    let sidebar = w::vbox(0);
    sidebar.set_hexpand(false);
    sidebar.add_css_class("settings-navigation");
    let nav = w::vbox(0);
    nav.set_hexpand(false);
    let nav_column = w::vbox(0);
    nav_column.append(&nav);
    let nav_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(false)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .child(&nav_column)
        .build();
    sidebar.append(&nav_scroll);
    // Sync, under the last item.
    let foot = w::hbox(6);
    foot.add_css_class("nav-foot");
    let foot_icon = gtk::Image::from_icon_name("emblem-ok-symbolic");
    foot_icon.add_css_class("status-icon");
    foot.append(&foot_icon);
    let status = w::label("", "status-readout");
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    status.set_hexpand(true);
    status.set_xalign(0.0);
    status.add_css_class("compact-hide");
    foot.append(&status);
    nav_column.append(&foot);

    // The header: the app and the sidebar's toggle, search, New and Settings.
    let header = chrome::app_header(&window, "avatar-default-symbolic", "Contacts");
    let collapse = w::bar_button("sidebar-show-symbolic", "Collapse the sidebar (Ctrl+B)");
    header.start.append(&collapse);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search people, companies, emails, phones"));
    search.update_property(&[gtk::accessible::Property::Label("Search contacts")]);
    search.add_css_class("bar-search");
    search.set_width_chars(22);
    search.set_max_width_chars(40);
    let search_box = gtk::Overlay::new();
    search_box.set_child(Some(&search));
    let hint = w::label("Ctrl F", "key-hint");
    hint.set_halign(gtk::Align::End);
    hint.set_valign(gtk::Align::Center);
    hint.set_margin_end(10);
    hint.set_can_target(false);
    search_box.add_overlay(&hint);
    search.connect_changed(move |s| hint.set_visible(s.text().is_empty()));
    header.center.append(&search_box);
    let new = gtk::Button::new();
    new.add_css_class("new-button");
    new.set_tooltip_text(Some("New contact (Ctrl+N)"));
    let face = w::hbox(6);
    face.append(&gtk::Image::from_icon_name("list-add-symbolic"));
    face.append(&gtk::Label::new(Some("New")));
    face.append(&w::label("Ctrl N", "key-hint"));
    new.set_child(Some(&face));
    new.set_valign(gtk::Align::Center);
    header.end.append(&new);
    header.end.append(&chrome::settings_button(settings));
    let frame = nyx_ui::frame::AppFrame::new(&header.root);
    frame.set_sidebar(&sidebar);
    let page = w::vbox(0);
    page.add_css_class("settings-content");
    page.set_hexpand(true);
    frame.set_page(&page);
    frame.set_flush(true);
    let overlay = frame.install(&window);
    let page_overlay = gtk::Overlay::new();
    page_overlay.set_vexpand(true);
    page.append(&page_overlay);
    let views = gtk::Stack::new();
    views.set_vexpand(true);
    page_overlay.set_child(Some(&views));

    // The list, with its title, count, sort and ⋯ menu.
    let panes = w::hbox(0);
    let list_col = w::vbox(0);
    list_col.add_css_class("list-col");
    let list_banner = w::vbox(0);
    list_banner.set_visible(false);
    list_col.append(&list_banner);
    let head = w::hbox(8);
    head.add_css_class("list-head");
    let list_title = w::label("", "list-title");
    list_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    head.append(&list_title);
    let list_count = w::label("", "list-count");
    list_count.set_hexpand(true);
    list_count.set_xalign(0.0);
    head.append(&list_count);
    let sort = gtk::Button::new();
    sort.add_css_class("sort-pick");
    sort.set_valign(gtk::Align::Center);
    sort.set_tooltip_text(Some("Sort by"));
    let sort_face = w::hbox(6);
    let sort_label = gtk::Label::new(Some("First name"));
    sort_face.append(&sort_label);
    sort_face.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
    sort.set_child(Some(&sort_face));
    head.append(&sort);
    let list_menu = w::bar_button("view-more-symbolic", "List actions");
    list_menu.set_valign(gtk::Align::Center);
    head.append(&list_menu);
    list_col.append(&head);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Multiple);
    list.set_activate_on_single_click(false);
    list.add_css_class("contact-list");
    let list_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&list)
        .build();
    list_col.append(&list_scroll);
    panes.append(&list_col);
    let pane_line = nyx_ui::frame::pane_line(gtk::Orientation::Vertical);
    panes.append(&pane_line);

    // The details, or the editor in their place.
    let detail_col = w::vbox(0);
    detail_col.set_hexpand(true);
    let back_bar = w::hbox(0);
    back_bar.add_css_class("back-bar");
    let back = w::labeled_button("go-previous-symbolic", "Contacts");
    back.add_css_class("flat");
    back.set_tooltip_text(Some("Back to contacts (Esc)"));
    back_bar.append(&back);
    detail_col.append(&back_bar);
    let details = w::vbox(0);
    details.add_css_class("contact-details");
    let clamp = nyx_ui::clamp::Clamp::new(&details, 620);
    let detail_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .hexpand(true)
        .vexpand(true)
        .child(&clamp)
        .build();
    detail_col.append(&detail_scroll);
    let edit_bar = w::hbox(8);
    edit_bar.add_css_class("edit-bar");
    edit_bar.set_visible(false);
    detail_col.append(&edit_bar);
    panes.append(&detail_col);
    views.add_named(&panes, Some("panes"));

    // A page of its own: Birthdays, Duplicates, an empty book.
    let full = w::vbox(0);
    let page_bar = w::hbox(10);
    page_bar.add_css_class("page-bar");
    let page_title = w::label("", "page-title");
    page_bar.append(&page_title);
    let page_sub = w::label("", "page-sub");
    page_sub.set_hexpand(true);
    page_sub.set_xalign(0.0);
    page_sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
    page_bar.append(&page_sub);
    let page_actions = w::hbox(8);
    page_actions.set_valign(gtk::Align::Center);
    page_bar.append(&page_actions);
    full.append(&page_bar);
    let page_body = w::vbox(0);
    page_body.add_css_class("page-body");
    full.append(&scroll(&page_body));
    views.add_named(&full, Some("full"));

    // The bar over the list when several people are selected.
    let selbar = w::hbox(6);
    selbar.add_css_class("selbar");
    selbar.set_halign(gtk::Align::Center);
    selbar.set_valign(gtk::Align::End);
    selbar.set_margin_bottom(20);
    selbar.set_visible(false);
    let selbar_count = w::label("", "selbar-count");
    selbar.append(&selbar_count);
    let bar_button = |icon: &str, text: &str| {
        let b = w::labeled_button(icon, text);
        b.add_css_class("selbar-button");
        selbar.append(&b);
        b
    };
    let selbar_add = bar_button("list-add-symbolic", "Add to list");
    let sel_email = bar_button("nyx-contacts-mail-symbolic", "Email");
    sel_email.set_visible(can_open("mailto"));
    let sel_export = bar_button("upload-symbolic", "Export");
    let selbar_remove = bar_button("list-remove-symbolic", "Remove from list");
    let sel_delete = bar_button("user-trash-symbolic", "Delete");
    sel_delete.add_css_class("danger-text-button");
    let sel_close = w::bar_button("window-close-symbolic", "Clear the selection (Esc)");
    selbar.append(&sel_close);
    page_overlay.add_overlay(&selbar);

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
        edit_bar,
        search,
        nav,
        sidebar,
        foot_icon,
        status,
        views,
        list_col,
        list_banner,
        list_title,
        list_count,
        sort_label,
        pane_line,
        detail_col,
        back_bar,
        page_bar,
        page_title,
        page_sub,
        page_actions,
        page_body,
        selbar,
        selbar_count,
        selbar_remove,
        selbar_add,
        toast_box,
        toast_serial: Cell::new(0),
        selected: RefCell::new(vec![]),
        narrow: Cell::new(false),
        showing_detail: Cell::new(false),
        rebuilding: Cell::new(false),
        shown: Cell::new(0),
        editing: RefCell::new(None),
        eds_books: RefCell::new(vec![]),
        book_names: RefCell::new(HashMap::new()),
        cards: RefCell::new(HashMap::new()),
        loading: RefCell::new(HashSet::new()),
        people: RefCell::new(None),
        target: RefCell::new(None),
        eds_ready: Cell::new(false),
        eds_syncing: Cell::new(false),
        sync_error: RefCell::new(None),
        native_watch: RefCell::new(None),
        dup_pairs: RefCell::new(None),
    });
    UI.with(|u| *u.borrow_mut() = Some(ui.clone()));
    let u = ui.clone();
    new.connect_clicked(move |_| new_contact(&u));
    let u = ui.clone();
    collapse.connect_clicked(move |_| {
        prefs::update(|p| p.sidebar_collapsed = !p.sidebar_collapsed);
        layout(&u);
    });
    let u = ui.clone();
    ui.search.connect_search_changed(move |_| refresh(&u));
    ui.search.connect_stop_search(|s| s.set_text(""));
    let u = ui.clone();
    sort.connect_clicked(move |b| {
        let by = |last: bool| {
            let u = u.clone();
            move || {
                prefs::update(|p| p.sort_last = last);
                refresh(&u);
            }
        };
        let sort_last = prefs::get().sort_last;
        let mark = |on: bool| if on { "object-select-symbolic" } else { "" };
        menu::below(b, vec![menu::item(mark(!sort_last), "First name", by(false)), menu::item(mark(sort_last), "Last name", by(true))]);
    });
    let u = ui.clone();
    list_menu.connect_clicked(move |b| menu::below(b, list_entries(&u, &u.section.borrow().clone())));
    let u = ui.clone();
    back.connect_clicked(move |_| {
        let u2 = u.clone();
        leave_editor(&u, move || {
            u2.showing_detail.set(false);
            layout(&u2);
        });
    });
    let u = ui.clone();
    ui.list.connect_selected_rows_changed(move |_| on_select(&u));
    let u = ui.clone();
    ui.list.connect_row_activated(move |_, _| edit_selected(&u));
    let u = ui.clone();
    menu::on_right_click(&ui.list, move |_, x, y| {
        let Some(row) = u.list.row_at_y(y as i32) else { return };
        if row.widget_name().is_empty() {
            return;
        }
        if !row.is_selected() {
            u.list.unselect_all();
            u.list.select_row(Some(&row));
        }
        menu::at(&u.list, x, y, contact_entries(&u, true));
    });
    let u = ui.clone();
    ui.selbar_add.connect_clicked(move |b| {
        let ids = u.selected.borrow().clone();
        menu::below(b, add_to_list_entries(&u, ids));
    });
    let u = ui.clone();
    sel_email.connect_clicked(move |_| email_people(&u, &chosen(&u)));
    let u = ui.clone();
    sel_export.connect_clicked(move |b| menu::below(b, export_entries(&u, chosen(&u))));
    let u = ui.clone();
    ui.selbar_remove.connect_clicked(move |_| remove_members(&u));
    let u = ui.clone();
    sel_delete.connect_clicked(move |_| delete(&u));
    let u = ui.clone();
    sel_close.connect_clicked(move |_| u.list.unselect_all());
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
            gdk::Key::n | gdk::Key::N if ctrl => new_contact(&u),
            gdk::Key::b | gdk::Key::B if ctrl => {
                prefs::update(|p| p.sidebar_collapsed = !p.sidebar_collapsed);
                layout(&u);
            }
            gdk::Key::F1 => show_shortcuts(),
            gdk::Key::Delete
                if u.editing.borrow().is_none()
                    && gtk::prelude::GtkWindowExt::focus(&u.window).is_some_and(|f| f.is_ancestor(&u.list)) =>
            {
                delete(&u)
            }
            gdk::Key::Escape => {
                if nyx_ui::settings_card::is_open() {
                    nyx_ui::settings_card::escape();
                } else if u.editing.borrow().is_some() {
                    leave_editor(&u, || {});
                } else if !u.search.text().is_empty() {
                    u.search.set_text("");
                } else if u.selected.borrow().len() > 1 {
                    u.list.unselect_all();
                } else {
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
    ui.window.connect_close_request(move |win| {
        let pending = u.editing.borrow().as_ref().map(|e| (e.dirty.clone(), e.close.clone()));
        if let Some((dirty, close)) = pending
            && dirty()
        {
            let win = win.clone();
            editor::ask_discard(
                &u,
                dirty,
                Rc::new(move || {
                    close();
                    win.close();
                }),
            );
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    refresh(&ui);
    let s = ui.section.borrow().clone();
    go(&ui, &s);
    load_eds(&ui);
    if let Ok(registry) = nyx_pim::Registry::connect() {
        let weak = Rc::downgrade(&ui);
        *ui.native_watch.borrow_mut() = Some(registry.watch(move || {
            if let Some(ui) = weak.upgrade() {
                load_eds(&ui);
            }
        }));
    }
    let weak = Rc::downgrade(&ui);
    let fingerprint = Rc::new(RefCell::new(None::<String>));
    let polling = Rc::new(Cell::new(false));
    // A snapshot run stays off the network: no account sync.
    let poll = std::env::var_os("CONTACTS_SNAPSHOT").is_none();
    glib::timeout_add_local(std::time::Duration::from_secs(5), move || {
        if !poll {
            return glib::ControlFlow::Break;
        }
        let Some(ui) = weak.upgrade() else { return glib::ControlFlow::Break };
        if polling.replace(true) {
            return glib::ControlFlow::Continue;
        }
        let handle = gtk::gio::spawn_blocking(|| {
            nyx_accounts_native::Client::connect()
                .and_then(|client| client.list())
                .and_then(|accounts| serde_json::to_string(&accounts).map_err(Into::into))
        });
        let previous = fingerprint.clone();
        let busy = polling.clone();
        glib::spawn_future_local(async move {
            let result = handle.await;
            busy.set(false);
            if let Ok(Ok(current)) = result {
                if previous.borrow().as_ref() != Some(&current) {
                    *previous.borrow_mut() = Some(current);
                    refresh_online(&ui);
                }
            }
        });
        glib::ControlFlow::Continue
    });
    ui.window.present();
    if prefs::take_broken() {
        toast(&ui, "Settings could not be read. The original is kept as settings.toml.bak.");
    }
    if let Some(path) = std::env::var_os("CONTACTS_SNAPSHOT") {
        snapshot(&ui, app, path.into());
    }
    Ok(ui)
}

/// `CONTACTS_SNAPSHOT=shot.png`: opens a page as asked, saves the window (or the dialog
/// it opened) as a PNG, and quits. `_SECTION` (an id, or a list's or book's name),
/// `_SELECT` (names, comma-separated), `_EDIT` (`1` for the selected contact, `new`),
/// `_IMPORT` (a file to preview), `_MENU` (`list`: the list's ⋯), `_W`, `_H` and `_DELAY` (ms).
fn snapshot(ui: &Ui, app: &gtk::Application, path: std::path::PathBuf) {
    let var = |k: &str| std::env::var(format!("CONTACTS_SNAPSHOT_{k}")).ok().filter(|v| !v.is_empty());
    let size = |k: &str, d: i32| var(k).and_then(|v| v.parse().ok()).unwrap_or(d);
    ui.window.set_default_size(size("W", 800), size("H", 700));
    let delay = size("DELAY", 1500) as u64;
    let u = ui.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(delay / 4), move || {
        if let Some(s) = var("SECTION") {
            let id = {
                let book = u.book.borrow();
                let books = u.eds_books.borrow();
                book.lists
                    .iter()
                    .find(|l| l.name == s)
                    .map(|l| l.id.clone())
                    .or_else(|| books.iter().find(|b| b.label() == s).map(|b| format!("eds:{}", b.uid)))
                    .unwrap_or(s)
            };
            if let Some(page) = id.strip_prefix("settings") {
                settings();
                if let Some(name) = page.strip_prefix(':') {
                    nyx_ui::settings_card::select(name);
                }
            } else {
                go(&u, &id);
            }
        }
    });
    let u = ui.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(delay / 2), move || {
        if let Some(names) = var("SELECT") {
            let wanted: Vec<String> = names.split(',').map(|s| s.trim().to_lowercase()).collect();
            let ids: Vec<String> = visible(&u)
                .into_iter()
                .filter(|c| wanted.iter().any(|w| c.name(false).to_lowercase().contains(w)))
                .map(|c| c.id)
                .collect();
            u.showing_detail.set(ids.len() == 1);
            *u.selected.borrow_mut() = ids;
            refresh(&u);
        }
        match var("EDIT").as_deref() {
            Some("new") => new_contact(&u),
            Some(_) => edit_selected(&u),
            None => {}
        }
        if let Some(file) = var("IMPORT") {
            transfer::import_path(&u, std::path::Path::new(&file));
        }
        if var("MENU").as_deref() == Some("list")
            && let Some(button) = u.list_title.parent().and_then(|h| h.last_child())
        {
            let section = u.section.borrow().clone();
            menu::below(&button, list_entries(&u, &section));
        }
    });
    let u = ui.clone();
    let app = app.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|t| t.downcast::<gtk::Window>().ok())
            .find(|t| t.is_visible() && t != u.window.upcast_ref::<gtk::Window>());
        let target = dialog.and_then(|d| d.child()).or_else(|| u.window.child());
        if let Some(child) = target {
            let paintable = gtk::WidgetPaintable::new(Some(&child));
            let (width, height) = (child.width(), child.height());
            let snapshot = gtk::Snapshot::new();
            if let Ok(bg) = gdk::RGBA::parse(theme::current_palette().bg.as_str()) {
                snapshot.append_color(&bg, &gtk::graphene::Rect::new(0.0, 0.0, width as f32, height as f32));
            }
            paintable.snapshot(&snapshot, width as f64, height as f64);
            if let (Some(node), Some(renderer)) = (snapshot.to_node(), u.window.renderer()) {
                let texture = renderer.render_texture(node, None);
                match texture.save_to_png(&path) {
                    Ok(()) => println!("Contacts snapshot: {width} × {height}"),
                    Err(e) => eprintln!("Snapshot: {e}"),
                }
            }
        }
        app.quit();
    });
}

pub fn button(label: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.set_tooltip_text(Some(tip));
    b
}
fn icon_text_button(icon: &str, text: &str, class: &str) -> gtk::Button {
    let b = w::labeled_button(icon, text);
    if !class.is_empty() {
        b.add_css_class(class);
    }
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
fn plain(text: &str, class: &str) -> gtk::Label {
    let l = w::label(text, class);
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
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
            ui.dup_pairs.borrow_mut().take();
            ui.people.borrow_mut().take();
            refresh(ui);
            true
        }
        Err(e) => {
            toast(ui, &format!("Could not save: {e:#}"));
            false
        }
    }
}

fn pair_key(a: &str, b: &str) -> String {
    if a < b { format!("{a}|{b}") } else { format!("{b}|{a}") }
}

/// Pairs of local contacts that share an email or phone, without those marked
/// "Not the same person".
fn dup_pairs(ui: &Ui) -> Vec<Pair> {
    if let Some(p) = ui.dup_pairs.borrow().as_ref() {
        return p.clone();
    }
    let book = ui.book.borrow();
    let dismissed: HashSet<String> = prefs::get().not_duplicates.into_iter().collect();
    let by_id: HashMap<&str, &Contact> = book.contacts.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut out = vec![];
    for group in model::DupIndex::new(&book.contacts).groups() {
        let cards: Vec<&Contact> = group.iter().filter_map(|id| by_id.get(id).copied()).collect();
        for i in 0..cards.len() {
            for j in i + 1..cards.len() {
                if let Some(why) = model::shared(cards[i], cards[j])
                    && !dismissed.contains(&pair_key(&cards[i].id, &cards[j].id))
                {
                    out.push((cards[i].id.clone(), cards[j].id.clone(), why));
                }
            }
        }
    }
    let name = |id: &str| by_id.get(id).map(|c| c.name(false).to_lowercase()).unwrap_or_default();
    out.sort_by_key(|(a, b, _)| (name(a), name(b)));
    *ui.dup_pairs.borrow_mut() = Some(out.clone());
    out
}

/// Fold every group of contacts that share an exact email or phone into one card (the
/// one with the most details first), with Undo. Pairs marked "Not the same person" stay apart.
fn merge_all(ui: &Ui) {
    let pairs = dup_pairs(ui);
    if pairs.is_empty() {
        toast(ui, "No contacts share an email or phone.");
        return;
    }
    let before = ui.book.borrow().clone();
    let mut book = before.clone();
    // Groups joined through the pairs.
    let mut groups: Vec<Vec<String>> = vec![];
    for (a, b, _) in &pairs {
        let ga = groups.iter().position(|g| g.contains(a));
        let gb = groups.iter().position(|g| g.contains(b));
        match (ga, gb) {
            (Some(x), Some(y)) if x != y => {
                let other = groups.remove(x.max(y));
                groups[x.min(y)].extend(other);
            }
            (Some(_), Some(_)) => {}
            (Some(x), None) => groups[x].push(b.clone()),
            (None, Some(y)) => groups[y].push(a.clone()),
            (None, None) => groups.push(vec![a.clone(), b.clone()]),
        }
    }
    let weight =
        |c: &Contact| c.emails.len() + c.phones.len() + c.addresses.len() + c.websites.len() + usize::from(!c.notes.is_empty());
    let mut merged = 0;
    for group in &groups {
        let mut cards: Vec<Contact> =
            group.iter().filter_map(|id| before.contacts.iter().find(|c| c.id == *id).cloned()).collect();
        cards.sort_by_key(|c| std::cmp::Reverse(weight(c)));
        let mut primary = cards.remove(0);
        for other in &cards {
            primary = model::combine(primary, other);
        }
        dedupe_phones(&mut primary);
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
            Some((
                "Undo",
                Box::new(move || {
                    commit(&u, before.clone());
                }),
            )),
        );
    }
}

/// One of each phone number: `555-987-6543` and `+1 555-987-6543` are the same.
fn dedupe_phones(c: &mut Contact) {
    let mut seen = HashSet::new();
    c.phones.retain(|f| model::phone_key(&f.value).is_none_or(|k| seen.insert(k)));
}

pub fn is_remote(ui: &Ui) -> bool {
    ui.section.borrow().starts_with("eds:")
}

fn eds_uid(section: &str) -> Option<&str> {
    section.strip_prefix("eds:")
}

fn load_eds(ui: &Ui) {
    let u = ui.clone();
    let handle = gtk::gio::spawn_blocking(nyx_pim::contacts::list_address_books);
    glib::spawn_future_local(async move {
        match handle.await {
            Ok(Ok(mut books)) => {
                u.eds_ready.set(true);
                let names = prefs::get().source_names;
                for b in &mut books {
                    if let Some(name) = names.get(&b.uid) {
                        *u.book_names.borrow_mut().entry(b.uid.clone()).or_default() = b.name.clone();
                        b.name = name.clone();
                    }
                }
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
            go(&u, "all");
            return;
        }
        let known: HashSet<String> = u.eds_books.borrow().iter().map(|b| b.uid.clone()).collect();
        u.cards.borrow_mut().retain(|uid, _| known.contains(uid));
        u.people.borrow_mut().take();
        nav(&u);
        update_status(&u);
        layout(&u);
        for book in shown_books(&u) {
            load_book(&u, book.uid);
        }
    });
}
fn refresh_online(ui: &Ui) {
    if ui.eds_syncing.replace(true) {
        return;
    }
    nav(ui);
    update_status(ui);
    let u = ui.clone();
    let handle = gtk::gio::spawn_blocking(|| {
        nyx_pim::Registry::connect().and_then(|registry| registry.refresh_accounts(nyx_accounts_native::Service::Contacts))
    });
    glib::spawn_future_local(async move {
        let result = handle.await;
        u.eds_syncing.set(false);
        match result {
            Ok(Err(error)) => {
                *u.sync_error.borrow_mut() = Some(error.to_string());
                toast(&u, &format!("Cached contacts are available; synchronization needs attention: {error}"));
            }
            Ok(Ok(())) => *u.sync_error.borrow_mut() = None,
            Err(_) => {}
        }
        load_eds(&u);
    });
}

/// An account book's cards, read from its vCards.
#[derive(Clone, Default)]
pub struct BookCards {
    pub contacts: Vec<Contact>,
    pub raw: HashMap<String, String>,
}

/// Reads a book's cards; then the pages that show them.
fn load_book(ui: &Ui, uid: String) {
    if !ui.loading.borrow_mut().insert(uid.clone()) {
        return;
    }
    let u = ui.clone();
    let requested = uid.clone();
    let handle = gtk::gio::spawn_blocking(move || nyx_pim::contacts::list_vcards(&uid));
    glib::spawn_future_local(async move {
        let result = handle.await;
        u.loading.borrow_mut().remove(&requested);
        let mut cards = BookCards::default();
        if let Ok(Ok(vcards)) = result {
            for card in vcards {
                let mut imported = crate::formats::vcard_read(&card);
                if let Some(mut c) = imported.contacts.pop() {
                    if let Some(id) = vcard_uid(&card) {
                        c.id = id;
                    }
                    cards.raw.insert(c.id.clone(), card.clone());
                    cards.contacts.push(c);
                }
            }
        }
        u.cards.borrow_mut().insert(requested.clone(), cards);
        u.people.borrow_mut().take();
        let section = u.section.borrow().clone();
        if matches!(section.as_str(), "all" | "favorites" | "birthdays") || eds_uid(&section) == Some(requested.as_str()) {
            refresh(&u);
        } else {
            nav(&u);
        }
    });
}

/// A book's cards, as last read.
fn book_cards(ui: &Ui, uid: &str) -> Vec<Contact> {
    ui.cards.borrow().get(uid).map(|b| b.contacts.clone()).unwrap_or_default()
}

/// Whether a source (`local`, or a book's id) is hidden in Settings › Sources.
fn hidden(key: &str) -> bool {
    prefs::get().hidden_sources.iter().any(|h| h == key)
}

/// The account books Contacts shows.
fn shown_books(ui: &Ui) -> Vec<nyx_pim::contacts::AddressBook> {
    ui.eds_books.borrow().iter().filter(|b| !hidden(&b.uid)).cloned().collect()
}

/// Everyone in the shown sources, once: this computer first, then the books you can
/// change, then read-only ones.
pub fn everyone(ui: &Ui) -> Vec<Person> {
    if let Some(p) = ui.people.borrow().as_ref() {
        return p.clone();
    }
    let mut sources = Vec::new();
    if !hidden("local") {
        sources.push((Origin::Local, ui.book.borrow().contacts.clone()));
    }
    let mut books = shown_books(ui);
    books.sort_by_key(|b| !b.writable);
    for b in books {
        sources.push((Origin::Book(b.uid.clone()), book_cards(ui, &b.uid)));
    }
    let people = people::combine(sources);
    *ui.people.borrow_mut() = Some(people.clone());
    people
}

fn person(ui: &Ui, key: &str) -> Option<Person> {
    everyone(ui).into_iter().find(|p| p.contact.id == key)
}

/// All and Favorites show people (copies combined); the other pages, one source's cards.
fn people_page(ui: &Ui) -> bool {
    matches!(ui.section.borrow().as_str(), "all" | "favorites")
}

/// Where a source is, for people: "On this computer", or the book's name.
fn origin_name(ui: &Ui, origin: &Origin) -> (String, bool) {
    match origin {
        Origin::Local => ("On this computer".into(), true),
        Origin::Book(uid) => ui
            .eds_books
            .borrow()
            .iter()
            .find(|b| b.uid == *uid)
            .map(|b| (b.label().to_string(), b.writable))
            .unwrap_or_else(|| (uid.clone(), false)),
    }
}

/// Whether the shown contact `id` has a copy on this computer, and one you can change.
fn copies(ui: &Ui, id: &str) -> (bool, bool) {
    if people_page(ui) {
        let Some(p) = person(ui, id) else { return (false, false) };
        let local = p.local().is_some();
        let writable = p.members.iter().any(|(o, _)| origin_name(ui, o).1);
        (local, writable)
    } else if is_remote(ui) {
        (false, online_book(ui).is_some_and(|b| b.writable))
    } else {
        (true, true)
    }
}

fn vcard_uid(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        line.get(..4).filter(|p| p.eq_ignore_ascii_case("uid:")).map(|_| line[4..].trim().to_string())
    })
}

fn lookup(ui: &Ui, id: &str) -> Option<Contact> {
    if let Some(uid) = eds_uid(&ui.section.borrow()) {
        book_cards(ui, uid).into_iter().find(|c| c.id == id)
    } else if people_page(ui) {
        person(ui, id).map(|p| p.contact)
    } else {
        ui.book.borrow().contacts.iter().find(|c| c.id == id).cloned()
    }
}

/// Leaves the editor first (asking when that would lose changes), then runs `then`.
pub fn leave_editor(ui: &Ui, then: impl Fn() + 'static) {
    let pending = ui.editing.borrow().as_ref().map(|e| (e.dirty.clone(), e.close.clone()));
    let Some((dirty, close)) = pending else {
        then();
        return;
    };
    if !dirty() {
        close();
        then();
        return;
    }
    editor::ask_discard(
        ui,
        dirty,
        Rc::new(move || {
            close();
            then();
        }),
    );
}

/// The editor opened or closed in the details pane.
pub fn set_editing(ui: &Ui, on: bool) {
    if on {
        ui.showing_detail.set(true);
        update_selbar(ui);
        layout(ui);
    } else {
        ui.target.borrow_mut().take();
        ui.showing_detail.set(ui.selected.borrow().len() == 1);
        refresh(ui);
    }
}

pub fn navigate(ui: &Ui, section: &str) {
    if section == "settings" {
        settings();
        return;
    }
    let (u, target) = (ui.clone(), section.to_string());
    leave_editor(ui, move || go(&u, &target));
}
fn go(ui: &Ui, section: &str) {
    let eds_ok = eds_uid(section).is_some_and(|uid| ui.eds_books.borrow().iter().any(|b| b.uid == uid));
    let valid = matches!(section, "all" | "local" | "favorites" | "birthdays" | "duplicates" | "online")
        || ui.book.borrow().lists.iter().any(|l| l.id == section)
        || eds_ok;
    *ui.section.borrow_mut() = if valid { section.into() } else { "all".into() };
    ui.selected.borrow_mut().clear();
    ui.showing_detail.set(false);
    if !is_remote(ui) {
        prefs::update(|p| p.last_section = ui.section.borrow().clone());
    }
    let book = eds_uid(&ui.section.borrow()).map(str::to_string);
    if let Some(uid) = book
        && !ui.cards.borrow().contains_key(&uid)
    {
        load_book(ui, uid);
    }
    refresh(ui);
}
fn sort_key(c: &Contact, sort_last: bool) -> String {
    let key = if sort_last { format!("{} {}", c.last, c.first) } else { format!("{} {}", c.first, c.last) };
    if key.trim().is_empty() { c.name(false).to_lowercase() } else { key.trim().to_lowercase() }
}
pub fn visible(ui: &Ui) -> Vec<Contact> {
    let book = ui.book.borrow();
    let section = ui.section.borrow();
    let query = ui.search.text();
    let p = prefs::get();
    if matches!(section.as_str(), "online" | "birthdays" | "duplicates") {
        return vec![];
    }
    let mut result: Vec<Contact> = if let Some(uid) = eds_uid(section.as_str()) {
        book_cards(ui, uid).into_iter().filter(|c| c.matches(&query)).collect()
    } else if matches!(section.as_str(), "all" | "favorites") {
        let favorites = section.as_str() == "favorites";
        drop(book);
        everyone(ui)
            .into_iter()
            .map(|p| p.contact)
            .filter(|c| (!favorites || c.favorite) && c.matches(&query))
            .collect()
    } else {
        book.contacts
            .iter()
            .filter(|c| {
                (match section.as_str() {
                    "local" => true,
                    id => book.lists.iter().find(|l| l.id == id).is_some_and(|l| l.members.contains(&c.id)),
                }) && c.matches(&query)
            })
            .cloned()
            .collect()
    };
    result.sort_by_cached_key(|c| sort_key(c, p.sort_last));
    result
}
pub fn chosen(ui: &Ui) -> Vec<Contact> {
    let selected = ui.selected.borrow();
    if selected.is_empty() {
        visible(ui)
    } else {
        let selected = selected.clone();
        visible(ui).into_iter().filter(|c| selected.contains(&c.id)).collect()
    }
}

/// The section's name: the list header's title and the window's.
fn section_name(ui: &Ui) -> String {
    let section = ui.section.borrow().clone();
    match section.as_str() {
        "all" => "All contacts".into(),
        "local" => "On this computer".into(),
        "favorites" => "Favorites".into(),
        "birthdays" => "Birthdays".into(),
        "duplicates" => "Duplicates".into(),
        "online" => "Accounts".into(),
        id => online_book(ui)
            .map(|b| b.label().to_string())
            .or_else(|| ui.book.borrow().lists.iter().find(|l| l.id == id).map(|l| l.name.clone()))
            .unwrap_or_default(),
    }
}

/// Whether the section shows as a page of its own rather than the list and details.
fn is_full(ui: &Ui) -> bool {
    ui.editing.borrow().is_none()
        && (matches!(ui.section.borrow().as_str(), "birthdays" | "duplicates" | "online")
            || (ui.shown.get() == 0 && ui.search.text().is_empty()))
}

/// The letter a contact is filed under in the list.
fn letter(c: &Contact, sort_last: bool) -> String {
    sort_key(c, sort_last)
        .chars()
        .find(|ch| !ch.is_whitespace())
        .map(|ch| if ch.is_alphabetic() { ch.to_uppercase().collect() } else { "#".to_string() })
        .unwrap_or_else(|| "#".into())
}

/// What a row says under the name: job title and company, or an email or phone.
fn row_sub(c: &Contact) -> String {
    let work = [c.title.as_str(), c.company.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    if !work.is_empty() {
        return work;
    }
    c.emails.first().or(c.phones.first()).map(|f| f.value.clone()).unwrap_or_default()
}

pub fn refresh(ui: &Ui) {
    ui.rebuilding.set(true);
    while let Some(c) = ui.list.first_child() {
        ui.list.remove(&c);
    }
    let contacts = visible(ui);
    ui.shown.set(contacts.len());
    let p = prefs::get();
    ui.sort_label.set_text(if p.sort_last { "Last name" } else { "First name" });
    let selected = ui.selected.borrow().clone();
    let alive: Vec<_> = selected.into_iter().filter(|id| contacts.iter().any(|c| c.id == *id)).collect();
    *ui.selected.borrow_mut() = alive.clone();
    let multi = alive.len() > 1;
    let mut last = String::new();
    for c in &contacts {
        let l = letter(c, p.sort_last);
        if l != last {
            let row = gtk::ListBoxRow::new();
            row.set_selectable(false);
            row.set_activatable(false);
            row.set_can_focus(false);
            row.add_css_class("letter-row");
            row.set_child(Some(&plain(&l, "letter")));
            ui.list.append(&row);
            last = l;
        }
        let row = contact_row(ui, c, multi, alive.contains(&c.id), p.last_first);
        ui.list.append(&row);
        if alive.contains(&c.id) {
            ui.list.select_row(Some(&row));
        }
    }
    if contacts.is_empty() {
        let text = if ui.search.text().is_empty() {
            "No one here yet.".to_string()
        } else {
            format!("No one matches “{}”.", ui.search.text())
        };
        let row = gtk::ListBoxRow::new();
        row.add_css_class("list-empty-row");
        row.set_selectable(false);
        row.set_activatable(false);
        let l = label(&text, "dim");
        l.set_selectable(false);
        l.set_justify(gtk::Justification::Center);
        l.set_xalign(0.5);
        row.set_child(Some(&l));
        ui.list.append(&row);
    }
    ui.rebuilding.set(false);
    let name = section_name(ui);
    chrome::set_title(&ui.window, "Contacts", &name);
    ui.list_title.set_text(&name);
    ui.list_count.set_text(&contacts.len().to_string());
    list_banner(ui);
    nav(ui);
    if is_full(ui) {
        build_full(ui);
    }
    details(ui);
    update_selbar(ui);
    update_status(ui);
    layout(ui);
}

fn contact_row(ui: &Ui, c: &Contact, multi: bool, selected: bool, last_first: bool) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&c.id);
    row.add_css_class("contact-row");
    let line = w::hbox(10);
    let check = gtk::CheckButton::new();
    check.add_css_class("row-check");
    check.set_visible(multi);
    check.set_active(selected);
    check.set_can_focus(false);
    check.set_valign(gtk::Align::Center);
    let (u, r) = (ui.clone(), row.downgrade());
    check.connect_toggled(move |c| {
        if u.rebuilding.get() {
            return;
        }
        let Some(r) = r.upgrade() else { return };
        if c.is_active() {
            u.list.select_row(Some(&r));
        } else {
            u.list.unselect_row(&r);
        }
    });
    line.append(&check);
    line.append(&avatar(c, 32));
    let text = w::vbox(1);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&plain(&c.name(last_first), "contact-row-name"));
    let sub = row_sub(c);
    if !sub.is_empty() {
        text.append(&plain(&sub, "contact-row-sub"));
    }
    line.append(&text);
    if c.favorite {
        let star = gtk::Image::from_icon_name("starred-symbolic");
        star.add_css_class("row-star");
        line.append(&star);
    }
    row.set_child(Some(&line));
    row
}

fn on_select(ui: &Ui) {
    if ui.rebuilding.get() {
        return;
    }
    let ids: Vec<String> =
        ui.list.selected_rows().iter().map(|r| r.widget_name().to_string()).filter(|n| !n.is_empty()).collect();
    if ids == *ui.selected.borrow() {
        return;
    }
    if ui.editing.borrow().is_some() {
        let before = ui.selected.borrow().clone();
        restore_selection(ui, &before);
        let u = ui.clone();
        leave_editor(ui, move || {
            restore_selection(&u, &ids);
            apply_selection(&u, ids.clone());
        });
        return;
    }
    apply_selection(ui, ids);
}
fn apply_selection(ui: &Ui, ids: Vec<String>) {
    ui.showing_detail.set(ids.len() == 1);
    *ui.selected.borrow_mut() = ids;
    sync_checks(ui);
    details(ui);
    update_selbar(ui);
    layout(ui);
}
fn restore_selection(ui: &Ui, ids: &[String]) {
    ui.rebuilding.set(true);
    ui.list.unselect_all();
    let mut child = ui.list.first_child();
    while let Some(c) = child {
        if let Some(row) = c.downcast_ref::<gtk::ListBoxRow>()
            && ids.iter().any(|id| *id == row.widget_name())
        {
            ui.list.select_row(Some(row));
        }
        child = c.next_sibling();
    }
    ui.rebuilding.set(false);
}
/// Checkboxes on every row while several people are selected.
fn sync_checks(ui: &Ui) {
    let multi = ui.selected.borrow().len() > 1;
    ui.rebuilding.set(true);
    let mut child = ui.list.first_child();
    while let Some(c) = child {
        if let Some(row) = c.downcast_ref::<gtk::ListBoxRow>()
            && let Some(check) = row.child().and_then(|l| l.first_child()).and_downcast::<gtk::CheckButton>()
        {
            check.set_visible(multi);
            check.set_active(row.is_selected());
        }
        child = c.next_sibling();
    }
    ui.rebuilding.set(false);
}

fn update_selbar(ui: &Ui) {
    let n = ui.selected.borrow().len();
    let show = n > 1 && ui.editing.borrow().is_none() && !is_full(ui);
    ui.selbar.set_visible(show);
    ui.selbar_count.set_text(&format!("{n} selected"));
    let section = ui.section.borrow().clone();
    ui.selbar_remove.set_visible(ui.book.borrow().lists.iter().any(|l| l.id == section));
    ui.selbar_add.set_visible(!is_remote(ui));
}

fn update_status(ui: &Ui) {
    let (icon, text, class) = if ui.eds_syncing.get() {
        ("emblem-synchronizing-symbolic", "Syncing accounts…".to_string(), "")
    } else if ui.sync_error.borrow().is_some() {
        ("dialog-warning-symbolic", "Sync needs attention".to_string(), "warn")
    } else if !ui.eds_books.borrow().is_empty() {
        ("emblem-ok-symbolic", "All accounts up to date".to_string(), "ok")
    } else {
        ("drive-harddisk-symbolic", "Kept on this computer".to_string(), "")
    };
    ui.foot_icon.set_icon_name(Some(icon));
    for c in ["ok", "warn"] {
        ui.foot_icon.remove_css_class(c);
    }
    if !class.is_empty() {
        ui.foot_icon.add_css_class(class);
    }
    ui.status.set_text(&text);
    let tip = ui.sync_error.borrow().clone().unwrap_or(text);
    ui.status.set_tooltip_text(Some(&tip));
    ui.foot_icon.set_tooltip_text(Some(&tip));
}

/// The read-only banner over an account book's list.
fn list_banner(ui: &Ui) {
    clear(&ui.list_banner);
    let Some(book) = online_book(ui).filter(|b| !b.writable) else {
        ui.list_banner.set_visible(false);
        return;
    };
    let banner = w::hbox(10);
    banner.add_css_class("ro-banner");
    let icon = gtk::Image::from_icon_name("system-lock-screen-symbolic");
    icon.set_valign(gtk::Align::Start);
    banner.append(&icon);
    let text = w::vbox(2);
    text.append(&plain(&format!("{} is read only", book.label()), "banner-title"));
    let sub = label("Copy someone to On this computer to change them.", "banner-text");
    sub.set_selectable(false);
    text.append(&sub);
    banner.append(&text);
    ui.list_banner.append(&banner);
    ui.list_banner.set_visible(true);
}

// ---------- The sidebar ----------

enum End {
    None,
    Count(usize),
    Sub(String),
    SubCount(String, usize),
    Badge(usize),
}

pub fn list_color(id: &str) -> usize {
    prefs::get().list_colors.get(id).copied().unwrap_or_else(|| id.bytes().map(usize::from).sum::<usize>() % SWATCHES.len())
}

/// The swatches' colours, in `SWATCHES` order (style.css's `.swatch-N` match).
const SWATCH_RGB: [(u8, u8, u8); 8] = [
    (0x3d, 0x8a, 0xe8),
    (0x8b, 0x6c, 0xf0),
    (0xc9, 0x5f, 0xc6),
    (0xf2, 0x65, 0x5f),
    (0xf5, 0x87, 0x5a),
    (0xee, 0xb4, 0x3f),
    (0x59, 0xd4, 0x5b),
    (0x3c, 0xc4, 0xc9),
];

/// A colour dot from the swatches.
pub fn dot(index: usize, size: i32) -> gtk::DrawingArea {
    let d = gtk::DrawingArea::new();
    d.add_css_class("list-dot");
    d.set_content_width(size);
    d.set_content_height(size);
    d.set_valign(gtk::Align::Center);
    d.set_halign(gtk::Align::Center);
    let (r, g, b) = SWATCH_RGB[index % SWATCH_RGB.len()];
    d.set_draw_func(move |_, cr, w, h| {
        cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        cr.arc(w as f64 / 2.0, h as f64 / 2.0, w.min(h) as f64 / 2.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    });
    d
}

fn nav_item(ui: &Ui, lead: gtk::Widget, title: &str, end: End, section: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("nav-item");
    if *ui.section.borrow() == section {
        button.add_css_class("active");
    }
    let content = w::hbox(12);
    content.append(&lead);
    let text = plain(title, "nav-label");
    text.set_hexpand(true);
    text.add_css_class("compact-hide");
    content.append(&text);
    let readout = |t: &str, class: &str| {
        let l = w::label(t, class);
        l.add_css_class("compact-hide");
        l.set_valign(gtk::Align::Center);
        l
    };
    match end {
        End::None => {}
        End::Count(n) => content.append(&readout(&n.to_string(), "nav-readout")),
        End::Sub(s) => content.append(&readout(&s, "nav-sub")),
        End::SubCount(s, n) => {
            content.append(&readout(&s, "nav-sub"));
            content.append(&readout(&n.to_string(), "nav-readout"));
        }
        End::Badge(n) => content.append(&readout(&n.to_string(), "nav-badge")),
    }
    button.set_child(Some(&content));
    button.set_tooltip_text(Some(title));
    let (u, id) = (ui.clone(), section.to_string());
    button.connect_clicked(move |_| navigate(&u, &id));
    ui.nav.append(&button);
    button
}
fn icon(name: &str) -> gtk::Widget {
    gtk::Image::from_icon_name(name).upcast()
}
/// A group heading's + button: its tooltip and what it does.
type GroupAdd<'a> = Option<(&'a str, Box<dyn Fn()>)>;
/// A group's heading, in its section's colour (`accent` for the first, else a swatch),
/// and the icon rail's bar in the same colour.
fn nav_group(ui: &Ui, title: &str, section: &str, add: GroupAdd<'_>) {
    let bar = gtk::Separator::new(gtk::Orientation::Horizontal);
    bar.add_css_class("nav-divider");
    bar.add_css_class("compact-show");
    bar.add_css_class(&format!("section-{section}"));
    bar.set_visible(false);
    ui.nav.append(&bar);
    let row = match add {
        Some((tip, run)) => nyx_ui::sidebar::group_label_with_add(title, false, tip, run),
        None => nyx_ui::sidebar::group_label(title, false),
    };
    nyx_ui::sidebar::set_section(&row, section);
    ui.nav.append(&row);
}
fn nav(ui: &Ui) {
    clear(&ui.nav);
    let everyone = everyone(ui);
    let (all, favorites, birthdays) = (
        everyone.len(),
        everyone.iter().filter(|p| p.contact.favorite).count(),
        crate::control::upcoming(&Book { contacts: everyone.iter().map(|p| p.contact.clone()).collect(), lists: vec![] }, 30).len(),
    );
    let (local, lists) = {
        let book = ui.book.borrow();
        (book.contacts.len(), book.lists.iter().map(|l| (l.id.clone(), l.name.clone(), l.members.len())).collect::<Vec<_>>())
    };
    nav_item(ui, icon("system-users-symbolic"), "All contacts", End::Count(all), "all");
    nav_item(ui, icon("non-starred-symbolic"), "Favorites", End::Count(favorites), "favorites");
    let u = ui.clone();
    nav_group(ui, "Lists", nyx_ui::sidebar::ACCENT_SECTION, Some(("New list", Box::new(move || new_list(&u, vec![])))));
    for (id, name, count) in lists {
        let b = nav_item(ui, dot(list_color(&id), 10).upcast(), &name, End::Count(count), &id);
        let u = ui.clone();
        menu::on_right_click(&b, move |w, x, y| menu::at(w, x, y, list_entries(&u, &id)));
    }
    nav_group(ui, "Accounts", "swatch-green", None);
    let books = shown_books(ui);
    let section = ui.section.borrow().clone();
    for book in &books {
        let id = format!("eds:{}", book.uid);
        let (lead, end) = if !book.writable {
            ("system-lock-screen-symbolic", End::Sub("read only".into()))
        } else if ui.eds_syncing.get() {
            ("weather-overcast-symbolic", End::Sub("syncing…".into()))
        } else {
            ("weather-overcast-symbolic", End::Count(book_cards(ui, &book.uid).len()))
        };
        nav_item(ui, icon(lead), book.label(), end, &id);
    }
    if ui.eds_books.borrow().is_empty() {
        let b = nav_item(ui, icon("list-add-symbolic"), "Add an account", End::None, "online");
        b.add_css_class("nav-add");
    }
    if !hidden("local") {
        nav_item(ui, icon("drive-harddisk-symbolic"), "On this computer", End::Count(local), "local");
    }
    nav_group(ui, "Tidy up", "swatch-violet", None);
    nav_item(ui, icon("nyx-contacts-cake-symbolic"), "Birthdays", End::SubCount("next 30 days".into(), birthdays), "birthdays");
    let pairs = dup_pairs(ui).len();
    if pairs > 0 || section == "duplicates" {
        nav_item(ui, icon("nyx-contacts-merge-symbolic"), "Duplicates", End::Badge(pairs), "duplicates");
    }
    let compact = ui.sidebar.has_css_class("compact");
    apply_compact(&ui.sidebar, compact);
}

fn apply_compact(root: &gtk::Box, compact: bool) {
    if compact {
        root.add_css_class("compact");
    } else {
        root.remove_css_class("compact");
    }
    fn walk(widget: &gtk::Widget, compact: bool) {
        if widget.has_css_class("nav-item")
            && let Some(content) = widget.downcast_ref::<gtk::Button>().and_then(|b| b.child())
        {
            content.set_halign(if compact { gtk::Align::Center } else { gtk::Align::Fill });
        }
        if widget.has_css_class("compact-hide") {
            widget.set_visible(!compact);
        }
        if widget.has_css_class("compact-show") {
            widget.set_visible(compact);
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            walk(&c, compact);
            child = c.next_sibling();
        }
    }
    walk(root.upcast_ref(), compact);
}

pub fn layout(ui: &Ui) {
    let width = if ui.window.width() > 0 { ui.window.width() } else { ui.window.default_width() };
    nyx_ui::settings_card::fit(&ui.window);
    let compact = (width > 0 && width < 980) || prefs::get().sidebar_collapsed;
    let narrow = width > 0 && width < 720;
    ui.narrow.set(narrow);
    if compact != ui.sidebar.has_css_class("compact") || !ui.sidebar.has_css_class("sized") {
        ui.sidebar.add_css_class("sized");
        apply_compact(&ui.sidebar, compact);
    }
    let full = is_full(ui);
    ui.views.set_visible_child_name(if full { "full" } else { "panes" });
    if !full {
        let detail = narrow && ui.showing_detail.get();
        ui.list_col.set_visible(!narrow || !detail);
        ui.detail_col.set_visible(!narrow || detail);
        ui.pane_line.set_visible(!narrow);
        ui.back_bar.set_visible(narrow);
        ui.list_col.set_hexpand(narrow);
        ui.list_col.set_width_request(if narrow { -1 } else { 300 });
    }
}

// ---------- Details ----------

fn section_label(text: &str) -> gtk::Label {
    plain(text, "section-label")
}
fn card() -> gtk::Box {
    let c = w::vbox(0);
    c.add_css_class("field-card");
    c
}

/// A value's row: its label, the value (and a line under it), and actions shown over its
/// end on hover, so they take no room from the value.
fn field_row(name: &str, value: &str, under: &str, link: bool, acts: Vec<gtk::Button>) -> gtk::Overlay {
    let row = gtk::Overlay::new();
    row.add_css_class("field-row");
    let line = w::hbox(12);
    let l = plain(name, "field-label");
    l.set_width_chars(8);
    l.set_max_width_chars(8);
    l.set_valign(gtk::Align::Center);
    line.append(&l);
    let col = w::vbox(1);
    col.set_hexpand(true);
    col.set_valign(gtk::Align::Center);
    let v = label(value, if link { "field-link" } else { "field-value" });
    v.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    col.append(&v);
    if !under.is_empty() {
        col.append(&label(under, "field-under"));
    }
    line.append(&col);
    row.set_child(Some(&line));
    if !acts.is_empty() {
        let box_ = w::hbox(2);
        box_.add_css_class("field-acts");
        box_.set_halign(gtk::Align::End);
        box_.set_valign(gtk::Align::Center);
        for a in acts {
            box_.append(&a);
        }
        row.add_overlay(&box_);
    }
    row
}
fn act(ui: &Ui, icon: &str, tip: &str, run: impl Fn(&Ui) + 'static) -> gtk::Button {
    let b = w::bar_button(icon, tip);
    let u = ui.clone();
    b.connect_clicked(move |_| run(&u));
    b
}
fn copy_act(ui: &Ui, value: String) -> gtk::Button {
    act(ui, "edit-copy-symbolic", "Copy", move |u| {
        u.window.clipboard().set_text(&value);
        toast(u, "Copied.");
    })
}
fn uri_act(ui: &Ui, icon: &str, tip: &str, uri: String) -> gtk::Button {
    act(ui, icon, tip, move |u| launch(u, &uri))
}
/// `acts` and `more`, if there is one.
fn acts_for(mut acts: Vec<gtk::Button>, more: Option<gtk::Button>) -> Vec<gtk::Button> {
    acts.extend(more);
    acts
}

thread_local! {
    /// Whether an app on this system opens each kind of link (mailto, tel, sms, https).
    static HANDLERS: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
}
/// Whether some app here opens `scheme:` links: no Call without a phone app, no Message
/// without a messaging one.
pub fn can_open(scheme: &str) -> bool {
    HANDLERS.with(|h| {
        *h.borrow_mut()
            .entry(scheme.to_string())
            .or_insert_with(|| gtk::gio::AppInfo::default_for_uri_scheme(scheme).is_some())
    })
}

fn mailto(value: &str) -> String {
    format!("mailto:{}", glib::uri_escape_string(value, Some("@+"), false))
}
fn tel(value: &str, scheme: &str) -> String {
    format!("{scheme}:{}", value.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect::<String>())
}
fn web(value: &str) -> String {
    if value.starts_with("https://") || value.starts_with("http://") { value.to_string() } else { format!("https://{value}") }
}

/// "Turns 41 in 5 months", "Turns 41 tomorrow", or without a year "In 5 days".
fn countdown(birthday: &str) -> String {
    let Some((n, age)) = crate::control::next_birthday(birthday) else { return String::new() };
    let when = match n {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        n if n < 45 => format!("in {n} days"),
        n => {
            let m = ((n as f64) / 30.44).round() as i64;
            if m >= 12 { "in a year".to_string() } else { format!("in {m} months") }
        }
    };
    match age {
        Some(a) => format!("Turns {a} {when}"),
        None => {
            let mut s = when;
            if let Some(f) = s.get_mut(..1) {
                f.make_ascii_uppercase();
            }
            s
        }
    }
}

pub fn details(ui: &Ui) {
    if ui.editing.borrow().is_some() {
        return;
    }
    clear(&ui.details);
    ui.details.remove_css_class("editing");
    let ids = ui.selected.borrow().clone();
    if ids.len() != 1 {
        let empty = w::vbox(8);
        empty.add_css_class("detail-empty");
        empty.set_valign(gtk::Align::Center);
        empty.set_halign(gtk::Align::Center);
        empty.set_vexpand(true);
        let i = gtk::Image::from_icon_name("system-users-symbolic");
        i.set_pixel_size(36);
        i.add_css_class("dim");
        empty.append(&i);
        let (title, text) = if ids.is_empty() {
            ("No one selected".to_string(), "Choose someone in the list, or press Ctrl N to add someone.".to_string())
        } else {
            (format!("{} contacts selected", ids.len()), "Choose what to do with them below, or Ctrl-click to add more.".into())
        };
        let t = w::label(&title, "empty-title");
        empty.append(&t);
        let m = label(&text, "dim");
        m.set_selectable(false);
        m.set_max_width_chars(42);
        m.set_justify(gtk::Justification::Center);
        m.set_xalign(0.5);
        empty.append(&m);
        ui.details.append(&empty);
        return;
    }
    let Some(c) = lookup(ui, &ids[0]) else { return };
    let (local, writable) = copies(ui, &ids[0]);
    let book = online_book(ui);

    // Who: the photo, the name with its star, the job, and where they're kept.
    let head = w::vbox(6);
    head.add_css_class("detail-head");
    head.set_halign(gtk::Align::Center);
    let av = avatar(&c, 88);
    av.set_halign(gtk::Align::Center);
    head.append(&av);
    let name_line = w::hbox(8);
    name_line.set_halign(gtk::Align::Center);
    name_line.add_css_class("detail-name-line");
    let name = w::label(&c.name(prefs::get().last_first), "detail-name");
    name.set_selectable(true);
    name.set_wrap(true);
    name.set_justify(gtk::Justification::Center);
    name_line.append(&name);
    if local {
        let star = w::bar_button(
            if c.favorite { "starred-symbolic" } else { "non-starred-symbolic" },
            if c.favorite { "Remove from Favorites" } else { "Add to Favorites" },
        );
        star.add_css_class("star-toggle");
        if c.favorite {
            star.add_css_class("on");
        }
        let id = c.id.clone();
        let u = ui.clone();
        star.connect_clicked(move |_| toggle_favorite(&u, &id));
        name_line.append(&star);
    } else if c.favorite {
        let star = gtk::Image::from_icon_name("starred-symbolic");
        star.add_css_class("row-star");
        name_line.append(&star);
    }
    head.append(&name_line);
    let work = [c.title.as_str(), c.company.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    if !work.is_empty() {
        let l = w::label(&work, "detail-title");
        l.set_selectable(true);
        l.set_xalign(0.5);
        l.set_halign(gtk::Align::Center);
        head.append(&l);
    }
    let where_ = w::hbox(5);
    where_.add_css_class("detail-where");
    where_.set_halign(gtk::Align::Center);
    let (wicon, wtext) = if people_page(ui) {
        // Every place a copy is kept.
        let names: Vec<String> = person(ui, &ids[0])
            .map(|p| {
                let mut names: Vec<String> = Vec::new();
                for (o, _) in &p.members {
                    let n = origin_name(ui, o).0;
                    if !names.contains(&n) {
                        names.push(n);
                    }
                }
                names
            })
            .unwrap_or_default();
        let icon = if local { "drive-harddisk-symbolic" } else if writable { "weather-overcast-symbolic" } else { "system-lock-screen-symbolic" };
        (icon, names.join(" · ") + if writable { "" } else { " · read only" })
    } else {
        match &book {
            None => ("drive-harddisk-symbolic", "On this computer".to_string()),
            Some(b) if !b.writable => ("system-lock-screen-symbolic", format!("{} · read only", b.label())),
            Some(b) => ("weather-overcast-symbolic", b.label().to_string()),
        }
    };
    let wicon = gtk::Image::from_icon_name(wicon);
    wicon.set_valign(gtk::Align::Start);
    where_.append(&wicon);
    // A person kept in several places names them all; long lists wrap.
    let wlabel = gtk::Label::new(Some(&wtext));
    wlabel.set_wrap(true);
    wlabel.set_max_width_chars(60);
    wlabel.set_justify(gtk::Justification::Center);
    where_.append(&wlabel);
    head.append(&where_);

    // What you can do with them.
    let acts = w::hbox(6);
    acts.add_css_class("contact-actions");
    acts.set_halign(gtk::Align::Center);
    // Only what can work: with the detail it needs, and an app for it on this system.
    let action = |icon: &str, text: &str, shown: bool, tip: &str| {
        let b = gtk::Button::new();
        b.add_css_class("contact-action");
        let face = w::vbox(5);
        let i = gtk::Image::from_icon_name(icon);
        face.append(&i);
        face.append(&gtk::Label::new(Some(text)));
        b.set_child(Some(&face));
        b.set_visible(shown);
        b.set_tooltip_text(Some(tip));
        acts.append(&b);
        b
    };
    let email = c.emails.first().map(|f| f.value.clone());
    let phone = c.phones.first().map(|f| f.value.clone());
    let b = action("nyx-contacts-mail-symbolic", "Email", email.is_some() && can_open("mailto"), "Write an email");
    if let Some(e) = email.clone() {
        let u = ui.clone();
        b.connect_clicked(move |_| launch(&u, &mailto(&e)));
    }
    let b = action("call-start-symbolic", "Call", phone.is_some() && can_open("tel"), "Call");
    if let Some(p) = phone.clone() {
        let u = ui.clone();
        b.connect_clicked(move |_| launch(&u, &tel(&p, "tel")));
    }
    let b = action("chat-symbolic", "Message", phone.is_some() && can_open("sms"), "Send a text message");
    if let Some(p) = phone.clone() {
        let u = ui.clone();
        b.connect_clicked(move |_| launch(&u, &tel(&p, "sms")));
    }
    let b = action("document-edit-symbolic", "Edit", writable, "Edit (Enter)");
    let u = ui.clone();
    b.connect_clicked(move |_| edit_selected(&u));
    let b = action("view-more-symbolic", "More", true, "More actions");
    let u = ui.clone();
    b.connect_clicked(move |b| menu::below(b, contact_entries(&u, false)));
    head.append(&acts);
    ui.details.append(&head);

    // Their details, a card for each kind.
    let section = |body: &gtk::Box, title: Option<&str>| {
        let s = w::vbox(6);
        s.add_css_class("detail-section");
        if let Some(t) = title {
            s.append(&section_label(t));
        }
        body.append(&s);
        s
    };
    if !c.emails.is_empty() {
        let k = card();
        for f in &c.emails {
            k.append(&field_row(
                &f.label,
                &f.value,
                "",
                true,
                acts_for(vec![copy_act(ui, f.value.clone())], can_open("mailto").then(|| uri_act(ui, "nyx-contacts-mail-symbolic", "Write an email", mailto(&f.value)))),
            ));
        }
        section(&ui.details, None).append(&k);
    }
    if !c.phones.is_empty() {
        let k = card();
        for f in &c.phones {
            k.append(&field_row(
                &f.label,
                &f.value,
                "",
                false,
                acts_for(
                    acts_for(vec![copy_act(ui, f.value.clone())], can_open("tel").then(|| uri_act(ui, "call-start-symbolic", "Call", tel(&f.value, "tel")))),
                    can_open("sms").then(|| uri_act(ui, "chat-symbolic", "Send a text message", tel(&f.value, "sms"))),
                ),
            ));
        }
        section(&ui.details, None).append(&k);
    }
    if !c.addresses.is_empty() {
        let k = card();
        for f in &c.addresses {
            let a = model::Address::parse(&f.value);
            let locality = [a.city.as_str(), &[a.region.as_str(), &a.postal].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            let main = [a.po_box.as_str(), &a.extended, &a.street, &locality]
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            let shown = a.display();
            let query = shown.replace('\n', ", ");
            let q = glib::uri_escape_string(&query, None::<&str>, false);
            k.append(&field_row(
                &f.label,
                if main.is_empty() { &a.country } else { &main },
                if main.is_empty() { "" } else { &a.country },
                false,
                acts_for(
                    vec![copy_act(ui, shown.clone())],
                    can_open("https").then(|| uri_act(ui, "mark-location-symbolic", "Show on a map", format!("https://www.openstreetmap.org/search?query={q}"))),
                ),
            ));
        }
        section(&ui.details, None).append(&k);
    }
    if !c.birthday.is_empty() || !c.websites.is_empty() {
        let k = card();
        if !c.birthday.is_empty() {
            k.append(&field_row("birthday", &birthday_text(&c.birthday), &countdown(&c.birthday), false, vec![]));
        }
        for f in &c.websites {
            k.append(&field_row(
                if f.label.is_empty() { "website" } else { &f.label },
                &f.value,
                "",
                true,
                acts_for(vec![copy_act(ui, f.value.clone())], can_open("https").then(|| uri_act(ui, "nyx-contacts-external-symbolic", "Open in the browser", web(&f.value)))),
            ));
        }
        section(&ui.details, None).append(&k);
    }
    if local {
        let s = section(&ui.details, Some("Lists"));
        let chips = gtk::FlowBox::new();
        chips.set_selection_mode(gtk::SelectionMode::None);
        chips.set_max_children_per_line(12);
        chips.set_row_spacing(6);
        chips.set_column_spacing(6);
        chips.add_css_class("list-chips");
        let lists: Vec<(String, String)> = ui
            .book
            .borrow()
            .lists
            .iter()
            .filter(|l| l.members.contains(&c.id))
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect();
        for (id, name) in lists {
            let b = gtk::Button::with_label(&name);
            b.add_css_class("chip");
            b.set_tooltip_text(Some(&format!("Show {name}")));
            let u = ui.clone();
            b.connect_clicked(move |_| navigate(&u, &id));
            chips.insert(&b, -1);
        }
        let add = icon_text_button("list-add-symbolic", "Add to a list", "chip");
        add.add_css_class("accent");
        let (u, id) = (ui.clone(), c.id.clone());
        add.connect_clicked(move |b| menu::below(b, add_to_list_entries(&u, vec![id.clone()])));
        chips.insert(&add, -1);
        s.append(&chips);
    }
    if !c.notes.trim().is_empty() {
        let s = section(&ui.details, Some("Notes"));
        let k = card();
        k.add_css_class("note-card");
        k.append(&label(c.notes.trim(), "note-text"));
        s.append(&k);
    }
}

/// A birthday for reading: "12 April 1985", or "12 April" without a year. Anything
/// Contacts doesn't read is shown as it is.
pub fn birthday_text(b: &str) -> String {
    let Some((m, d)) = model::birthday_md(b).filter(|(m, _)| (1..=12).contains(m)) else { return b.to_string() };
    let month = MONTHS[m as usize - 1];
    match b.get(..4).filter(|_| !b.starts_with("--")) {
        Some(y) => format!("{d} {month} {y}"),
        None => format!("{d} {month}"),
    }
}
const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

thread_local! {
    /// Photos cut square and scaled, by contact and size, with the photo's length to
    /// notice a new one.
    static PHOTOS: RefCell<HashMap<(String, i32), (usize, gdk::Texture)>> = RefCell::new(HashMap::new());
}
fn photo_texture(c: &Contact, size: i32) -> Option<gdk::Texture> {
    let key = (c.id.clone(), size);
    if let Some(t) = PHOTOS.with(|p| p.borrow().get(&key).filter(|(n, _)| *n == c.photo.len()).map(|(_, t)| t.clone())) {
        return Some(t);
    }
    use gtk::gdk_pixbuf::{InterpType, Pixbuf};
    let stream = gtk::gio::MemoryInputStream::from_bytes(&glib::Bytes::from(&c.photo[..]));
    let pb = Pixbuf::from_stream(&stream, None::<&gtk::gio::Cancellable>).ok()?;
    let side = pb.width().min(pb.height());
    let square = pb.new_subpixbuf((pb.width() - side) / 2, (pb.height() - side) / 2, side, side);
    let px = (size * 2).min(side).max(1);
    let scaled = square.scale_simple(px, px, InterpType::Bilinear)?;
    let texture = gdk::Texture::for_pixbuf(&scaled);
    PHOTOS.with(|p| p.borrow_mut().insert(key, (c.photo.len(), texture.clone())));
    Some(texture)
}

/// A round photo, or coloured initials.
pub fn avatar(c: &Contact, size: i32) -> gtk::Widget {
    let frame = w::hbox(0);
    frame.add_css_class("avatar");
    frame.set_size_request(size, size);
    frame.set_hexpand(false);
    frame.set_vexpand(false);
    frame.set_halign(gtk::Align::Start);
    frame.set_valign(gtk::Align::Center);
    frame.set_overflow(gtk::Overflow::Hidden);
    if !c.photo.is_empty()
        && let Some(t) = photo_texture(c, size)
    {
        let image = gtk::Image::from_paintable(Some(&t));
        image.set_pixel_size(size);
        frame.add_css_class("photo");
        frame.append(&image);
        return frame.upcast();
    }
    if [&c.first, &c.last, &c.display, &c.company].iter().all(|s| s.trim().is_empty()) && c.emails.is_empty() {
        frame.add_css_class("avatar-empty");
        let i = gtk::Image::from_icon_name("avatar-default-symbolic");
        i.set_pixel_size(size * 3 / 8);
        i.set_hexpand(true);
        i.set_halign(gtk::Align::Center);
        frame.append(&i);
        return frame.upcast();
    }
    let (first, last) = if c.first.is_empty() && c.last.is_empty() {
        let name = c.name(false);
        let mut parts = name.split_whitespace();
        (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string())
    } else {
        (c.first.clone(), c.last.clone())
    };
    let initials: String =
        [first.chars().next(), last.chars().next()].into_iter().flatten().collect::<String>().to_uppercase();
    let hue = format!("{first}{last}").encode_utf16().map(usize::from).sum::<usize>() % SWATCHES.len();
    frame.add_css_class(&format!("swatch-{hue}"));
    let l = gtk::Label::new(Some(&initials));
    l.add_css_class("avatar-initials");
    l.set_hexpand(true);
    l.set_halign(gtk::Align::Center);
    let attrs = gtk::pango::AttrList::new();
    attrs.insert(gtk::pango::AttrSize::new_size_absolute((size as f64 * 0.38 * gtk::pango::SCALE as f64) as i32));
    attrs.insert(gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Semibold));
    l.set_attributes(Some(&attrs));
    frame.append(&l);
    frame.upcast()
}
fn find(ui: &Ui) {
    ui.search.grab_focus();
}
fn edit_selected(ui: &Ui) {
    let Some(id) = ui.selected.borrow().first().cloned() else { return };
    // A person in All: the copy on this computer, else the first one you can change.
    if people_page(ui) {
        let Some(p) = person(ui, &id) else { return };
        let Some((origin, card)) = p.members.iter().find(|(o, _)| origin_name(ui, o).1).cloned() else {
            toast(ui, "Every copy of this person is read only. Copy them to On this computer to change them.");
            return;
        };
        *ui.target.borrow_mut() = Some(origin);
        editor::open(ui, Some(card));
        return;
    }
    if online_book(ui).is_some_and(|book| !book.writable) {
        toast(ui, "This address book is read only. Copy someone to On this computer to change them.");
        return;
    }
    let c = lookup(ui, &id);
    editor::open(ui, c);
}
fn new_contact(ui: &Ui) {
    let u = ui.clone();
    leave_editor(ui, move || {
        let section = u.section.borrow().clone();
        if matches!(section.as_str(), "birthdays" | "duplicates" | "online") {
            go(&u, "all");
        } else if online_book(&u).is_some_and(|b| !b.writable) {
            go(&u, "local");
        }
        editor::open(&u, None);
    });
}
fn toggle_favorite(ui: &Ui, id: &str) {
    let mut book = ui.book.borrow().clone();
    if let Some(c) = book.contacts.iter_mut().find(|c| c.id == id) {
        c.favorite = !c.favorite;
    }
    commit(ui, book);
}
/// A copy of an account's contact in this computer's book.
fn copy_to_local(ui: &Ui, c: &Contact) {
    let mut copy = c.clone();
    copy.id = model::id();
    copy.extra.retain(|e| !e.to_uppercase().starts_with("UID:"));
    let mut book = ui.book.borrow().clone();
    book.contacts.push(copy.clone());
    if commit(ui, book) {
        let u = ui.clone();
        toast_action(
            ui,
            &format!("Copied {} to On this computer.", copy.name(false)),
            Some((
                "Show",
                Box::new(move || {
                    go(&u, "local");
                    *u.selected.borrow_mut() = vec![copy.id.clone()];
                    u.showing_detail.set(true);
                    refresh(&u);
                }),
            )),
        );
    }
}

// ---------- Menus ----------

/// The ⋯ of a section: a list's actions, or the book's.
fn list_entries(ui: &Ui, section: &str) -> Vec<menu::Entry> {
    let list = ui.book.borrow().lists.iter().find(|l| l.id == section).cloned();
    if let Some(l) = list {
        let colours = SWATCHES
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let (u, id) = (ui.clone(), l.id.clone());
                menu::swatch(i, name, move || {
                    prefs::update(|p| {
                        p.list_colors.insert(id.clone(), i);
                    });
                    refresh(&u);
                })
            })
            .collect();
        let (u1, u2, u3, u4) = (ui.clone(), ui.clone(), ui.clone(), ui.clone());
        let (id1, id2, id3, id4) = (l.id.clone(), l.id.clone(), l.id.clone(), l.id.clone());
        return vec![
            menu::item("nyx-contacts-mail-symbolic", "Email everyone", move || email_list(&u1, &id1)).enabled(can_open("mailto")),
            menu::item("upload-symbolic", "Export this list…", move || {
                let people = list_members(&u2, &id2);
                transfer::export(&u2, people, false);
            }),
            menu::separator(),
            menu::item("document-edit-symbolic", "Rename…", move || rename_list(&u3, &id3)),
            menu::sub("preferences-color-symbolic", "Colour", colours),
            menu::separator(),
            menu::danger("user-trash-symbolic", "Delete this list", move || delete_list(&u4, &id4)),
            menu::note("Its people stay in your contacts"),
        ];
    }
    let mut entries = vec![];
    if !is_remote(ui) {
        let u = ui.clone();
        entries.push(menu::item("download-symbolic", "Import contacts…", move || transfer::choose_import(&u)));
    }
    let people = visible(ui);
    let (u1, u2, p1, p2) = (ui.clone(), ui.clone(), people.clone(), people);
    entries.push(menu::item("upload-symbolic", "Export as vCard…", move || transfer::export(&u1, p1.clone(), false)));
    entries.push(menu::item("upload-symbolic", "Export as CSV…", move || transfer::export(&u2, p2.clone(), true)));
    if is_remote(ui) {
        let u = ui.clone();
        entries.push(menu::separator());
        entries.push(menu::item("view-refresh-symbolic", "Sync now", move || refresh_online(&u)));
        if let Some(uid) = eds_uid(section) {
            let (u, uid) = (ui.clone(), uid.to_string());
            entries.push(menu::item("document-edit-symbolic", "Rename…", move || rename_source(&u, &uid, |_| {})));
        }
    } else {
        let u = ui.clone();
        entries.push(menu::separator());
        entries.push(
            menu::item("nyx-contacts-merge-symbolic", "Merge exact duplicates", move || merge_all(&u))
                .enabled(!dup_pairs(ui).is_empty()),
        );
    }
    entries
}

fn list_members(ui: &Ui, id: &str) -> Vec<Contact> {
    let book = ui.book.borrow();
    let Some(l) = book.lists.iter().find(|l| l.id == id) else { return vec![] };
    book.contacts.iter().filter(|c| l.members.contains(&c.id)).cloned().collect()
}

/// Lists to add `ids` to, and a new one.
fn add_to_list_entries(ui: &Ui, ids: Vec<String>) -> Vec<menu::Entry> {
    // Lists hold this computer's contacts; people only in an account book stay out.
    let ids: Vec<String> = {
        let book = ui.book.borrow();
        ids.into_iter().filter(|id| book.contacts.iter().any(|c| c.id == *id)).collect()
    };
    let lists = ui.book.borrow().lists.clone();
    let mut entries: Vec<menu::Entry> = lists
        .into_iter()
        .map(|l| {
            let (u, ids) = (ui.clone(), ids.clone());
            let all_in = ids.iter().all(|id| l.members.contains(id));
            let (color, name) = (list_color(&l.id), l.name.clone());
            menu::swatch(color, &name, move || {
                let mut book = u.book.borrow().clone();
                if let Some(list) = book.lists.iter_mut().find(|x| x.id == l.id) {
                    list.members.extend(ids.clone());
                }
                if commit(&u, book) {
                    toast(&u, &format!("Added to {}.", l.name));
                }
            })
            .enabled(!all_in)
        })
        .collect();
    if !entries.is_empty() {
        entries.push(menu::separator());
    }
    let u = ui.clone();
    entries.push(menu::item("list-add-symbolic", "New list…", move || new_list(&u, ids.clone())));
    entries
}

fn export_entries(ui: &Ui, people: Vec<Contact>) -> Vec<menu::Entry> {
    let (u1, u2, p1) = (ui.clone(), ui.clone(), people.clone());
    vec![
        menu::item("upload-symbolic", "As vCard…", move || transfer::export(&u1, p1.clone(), false)),
        menu::item("upload-symbolic", "As CSV…", move || transfer::export(&u2, people.clone(), true)),
    ]
}

/// The More menu (`edit` false: Edit is its own button there), and the right-click menu
/// of the list's selection.
fn contact_entries(ui: &Ui, edit: bool) -> Vec<menu::Entry> {
    let ids = ui.selected.borrow().clone();
    let (local, writable) = match ids.as_slice() {
        [one] => copies(ui, one),
        _ => (!is_remote(ui), !is_remote(ui) || online_book(ui).is_some_and(|b| b.writable)),
    };
    let section = ui.section.borrow().clone();
    let in_list = ui.book.borrow().lists.iter().any(|l| l.id == section);
    let mut e = vec![];
    if ids.len() == 1
        && let Some(c) = lookup(ui, &ids[0])
    {
        if writable && edit {
            let u = ui.clone();
            e.push(menu::item("document-edit-symbolic", "Edit", move || edit_selected(&u)).keyed("Enter"));
        }
        if local {
            let (u, id) = (ui.clone(), c.id.clone());
            e.push(menu::item(
                if c.favorite { "non-starred-symbolic" } else { "starred-symbolic" },
                if c.favorite { "Remove from Favorites" } else { "Add to Favorites" },
                move || toggle_favorite(&u, &id),
            ));
        }
        let (u, card) = (ui.clone(), c.clone());
        e.push(menu::item("edit-copy-symbolic", "Copy as vCard", move || {
            u.window.clipboard().set_text(&crate::formats::vcard_write(std::slice::from_ref(&card)));
            toast(&u, "Copied as vCard.");
        }));
        if !local {
            let (u, card) = (ui.clone(), c.clone());
            e.push(menu::item("drive-harddisk-symbolic", "Copy to On this computer", move || copy_to_local(&u, &card)));
        }
    } else if ids.len() > 1 && can_open("mailto") {
        let u = ui.clone();
        e.push(menu::item("nyx-contacts-mail-symbolic", "Email", move || email_people(&u, &chosen(&u))));
    }
    if local && !ids.is_empty() {
        e.push(menu::sub("list-add-symbolic", "Add to list", add_to_list_entries(ui, ids.clone())));
    }
    if !ids.is_empty() {
        e.push(menu::sub("upload-symbolic", "Export", export_entries(ui, chosen(ui))));
    }
    if in_list && !ids.is_empty() {
        let u = ui.clone();
        e.push(menu::item("list-remove-symbolic", "Remove from this list", move || remove_members(&u)));
    }
    if writable && !ids.is_empty() {
        let u = ui.clone();
        e.push(menu::separator());
        e.push(menu::danger("user-trash-symbolic", "Delete", move || delete(&u)).keyed("Del"));
    }
    e
}

// ---------- Pages of their own ----------

fn build_full(ui: &Ui) {
    clear(&ui.page_body);
    clear(&ui.page_actions);
    let section = ui.section.borrow().clone();
    ui.page_bar.set_visible(matches!(section.as_str(), "birthdays" | "duplicates"));
    match section.as_str() {
        "birthdays" => birthdays_page(ui),
        "duplicates" => duplicates_page(ui),
        _ => empty_page(ui),
    }
}

fn page_clamp(ui: &Ui, max: i32) -> gtk::Box {
    let body = w::vbox(0);
    body.add_css_class("page-clamp");
    ui.page_body.append(&nyx_ui::clamp::Clamp::new(&body, max));
    body
}

fn empty_block(icon: &str, title: &str, text: &str) -> (gtk::Box, gtk::Box) {
    let empty = w::vbox(8);
    empty.add_css_class("empty-page");
    empty.set_valign(gtk::Align::Center);
    empty.set_halign(gtk::Align::Center);
    empty.set_vexpand(true);
    let i = gtk::Image::from_icon_name(icon);
    i.set_pixel_size(40);
    i.add_css_class("dim");
    empty.append(&i);
    empty.append(&w::label(title, "empty-title"));
    let m = label(text, "dim");
    m.set_selectable(false);
    m.set_max_width_chars(46);
    m.set_justify(gtk::Justification::Center);
    m.set_xalign(0.5);
    empty.append(&m);
    let buttons = w::hbox(8);
    buttons.set_halign(gtk::Align::Center);
    buttons.set_margin_top(8);
    empty.append(&buttons);
    (empty, buttons)
}

fn empty_page(ui: &Ui) {
    let section = ui.section.borrow().clone();
    let list = ui.book.borrow().lists.iter().find(|l| l.id == section).map(|l| l.name.clone());
    let book = online_book(ui);
    let (icon, title, text, new, import) = match (section.as_str(), &list, &book) {
        ("online", ..) => (
            "weather-overcast-symbolic",
            "No accounts yet".to_string(),
            if ui.eds_ready.get() {
                "Connect an account in Settings → Online Accounts to see its contacts here."
            } else {
                "Install evolution-data-server to see the contacts in your online accounts."
            }
            .to_string(),
            false,
            false,
        ),
        (_, _, Some(b)) => (
            "weather-overcast-symbolic",
            if ui.loading.borrow().contains(&b.uid) { "Looking for contacts…".to_string() } else { format!("No one in {} yet", b.label()) },
            "Contacts added to this account show here. This computer's own contacts are under On this computer.".to_string(),
            b.writable && !ui.loading.borrow().contains(&b.uid),
            false,
        ),
        ("favorites", ..) => (
            "non-starred-symbolic",
            "No favorites yet".to_string(),
            "Star someone to keep them here, one click from Email, Call or Message.".to_string(),
            false,
            false,
        ),
        (_, Some(name), _) => (
            "system-users-symbolic",
            format!("No one in {name} yet"),
            "Select people in All contacts, then choose Add to list. Or add someone new here.".to_string(),
            true,
            false,
        ),
        _ => (
            "contact-new-symbolic",
            "No contacts on this computer yet".to_string(),
            "Add someone, or bring your contacts in from a vCard or CSV file. Contacts in your accounts show under Accounts."
                .to_string(),
            true,
            true,
        ),
    };
    let (empty, buttons) = empty_block(icon, &title, &text);
    if new {
        let b = icon_text_button("list-add-symbolic", "New contact", "suggested-action");
        let u = ui.clone();
        b.connect_clicked(move |_| new_contact(&u));
        buttons.append(&b);
    }
    if import {
        let b = icon_text_button("download-symbolic", "Import…", "");
        let u = ui.clone();
        b.connect_clicked(move |_| transfer::choose_import(&u));
        buttons.append(&b);
    }
    ui.page_body.append(&empty);
}

/// Opens someone's details in All contacts.
fn show_contact(ui: &Ui, id: &str) {
    go(ui, "all");
    *ui.selected.borrow_mut() = vec![id.to_string()];
    ui.showing_detail.set(true);
    refresh(ui);
}

fn birthdays_page(ui: &Ui) {
    let everyone = Book { contacts: everyone(ui).into_iter().map(|p| p.contact).collect(), lists: vec![] };
    let upcoming = crate::control::upcoming(&everyone, 366);
    let soon = upcoming.iter().filter(|(n, _)| *n <= 30).count();
    ui.page_title.set_text("Birthdays");
    ui.page_sub.set_text(&match soon {
        0 => "None in the next 30 days".to_string(),
        n => format!("{n} in the next 30 days"),
    });
    let add = icon_text_button("x-office-calendar-symbolic", "Add to Calendar", "bar-action");
    add.set_tooltip_text(Some("Open everyone's birthday in your calendar app, repeating each year"));
    add.set_sensitive(!upcoming.is_empty());
    let u = ui.clone();
    add.connect_clicked(move |_| add_to_calendar(&u));
    ui.page_actions.append(&add);
    if upcoming.is_empty() {
        let (empty, _) =
            empty_block("nyx-contacts-cake-symbolic", "No birthdays yet", "Add a birthday to someone's details and it shows here.");
        ui.page_body.append(&empty);
        return;
    }
    let body = page_clamp(ui, 500);
    let now = glib::DateTime::now_local().ok();
    let mut month = 0;
    let mut k = card();
    let last_first = prefs::get().last_first;
    for (n, c) in upcoming {
        let date = now.as_ref().and_then(|t| t.add_days(n as i32).ok());
        let m = date.as_ref().map(|d| d.month()).unwrap_or(0);
        if m != month {
            month = m;
            body.append(&plain(MONTHS.get((m as usize).wrapping_sub(1)).copied().unwrap_or(""), "month-label"));
            k = card();
            body.append(&k);
        }
        let row = w::hbox(12);
        row.add_css_class("birthday-row");
        if n == 0 {
            row.add_css_class("today");
        }
        row.append(&avatar(&c, 34));
        let text = w::vbox(1);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&plain(&c.name(last_first), "birthday-name"));
        let age = crate::control::next_birthday(&c.birthday).and_then(|(_, a)| a);
        text.append(&plain(&age.map(|a| format!("Turns {a}")).unwrap_or_else(|| "No year".into()), "birthday-age"));
        row.append(&text);
        let when = match n {
            0 => "Today".to_string(),
            1 => "Tomorrow".to_string(),
            n => format!("{} · in {n} days", date.and_then(|d| d.format("%a %-d").ok()).unwrap_or_default()),
        };
        row.append(&plain(&when, "birthday-when"));
        if n == 0 {
            let reach = c
                .phones
                .first()
                .filter(|_| can_open("sms"))
                .map(|p| tel(&p.value, "sms"))
                .or_else(|| c.emails.first().filter(|_| can_open("mailto")).map(|e| mailto(&e.value)));
            if let Some(uri) = reach {
                let b = icon_text_button("chat-symbolic", "Message", "small-button");
                let u = ui.clone();
                b.connect_clicked(move |_| launch(&u, &uri));
                row.append(&b);
            }
        }
        let click = gtk::GestureClick::new();
        let (u, id) = (ui.clone(), c.id.clone());
        click.connect_released(move |g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            show_contact(&u, &id);
        });
        row.add_controller(click);
        row.set_cursor_from_name(Some("pointer"));
        k.append(&row);
    }
}

/// Everyone's birthday as a calendar file, repeating yearly, opened in the calendar app.
fn add_to_calendar(ui: &Ui) {
    let book = ui.book.borrow().clone();
    let stamp = glib::DateTime::now_utc().ok().and_then(|t| t.format("%Y%m%dT%H%M%SZ").ok()).unwrap_or_default();
    let year = glib::DateTime::now_local().ok().map(|t| t.year()).unwrap_or(2000);
    let esc = |s: &str| s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n");
    let mut ics = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nyx//Contacts//EN\r\nX-WR-CALNAME:Birthdays\r\n");
    let mut count = 0;
    for c in &book.contacts {
        let Some((m, d)) = model::birthday_md(&c.birthday) else { continue };
        let y = c.birthday.get(..4).filter(|_| !c.birthday.starts_with("--")).and_then(|y| y.parse::<i32>().ok()).unwrap_or(year);
        ics.push_str(&format!(
            "BEGIN:VEVENT\r\nUID:{}-birthday@nyx-contacts\r\nDTSTAMP:{stamp}\r\nDTSTART;VALUE=DATE:{y:04}{m:02}{d:02}\r\nRRULE:FREQ=YEARLY\r\nSUMMARY:{}\r\nTRANSP:TRANSPARENT\r\nEND:VEVENT\r\n",
            c.id,
            esc(&format!("{}’s birthday", c.name(false)))
        ));
        count += 1;
    }
    ics.push_str("END:VCALENDAR\r\n");
    let path = nyx_core::paths::cache_dir().join("birthdays.ics");
    if let Err(e) = std::fs::create_dir_all(nyx_core::paths::cache_dir()).and_then(|_| std::fs::write(&path, ics)) {
        toast(ui, &format!("Could not write the birthdays: {e}"));
        return;
    }
    launch(ui, &gtk::gio::File::for_path(&path).uri());
    toast(ui, &format!("Opening {count} birthdays in your calendar app."));
}

/// What Duplicates offers to pick between.
#[derive(Clone, Copy)]
enum Pick {
    Name,
    Photo,
    Single(usize),
}

fn duplicates_page(ui: &Ui) {
    let pairs = dup_pairs(ui);
    ui.page_title.set_text("Duplicates");
    let Some((a_id, b_id, why)) = pairs.first().cloned() else {
        ui.page_sub.set_text("None left");
        let (empty, _) = empty_block(
            "nyx-contacts-merge-symbolic",
            "No duplicates",
            "Nobody on this computer shares an email or phone number with someone else.",
        );
        ui.page_body.append(&empty);
        return;
    };
    let (a, b) = {
        let book = ui.book.borrow();
        let find = |id: &str| book.contacts.iter().find(|c| c.id == id).cloned();
        match (find(&a_id), find(&b_id)) {
            (Some(a), Some(b)) => (a, b),
            _ => return,
        }
    };
    let n = pairs.len();
    ui.page_sub.set_text(&format!("{n} {} · the same {why}", if n == 1 { "pair" } else { "pairs" }));
    if n > 1 {
        let all = button(&format!("Merge all {n}"), "Merge every pair, keeping each one's details");
        all.add_css_class("flat");
        let u = ui.clone();
        all.connect_clicked(move |_| merge_all(&u));
        ui.page_actions.append(&all);
    }
    let apart = button("Not the same person", "Keep these two apart, and stop suggesting them");
    apart.add_css_class("flat");
    ui.page_actions.append(&apart);
    let merge = icon_text_button("nyx-contacts-merge-symbolic", "Merge", "suggested-action");
    ui.page_actions.append(&merge);

    let body = page_clamp(ui, 720);
    let grid = gtk::Grid::new();
    grid.add_css_class("dup-grid");
    grid.set_overflow(gtk::Overflow::Hidden);
    let mut row = 0;
    let cell = |grid: &gtk::Grid, col: i32, row: i32, child: &gtk::Widget, class: &str| {
        let c = w::hbox(8);
        c.add_css_class("dup-cell");
        if !class.is_empty() {
            c.add_css_class(class);
        }
        if col > 0 {
            c.set_hexpand(true);
        }
        c.append(child);
        grid.attach(&c, col, row, 1, 1);
        c
    };
    let details = |c: &Contact| c.emails.len() + c.phones.len() + c.addresses.len() + c.websites.len();
    let head = |c: &Contact| {
        let v = w::vbox(6);
        v.append(&avatar(c, 40));
        v.append(&plain(&c.name(false), "dup-name"));
        v.append(&plain(&format!("On this computer · {} details", details(c)), "dup-where"));
        v.upcast::<gtk::Widget>()
    };
    let key = |text: &str| plain(text, "dup-key").upcast::<gtk::Widget>();
    cell(&grid, 0, row, &w::hbox(0).upcast(), "dup-head");
    cell(&grid, 1, row, &head(&a), "dup-head");
    cell(&grid, 2, row, &head(&b), "dup-head");
    row += 1;
    let picks: Rc<RefCell<Vec<(Pick, gtk::CheckButton)>>> = Rc::new(RefCell::new(vec![]));
    let none = |text: &str| if text.is_empty() { "None".to_string() } else { text.to_string() };
    // A value each side, to choose between.
    let mut choose = |title: &str, pick: Pick, left: &str, right: &str| {
        cell(&grid, 0, row, &key(title), "");
        let l = gtk::CheckButton::with_label(&none(left));
        let r = gtk::CheckButton::with_label(&none(right));
        r.set_group(Some(&l));
        if left.is_empty() { r.set_active(true) } else { l.set_active(true) }
        for (side, empty) in [(&l, left.is_empty()), (&r, right.is_empty())] {
            side.add_css_class("pick");
            if empty {
                side.add_css_class("none");
            }
        }
        cell(&grid, 1, row, &l.clone().upcast(), "");
        cell(&grid, 2, row, &r.clone().upcast(), "");
        picks.borrow_mut().push((pick, r));
        row += 1;
    };
    let full_name = |c: &Contact| {
        [c.prefix.as_str(), &c.first, &c.middle, &c.last, &c.suffix].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
    };
    let (na, nb) = (full_name(&a), full_name(&b));
    if na != nb || a.display != b.display {
        choose("Name", Pick::Name, &a.name(false), &b.name(false));
    }
    if a.photo != b.photo && (!a.photo.is_empty() || !b.photo.is_empty()) {
        choose("Photo", Pick::Photo, if a.photo.is_empty() { "" } else { "Their photo" }, if b.photo.is_empty() { "" } else { "Their photo" });
    }
    let sa = editor::singles(&a);
    let sb = editor::singles(&b);
    for (i, title) in [(6, "Company"), (7, "Job title"), (8, "Birthday")] {
        if sa[i] != sb[i] {
            let show = |v: &str| if i == 8 && !v.is_empty() { birthday_text(v) } else { v.to_string() };
            choose(title, Pick::Single(i), &show(&sa[i]), &show(&sb[i]));
        }
    }
    // Values both keep.
    let mut keep = |title: &str, left: Vec<String>, right: Vec<String>, note: &str| {
        if left.is_empty() && right.is_empty() {
            return;
        }
        cell(&grid, 0, row, &key(title), "");
        let side = |v: &[String]| {
            let l = label(&if v.is_empty() { "None".to_string() } else { v.join("\n") }, if v.is_empty() { "dup-none" } else { "" });
            l.set_hexpand(true);
            l.upcast::<gtk::Widget>()
        };
        cell(&grid, 1, row, &side(&left), "");
        let r = cell(&grid, 2, row, &side(&right), "");
        if !note.is_empty() {
            r.append(&plain(note, "dup-kept"));
        }
        row += 1;
    };
    let values = |f: &[model::Field]| f.iter().map(|f| f.value.clone()).collect::<Vec<_>>();
    let same_phone = why == "phone number";
    let phone_note = if same_phone { "same number · kept once" } else if !a.phones.is_empty() && !b.phones.is_empty() { "both kept" } else { "kept" };
    keep("Phone", values(&a.phones), values(&b.phones), phone_note);
    let email_note = if why == "email" { "same email · kept once" } else if !a.emails.is_empty() && !b.emails.is_empty() { "both kept" } else { "kept" };
    keep("Email", values(&a.emails), values(&b.emails), email_note);
    let addr = |c: &Contact| c.addresses.iter().map(|f| model::Address::parse(&f.value).display().replace('\n', ", ")).collect::<Vec<_>>();
    keep("Address", addr(&a), addr(&b), "kept");
    keep("Website", values(&a.websites), values(&b.websites), "kept");
    let note = |c: &Contact| if c.notes.trim().is_empty() { vec![] } else { vec![c.notes.lines().next().unwrap_or("").to_string()] };
    keep("Notes", note(&a), note(&b), "kept");
    let lists = |c: &Contact| {
        ui.book.borrow().lists.iter().filter(|l| l.members.contains(&c.id)).map(|l| l.name.clone()).collect::<Vec<_>>()
    };
    keep("Lists", lists(&a), lists(&b), "kept");
    body.append(&grid);
    let foot = label("Values that differ are yours to choose; every email, phone and list is kept. Merge can be undone.", "dup-foot");
    foot.set_selectable(false);
    body.append(&foot);

    let (u, ka) = (ui.clone(), pair_key(&a.id, &b.id));
    apart.connect_clicked(move |_| {
        prefs::update(|p| {
            if !p.not_duplicates.contains(&ka) {
                p.not_duplicates.push(ka.clone());
            }
        });
        u.dup_pairs.borrow_mut().take();
        refresh(&u);
    });
    let u = ui.clone();
    merge.connect_clicked(move |_| {
        let mut p = model::combine(a.clone(), &b);
        let mut single = editor::singles(&p);
        for (pick, right) in picks.borrow().iter() {
            let from = if right.is_active() { &b } else { &a };
            match *pick {
                Pick::Name => {
                    single[..6].clone_from_slice(&editor::singles(from)[..6]);
                }
                Pick::Photo => p.photo = from.photo.clone(),
                Pick::Single(i) => single[i] = editor::singles(from)[i].clone(),
            }
        }
        editor::set_singles(&mut p, &single);
        dedupe_phones(&mut p);
        let before = u.book.borrow().clone();
        let mut book = before.clone();
        model::merge(&mut book, p.clone(), &b.id);
        if commit(&u, book) {
            let u2 = u.clone();
            toast_action(
                &u,
                &format!("Merged into {}.", p.name(false)),
                Some((
                    "Undo",
                    Box::new(move || {
                        commit(&u2, before.clone());
                    }),
                )),
            );
        }
    });
}

// ---------- Acting on people ----------

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
            && u.toast_serial.get() == serial
        {
            u.toast_box.set_visible(false);
        }
    });
}
pub fn delete(ui: &Ui) {
    if is_remote(ui) {
        delete_online(ui);
        return;
    }
    if people_page(ui) {
        delete_people(ui);
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
            &if ids.len() == 1 { "Deleted 1 contact.".to_string() } else { format!("Deleted {} contacts.", ids.len()) },
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
fn new_list(ui: &Ui, members: Vec<String>) {
    let u = ui.clone();
    w::ask_text("New list", "Organize contacts without making copies.", "", "Create", move |name| {
        let mut book = u.book.borrow().clone();
        let id = model::id();
        book.lists.push(model::List { id: id.clone(), name, members: members.iter().cloned().collect() });
        book.lists.sort_by_key(|l| l.name.to_lowercase());
        if commit(&u, book) && members.is_empty() {
            navigate(&u, &id);
        }
    });
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
fn rename_list(ui: &Ui, id: &str) {
    let list = ui.book.borrow().lists.iter().find(|l| l.id == id).cloned();
    if let Some(l) = list {
        let u = ui.clone();
        w::ask_text("Rename list", "", &l.name, "Rename", move |name| {
            let mut book = u.book.borrow().clone();
            if let Some(x) = book.lists.iter_mut().find(|x| x.id == l.id) {
                x.name = name;
            }
            commit(&u, book);
        });
    }
}
fn delete_list(ui: &Ui, id: &str) {
    let mut book = ui.book.borrow().clone();
    let Some(l) = book.lists.iter().find(|l| l.id == id).cloned() else { return };
    book.lists.retain(|x| x.id != id);
    let was_open = *ui.section.borrow() == id;
    if commit(ui, book) {
        if was_open {
            go(ui, "all");
        }
        let u = ui.clone();
        toast_action(
            ui,
            "List deleted. Its people are still in your contacts.",
            Some((
                "Undo",
                Box::new(move || {
                    let mut book = u.book.borrow().clone();
                    let mut l = l.clone();
                    l.members.retain(|id| book.contacts.iter().any(|c| c.id == *id));
                    book.lists.push(l);
                    book.lists.sort_by_key(|l| l.name.to_lowercase());
                    commit(&u, book);
                }),
            )),
        )
    }
}
fn email_people(ui: &Ui, people: &[Contact]) {
    let mut emails = std::collections::BTreeSet::new();
    for c in people {
        if let Some(e) = c.emails.first() {
            emails.insert(glib::uri_escape_string(&e.value, Some("@+"), false).to_string());
        }
    }
    if emails.is_empty() {
        toast(ui, "None of them has an email address.");
    } else {
        launch(ui, &format!("mailto:{}", emails.into_iter().collect::<Vec<_>>().join(",")));
    }
}
fn email_list(ui: &Ui, id: &str) {
    let people = list_members(ui, id);
    email_people(ui, &people);
}
pub fn launch(ui: &Ui, uri: &str) {
    if let Err(e) = gtk::gio::AppInfo::launch_default_for_uri(uri, None::<&gtk::gio::AppLaunchContext>) {
        toast(ui, &format!("Could not open: {e}"));
    }
}
pub const SHORTCUTS: &[(&[&str], &str)] = &[
    (&["Ctrl", "N"], "New contact"),
    (&["Enter"], "Edit the selected contact"),
    (&["Ctrl", "S"], "Save, while editing"),
    (&["Esc"], "Leave the editor, clear the search or the selection"),
    (&["Del"], "Delete the selected contacts"),
    (&["Ctrl", "A"], "Select everyone in the list"),
    (&["Ctrl", "F"], "Search contacts"),
    (&["Ctrl", "B"], "Collapse sidebar"),
    (&["Ctrl", ","], "Settings"),
    (&["Ctrl", "Shift", "P"], "Command palette"),
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

/// A source in Settings › Sources: its key (`local` or a book's id), name, a line about
/// it, and whether it's a leftover import that can be removed.
pub struct SourceRow {
    pub key: String,
    pub title: String,
    pub desc: String,
    pub removable: bool,
}

pub fn source_rows(ui: &Ui) -> Vec<SourceRow> {
    let count = |n: usize| if n == 1 { "1 contact".to_string() } else { format!("{n} contacts") };
    let mut rows = vec![SourceRow {
        key: "local".into(),
        title: "On this computer".into(),
        desc: count(ui.book.borrow().contacts.len()),
        removable: false,
    }];
    for b in ui.eds_books.borrow().iter() {
        let n = ui.cards.borrow().get(&b.uid).map(|c| c.contacts.len());
        let imported = b.uid.starts_with("import:") || b.uid.starts_with("detached:");
        let mut desc = n.map(count).unwrap_or_else(|| "Not read yet".into());
        if let Some(own) = ui.book_names.borrow().get(&b.uid).filter(|o| !o.is_empty()) {
            desc = format!("{own} · {desc}");
        }
        if imported {
            desc.push_str(" · imported earlier, no account");
        }
        if !b.writable {
            desc.push_str(" · read only");
        }
        rows.push(SourceRow { key: b.uid.clone(), title: b.label().to_string(), desc, removable: imported });
    }
    rows
}

/// Shows or hides a source in the sidebar and All.
pub fn set_source_shown(ui: &Ui, key: &str, shown: bool) {
    prefs::update(|p| {
        p.hidden_sources.retain(|h| h != key);
        if !shown {
            p.hidden_sources.push(key.to_string());
        }
    });
    ui.people.borrow_mut().take();
    if shown && key != "local" && !ui.cards.borrow().contains_key(key) {
        load_book(ui, key.to_string());
    }
    let section = ui.section.borrow().clone();
    if (key == "local" && section == "local") || eds_uid(&section) == Some(key) {
        go(ui, "all");
    } else {
        refresh(ui);
    }
}

/// Asks for a new name for the account book `uid` ("ken@example.com" to "Gmail"); `done`
/// gets the name it shows now. The extra button goes back to the book's own name.
pub fn rename_source(ui: &Ui, uid: &str, done: impl Fn(String) + 'static) {
    let Some(book) = ui.eds_books.borrow().iter().find(|b| b.uid == uid).cloned() else { return };
    let own = ui.book_names.borrow().get(uid).cloned();
    let done = Rc::new(done);
    let set = {
        let (u, uid, done) = (ui.clone(), uid.to_string(), done.clone());
        move |name: Option<String>| {
            let shown = name.clone().or_else(|| u.book_names.borrow().get(&uid).cloned()).unwrap_or_default();
            prefs::update(|p| match &name {
                Some(n) => {
                    p.source_names.insert(uid.clone(), n.clone());
                }
                None => {
                    p.source_names.remove(&uid);
                }
            });
            if let Some(b) = u.eds_books.borrow_mut().iter_mut().find(|b| b.uid == uid) {
                if name.is_some() {
                    u.book_names.borrow_mut().entry(uid.clone()).or_insert_with(|| b.name.clone());
                } else {
                    u.book_names.borrow_mut().remove(&uid);
                }
                b.name = shown;
            }
            u.people.borrow_mut().take();
            refresh(&u);
            let label = u.eds_books.borrow().iter().find(|b| b.uid == uid).map(|b| b.label().to_string()).unwrap_or_default();
            done(label);
        }
    };
    let set = Rc::new(set);
    let s = set.clone();
    let extra = w::ask_text("Rename source", "The name Contacts shows for it. The account keeps its own.", book.label(), "Rename", move |name| {
        s(Some(name));
    });
    if let Some(own) = own.filter(|o| !o.is_empty()) {
        let reset = gtk::Button::with_label(&format!("Use “{own}”"));
        reset.add_css_class("flat");
        reset.set_halign(gtk::Align::Start);
        reset.connect_clicked(move |b| {
            set(None);
            if let Some(w) = b.root().and_downcast::<gtk::Window>() {
                w.close();
            }
        });
        extra.append(&reset);
    }
}

/// Removes a leftover imported book (one with no account) and its cards, after asking.
pub fn remove_source(ui: &Ui, uid: &str, done: impl Fn() + 'static) {
    let Some(book) = ui.eds_books.borrow().iter().find(|b| b.uid == uid).cloned() else { return };
    let n = ui.cards.borrow().get(uid).map(|c| c.contacts.len()).unwrap_or(0);
    let prompt = gtk::AlertDialog::builder()
        .message(format!("Remove {}?", book.label()))
        .detail(format!(
            "Its {n} contacts go from Contacts. It was imported once from the old address book and has no account behind it, so nothing elsewhere changes."
        ))
        .buttons(["Cancel", "Remove"])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    let (u, uid) = (ui.clone(), uid.to_string());
    prompt.choose(Some(&ui.window), None::<&gtk::gio::Cancellable>, move |choice| {
        if choice != Ok(1) {
            return;
        }
        let id = uid.clone();
        let handle = gtk::gio::spawn_blocking(move || remove_book(&id));
        let (u, uid) = (u.clone(), uid.clone());
        let done = Rc::new(done);
        glib::spawn_future_local(async move {
            match handle.await {
                Ok(Ok(())) => {
                    u.cards.borrow_mut().remove(&uid);
                    prefs::update(|p| p.hidden_sources.retain(|h| *h != uid));
                    if eds_uid(&u.section.borrow()) == Some(uid.as_str()) {
                        go(&u, "all");
                    }
                    load_eds(&u);
                    toast(&u, "Removed.");
                    done();
                }
                Ok(Err(e)) => toast(&u, &format!("Could not remove it: {e:#}")),
                Err(_) => toast(&u, "Removing stopped."),
            }
        });
    });
}

/// Deletes a book with no account, and everything kept for its cards, from the shared
/// address-book store (`nyx-pim-preview`). Calendars and tasks aren't touched.
fn remove_book(uid: &str) -> anyhow::Result<()> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| anyhow::anyhow!("No data folder"))?;
    let mut db = rusqlite::Connection::open(base.join("nyx-pim-preview/pim.sqlite3"))?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = db.transaction()?;
    let account: Option<Option<String>> = tx
        .query_row("SELECT account FROM books WHERE uid=?1", [uid], |r| r.get(0))
        .map(Some)
        .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })?;
    match account {
        None => anyhow::bail!("That address book is gone already"),
        Some(Some(_)) => anyhow::bail!("That address book belongs to an account; remove the account in Settings instead"),
        Some(None) => {}
    }
    for table in ["contacts", "contact_resources", "contact_baselines", "contact_remote_versions", "contact_history"] {
        let exists: bool =
            tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [table], |r| r.get::<_, i64>(0))? > 0;
        if exists {
            tx.execute(&format!("DELETE FROM {table} WHERE book=?1"), [uid])?;
        }
    }
    tx.execute("DELETE FROM books WHERE uid=?1 AND account IS NULL", [uid])?;
    tx.commit()?;
    Ok(())
}

/// The book being edited or deleted from: a person's copy in All, else the page's.
pub fn online_book(ui: &Ui) -> Option<nyx_pim::contacts::AddressBook> {
    let uid = match ui.target.borrow().as_ref() {
        Some(Origin::Local) => return None,
        Some(Origin::Book(uid)) => uid.clone(),
        None => eds_uid(&ui.section.borrow())?.to_string(),
    };
    ui.eds_books.borrow().iter().find(|book| book.uid == uid).cloned()
}
pub fn online_snapshot(ui: &Ui, id: &str) -> Option<String> {
    let book = online_book(ui)?;
    ui.cards.borrow().get(&book.uid).and_then(|b| b.raw.get(id).cloned())
}
/// A card saved to `book`: in its cards now, and the page shows it.
pub fn online_saved(ui: &Ui, book: &str, contact: Contact, payload: String) {
    {
        let mut cards = ui.cards.borrow_mut();
        let b = cards.entry(book.to_string()).or_default();
        b.raw.insert(contact.id.clone(), payload);
        match b.contacts.iter_mut().find(|c| c.id == contact.id) {
            Some(previous) => *previous = contact.clone(),
            None => b.contacts.push(contact.clone()),
        }
    }
    ui.people.borrow_mut().take();
    if eds_uid(&ui.section.borrow()) == Some(book) {
        *ui.selected.borrow_mut() = vec![contact.id.clone()];
        ui.showing_detail.set(true);
    }
    refresh(ui);
    refresh_online(ui);
}

/// Deletes the selected people in All: every copy you can change (read-only books keep
/// theirs), after asking.
fn delete_people(ui: &Ui) {
    let ids = ui.selected.borrow().clone();
    let chosen: Vec<Person> = ids.iter().filter_map(|id| person(ui, id)).collect();
    if chosen.is_empty() {
        toast(ui, "Select contacts to delete.");
        return;
    }
    let mut places: Vec<String> = Vec::new();
    let mut kept = false;
    for p in &chosen {
        for (o, _) in &p.members {
            let (name, writable) = origin_name(ui, o);
            if !writable {
                kept = true;
            } else if !places.contains(&name) {
                places.push(name);
            }
        }
    }
    let who = if chosen.len() == 1 { chosen[0].contact.name(false) } else { format!("{} people", chosen.len()) };
    let detail = format!(
        "Every copy in {} goes.{}",
        places.join(", "),
        if kept { " Read-only books keep theirs." } else { "" }
    );
    let prompt = gtk::AlertDialog::builder()
        .message(format!("Delete {who}?"))
        .detail(detail)
        .buttons(["Cancel", "Delete"])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    let u = ui.clone();
    prompt.choose(Some(&ui.window), None::<&gtk::gio::Cancellable>, move |choice| {
        if choice != Ok(1) {
            return;
        }
        let local: Vec<String> = chosen.iter().filter_map(|p| p.local().map(|c| c.id.clone())).collect();
        if !local.is_empty() {
            let mut book = u.book.borrow().clone();
            book.contacts.retain(|c| !local.contains(&c.id));
            for l in &mut book.lists {
                l.members.retain(|id| !local.contains(id));
            }
            commit(&u, book);
        }
        let mut by_book: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for p in &chosen {
            for (o, c) in &p.members {
                if let Origin::Book(uid) = o
                    && origin_name(&u, o).1
                    && let Some(raw) = u.cards.borrow().get(uid).and_then(|b| b.raw.get(&c.id).cloned())
                {
                    by_book.entry(uid.clone()).or_default().push((c.id.clone(), raw));
                }
            }
        }
        u.selected.borrow_mut().clear();
        for (uid, expected) in by_book {
            let id = uid.clone();
            let handle = gtk::gio::spawn_blocking(move || nyx_pim::contacts::delete_vcards(&id, &expected));
            let u = u.clone();
            glib::spawn_future_local(async move {
                match handle.await {
                    Ok(Ok(())) => load_book(&u, uid),
                    Ok(Err(error)) => toast(&u, &error.to_string()),
                    Err(_) => toast(&u, "Deleting stopped; refresh the address book."),
                }
            });
        }
        refresh(&u);
        refresh_online(&u);
    });
}
fn delete_online(ui: &Ui) {
    let Some(book) = online_book(ui) else { return };
    if !book.writable {
        toast(ui, "This address book is read only.");
        return;
    }
    let ids = ui.selected.borrow().clone();
    if ids.is_empty() {
        toast(ui, "Select contacts to delete.");
        return;
    }
    let expected: Vec<_> = ids.iter().filter_map(|id| online_snapshot(ui, id).map(|p| (id.clone(), p))).collect();
    if expected.len() != ids.len() {
        toast(ui, "Refresh the book before deleting.");
        return;
    }
    let prompt=gtk::AlertDialog::builder().message(format!("Delete {} contacts from this address book?",ids.len())).detail("The deletion is queued locally and will be sent to the provider when connected. Previous cards remain in local history.").buttons(["Cancel","Delete"]).cancel_button(0).default_button(0).modal(true).build();
    let u = ui.clone();
    prompt.choose(Some(&ui.window), None::<&gtk::gio::Cancellable>, move |choice| {
        if choice != Ok(1) {
            return;
        }
        let id = book.uid.clone();
        let handle = gtk::gio::spawn_blocking(move || nyx_pim::contacts::delete_vcards(&id, &expected));
        let u = u.clone();
        glib::spawn_future_local(async move {
            match handle.await {
                Ok(Ok(())) => {
                    u.selected.borrow_mut().clear();
                    load_book(&u, book.uid.clone());
                    refresh_online(&u);
                }
                Ok(Err(error)) => toast(&u, &error.to_string()),
                Err(_) => toast(&u, "Contact deletion stopped; refresh the address book."),
            }
        });
    });
}
