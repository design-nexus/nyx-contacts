//! Menus with an icon before each item, pages for submenus, and notes under an item.

use gtk::prelude::*;

pub enum Entry {
    Item {
        label: String,
        icon: Option<&'static str>,
        /// A colour dot instead of an icon (the sidebar's swatch number).
        swatch: Option<usize>,
        key: Option<&'static str>,
        danger: bool,
        enabled: bool,
        run: Box<dyn Fn()>,
    },
    /// Opens a page of its own (Add to list, Colour), with a way back.
    Sub { label: String, icon: Option<&'static str>, entries: Vec<Entry> },
    /// A dim line under the item before it.
    Note(String),
    Separator,
}

pub fn item(icon: &'static str, label: &str, run: impl Fn() + 'static) -> Entry {
    Entry::Item { label: label.into(), icon: Some(icon), swatch: None, key: None, danger: false, enabled: true, run: Box::new(run) }
}

pub fn danger(icon: &'static str, label: &str, run: impl Fn() + 'static) -> Entry {
    Entry::Item { label: label.into(), icon: Some(icon), swatch: None, key: None, danger: true, enabled: true, run: Box::new(run) }
}

/// A colour to pick, shown as its dot.
pub fn swatch(index: usize, label: &str, run: impl Fn() + 'static) -> Entry {
    Entry::Item { label: label.into(), icon: None, swatch: Some(index), key: None, danger: false, enabled: true, run: Box::new(run) }
}

pub fn sub(icon: &'static str, label: &str, entries: Vec<Entry>) -> Entry {
    Entry::Sub { label: label.into(), icon: Some(icon), entries }
}

pub fn note(text: &str) -> Entry {
    Entry::Note(text.into())
}

pub fn separator() -> Entry {
    Entry::Separator
}

impl Entry {
    /// The same item, greyed out when `on` is false.
    pub fn enabled(mut self, on: bool) -> Self {
        if let Entry::Item { enabled, .. } = &mut self {
            *enabled = on;
        }
        self
    }

    /// The same item with its keyboard shortcut shown at its end.
    pub fn keyed(mut self, k: &'static str) -> Self {
        if let Entry::Item { key, .. } = &mut self {
            *key = Some(k);
        }
        self
    }
}

fn row(label: &str, end: Option<gtk::Widget>, danger: bool, icon: Option<&str>) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("menu-item");
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    button.set_child(Some(&line));
    if let Some(name) = icon {
        // An empty name keeps the label in line with the others (an unticked choice).
        let image = if name.is_empty() { gtk::Image::new() } else { gtk::Image::from_icon_name(name) };
        image.set_pixel_size(16);
        image.add_css_class("menu-icon");
        if danger {
            image.add_css_class("danger-text");
        }
        line.append(&image);
    }
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    if danger {
        text.add_css_class("danger-text");
    }
    line.append(&text);
    if let Some(end) = end {
        line.append(&end);
    }
    button
}

fn page(popover: &gtk::Popover, pages: &gtk::Stack, entries: Vec<Entry>, back: Option<&str>) -> gtk::Box {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 1);
    if let Some(title) = back {
        let r = row(title, None, false, Some("go-previous-symbolic"));
        let p = pages.clone();
        r.connect_clicked(move |_| p.set_visible_child_name("main"));
        list.append(&r);
        list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }
    for (i, entry) in entries.into_iter().enumerate() {
        match entry {
            Entry::Separator => list.append(&gtk::Separator::new(gtk::Orientation::Horizontal)),
            Entry::Note(text) => {
                let l = gtk::Label::new(Some(&text));
                l.add_css_class("menu-note");
                l.set_xalign(0.0);
                list.append(&l);
            }
            Entry::Item { label, icon, swatch, key, danger, enabled, run } => {
                let hint = key.map(|k| {
                    let l = gtk::Label::new(Some(k));
                    l.add_css_class("dim");
                    l.upcast::<gtk::Widget>()
                });
                let r = row(&label, hint, danger, icon);
                if let Some(i) = swatch
                    && let Some(line) = r.child().and_downcast::<gtk::Box>()
                {
                    line.prepend(&crate::ui::dot(i, 10));
                }
                r.set_sensitive(enabled);
                let p = popover.clone();
                r.connect_clicked(move |_| {
                    p.popdown();
                    run();
                });
                list.append(&r);
            }
            Entry::Sub { label, icon, entries } => {
                let name = format!("sub{i}");
                let sub = page(popover, pages, entries, Some(&label));
                pages.add_named(&sub, Some(&name));
                let r = row(&label, Some(gtk::Image::from_icon_name("go-next-symbolic").upcast()), false, icon);
                let p = pages.clone();
                r.connect_clicked(move |_| p.set_visible_child_name(&name));
                list.append(&r);
            }
        }
    }
    list
}

fn show(anchor: &gtk::Widget, entries: Vec<Entry>, at: Option<(f64, f64)>) {
    if entries.is_empty() {
        return;
    }
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.set_parent(anchor);
    popover.set_position(gtk::PositionType::Bottom);
    if let Some((x, y)) = at {
        popover.set_halign(gtk::Align::Start);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    }
    popover.add_css_class("menu-popover");
    // A snapshot's window never has focus, which closes a menu that hides on its own.
    if std::env::var_os("CONTACTS_SNAPSHOT").is_some() {
        popover.set_autohide(false);
    }
    let pages = gtk::Stack::new();
    pages.set_vhomogeneous(false);
    pages.set_hhomogeneous(false);
    pages.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    let main = page(&popover, &pages, entries, None);
    pages.add_named(&main, Some("main"));
    pages.set_visible_child_name("main");
    popover.set_child(Some(&pages));
    let weak = anchor.downgrade();
    popover.connect_closed(move |p| {
        let (p, a) = (p.clone(), weak.clone());
        gtk::glib::idle_add_local_once(move || {
            if a.upgrade().is_some() {
                p.unparent();
            }
        });
    });
    popover.popup();
}

/// A menu under `anchor` (a ⋯ or More button).
pub fn below(anchor: &impl IsA<gtk::Widget>, entries: Vec<Entry>) {
    show(anchor.upcast_ref(), entries, None);
}

/// A menu at a point in `anchor` (a right-click's position).
pub fn at(anchor: &impl IsA<gtk::Widget>, x: f64, y: f64, entries: Vec<Entry>) {
    show(anchor.upcast_ref(), entries, Some((x, y)));
}

/// Calls `open(widget, x, y)` on a right-click (or a long press) on `widget`.
pub fn on_right_click(widget: &impl IsA<gtk::Widget>, open: impl Fn(&gtk::Widget, f64, f64) + 'static) {
    let open = std::rc::Rc::new(open);
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let o = open.clone();
    click.connect_pressed(move |g, _, x, y| {
        if let Some(w) = g.widget() {
            g.set_state(gtk::EventSequenceState::Claimed);
            o(&w, x, y);
        }
    });
    widget.add_controller(click);
    let press = gtk::GestureLongPress::new();
    press.set_touch_only(true);
    press.connect_pressed(move |g, x, y| {
        if let Some(w) = g.widget() {
            open(&w, x, y);
        }
    });
    widget.add_controller(press);
}
