mod dialog_dismissal;
mod control;
mod online_check;
mod editor;
mod formats;
mod model;
mod prefs;
mod theme;
mod transfer;
mod ui;
use gtk::{gio, glib, prelude::*};
const APP_ID: &str = "io.github.design_nexus.nyx.Contacts";
fn main() -> glib::ExitCode {
    nyx_core::app::init(nyx_core::app::App { slug: "nyx-contacts", name: "Contacts" });
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!("Usage: nyx-contacts [--section all|favorites|birthdays|duplicates|settings] [--toggle] [FILE.vcf|FILE.csv …]\n       nyx-contacts --find QUERY   (matching contacts as JSON, no window)\n       nyx-contacts --sync-online  (synchronize enabled accounts, no window)");
        return glib::ExitCode::SUCCESS;
    }
    if let Some(i) = args.iter().position(|s| s == "--check-online") {
        let cleanup = if args.get(i+1).is_some_and(|s|s=="cleanup") {
            let Some(uid)=args.get(i+2) else { eprintln!("A diagnostic contact ID is required"); return glib::ExitCode::FAILURE; };
            Some(uid.as_str())
        } else { None };
        return match online_check::run(cleanup) {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(error) => { eprintln!("Contacts online check: {error:#}"); glib::ExitCode::FAILURE }
        };
    }
    if args.iter().any(|s| s == "--sync-online") {
        return match nyx_pim::Registry::connect().and_then(|r| r.refresh_accounts(nyx_accounts_native::Service::Contacts)) {
            Ok(()) => { println!("Contacts synchronization completed"); glib::ExitCode::SUCCESS }
            Err(error) => { eprintln!("Contacts synchronization: {error}"); glib::ExitCode::FAILURE }
        };
    }
    if let Some(i) = args.iter().position(|s| s == "--find") {
        let query = args.get(i + 1).map(String::as_str).unwrap_or("");
        return match control::find_headless(query) {
            Ok(found) => {
                println!("{found}");
                glib::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Contacts: {e:#}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if std::env::var_os("GSK_RENDERER").is_none() {
        unsafe {
            std::env::set_var("GSK_RENDERER", "ngl");
        }
    }
    let app = gtk::Application::builder()
        .application_id(std::env::var("NYX_CONTACTS_APP_ID").unwrap_or_else(|_| APP_ID.to_string()))
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    // Nyx Assistant looks people up and adds them through this (primary instance only).
    app.connect_startup(|app| nyx_core::control::serve(app, control::tools(), control::handle));
    app.connect_command_line(|app, cl| {
        let argv: Vec<_> = cl.arguments().iter().map(|s| s.to_string_lossy().into_owned()).collect();
        if argv.iter().any(|s| s == "--toggle")
            && let Some(w) = app.active_window() {
                w.close();
                return glib::ExitCode::SUCCESS;
            }
        let section = argv.iter().position(|s| s == "--section").and_then(|i| argv.get(i + 1));
        let ui = match ui::present(app, section.map(String::as_str)) {
            Ok(ui) => ui,
            Err(e) => {
                eprintln!("Contacts: {e:#}");
                let (w, b) = nyx_ui::widgets::dialog("Contacts could not open", 480);
                crate::dialog_dismissal::add(&w, &b, "Close");
                b.append(&gtk::Label::new(Some(&format!("{e:#}\nYour address book has been kept unchanged."))));
                w.set_application(Some(app));
                w.present();
                return glib::ExitCode::FAILURE;
            }
        };
        let mut skip = false;
        for arg in argv.iter().skip(1) {
            if skip {
                skip = false;
                continue;
            }
            if arg == "--section" {
                skip = true;
                continue;
            }
            if arg.starts_with('-') {
                continue;
            }
            let file = cl.create_file_for_arg(arg);
            if let Some(path) = file.path() {
                transfer::import_path(&ui, &path);
            }
        }
        glib::ExitCode::SUCCESS
    });
    app.connect_shutdown(|_| prefs::flush());
    app.run()
}
