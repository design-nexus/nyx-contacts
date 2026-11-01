//! Explicit, preview-only round trip through the same native backend as the editor.
use anyhow::{Context, Result, ensure};
use nyx_accounts_native::Service;
use nyx_pim::{Registry, contacts};
use rusqlite::{Connection, OptionalExtension};

pub fn run(cleanup_uid: Option<&str>) -> Result<()> {
    ensure!(
        std::env::var("NYX_ACCOUNTS_PREVIEW").as_deref() == Ok("1"),
        "This diagnostic is available only in the isolated account preview"
    );
    let account = nyx_accounts_native::Client::connect()?
        .list()?
        .into_iter()
        .find(|a| a.provider == "google" && a.enabled.contains(&Service::Contacts))
        .context("No Google Contacts account enabled")?;
    let registry = Registry::connect()?;
    if cleanup_uid.is_none() {
        registry.refresh_accounts(Service::Contacts)?;
    }
    let path = std::path::PathBuf::from(std::env::var_os("XDG_DATA_HOME").context("Preview data directory missing")?)
        .join("nyx-pim-preview/pim.sqlite3");
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let book: String =
        db.query_row("SELECT uid FROM books WHERE account=?1 AND backend='google' AND writable=1", [&account.id], |r| r.get(0))?;
    if let Some(uid) = cleanup_uid {
        uuid::Uuid::parse_str(uid)?;
        let payload: String =
            db.query_row("SELECT payload FROM contacts WHERE book=?1 AND uid=?2", [&book, uid], |r| r.get(0))?;
        ensure!(payload.contains(&format!("Nyx connection test {uid}")), "This is not a diagnostic contact");
        for issue in registry.sync_issues()?.into_iter().filter(|i| i.source == book && i.uid == uid) {
            registry.resolve_sync_issue(&issue, nyx_pim::Resolution::KeepLocal)?;
        }
        let deleted: bool = db.query_row("SELECT deleted FROM contacts WHERE book=?1 AND uid=?2", [&book, uid], |r| r.get(0))?;
        if !deleted {
            contacts::delete_vcards(&book, &[(uid.to_string(), payload)])?;
        }
        registry.refresh_accounts(Service::Contacts)?;
        let pending: i64 = db.query_row(
            "SELECT count(*) FROM contacts WHERE book=?1 AND uid=?2 AND (deleted=0 OR dirty<>0)",
            [&book, uid],
            |r| r.get(0),
        )?;
        ensure!(pending == 0, "Diagnostic contact cleanup is still pending");
        println!("Google Contacts: previous diagnostic contact removed");
        return Ok(());
    }
    let count: i64 = db.query_row("SELECT count(*) FROM contacts WHERE book=?1 AND deleted=0", [&book], |r| r.get(0))?;
    println!("Google Contacts: downloaded {count} contacts");
    let uid = uuid::Uuid::new_v4().to_string();
    let marker = format!("Nyx connection test {uid}");
    let payload = format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid}\r\nFN:{marker}\r\nN:connection test;Nyx;;;\r\nNOTE:Disposable connection test\r\nEND:VCARD\r\n"
    );
    let current = || -> Result<Option<String>> {
        Ok(db
            .query_row("SELECT payload FROM contacts WHERE book=?1 AND uid=?2 AND deleted=0", [&book, &uid], |r| r.get(0))
            .optional()?)
    };
    let check = (|| -> Result<()> {
        contacts::save_vcard(&book, &payload, None)?;
        registry.refresh_accounts(Service::Contacts)?;
        let remote: i64 = db.query_row(
            "SELECT count(*) FROM contact_resources WHERE book=?1 AND uid=?2 AND remote_id IS NOT NULL",
            [&book, &uid],
            |r| r.get(0),
        )?;
        let dirty: i64 = db.query_row("SELECT dirty FROM contacts WHERE book=?1 AND uid=?2", [&book, &uid], |r| r.get(0))?;
        ensure!(remote == 1 && dirty == 0, "Contact create was not acknowledged by Google");
        println!("Google Contacts: create acknowledged by provider");
        let before = current()?.context("Created contact missing")?;
        let edited = before.replace("Disposable connection test", "Updated disposable connection test");
        ensure!(edited != before, "Test note missing from downloaded contact");
        contacts::save_vcard(&book, &edited, Some(&before))?;
        registry.refresh_accounts(Service::Contacts)?;
        let dirty: i64 = db.query_row("SELECT dirty FROM contacts WHERE book=?1 AND uid=?2", [&book, &uid], |r| r.get(0))?;
        ensure!(
            dirty == 0 && current()?.is_some_and(|s| s.contains("Updated disposable connection test")),
            "Contact update was not acknowledged by Google"
        );
        println!("Google Contacts: update acknowledged by provider");
        Ok(())
    })();
    let cleanup = (|| -> Result<()> {
        if let Some(payload) = current()? {
            contacts::delete_vcards(&book, &[(uid.clone(), payload)])?;
            registry.refresh_accounts(Service::Contacts)?;
        }
        ensure!(current()?.is_none(), "Disposable contact cleanup needs attention");
        let dirty: i64 =
            db.query_row("SELECT count(*) FROM contacts WHERE book=?1 AND uid=?2 AND dirty<>0", [&book, &uid], |r| r.get(0))?;
        ensure!(dirty == 0, "Disposable contact deletion is still queued");
        println!("Google Contacts: disposable contact deleted");
        Ok(())
    })();
    cleanup?;
    check
}
