//! Contacts' tools on the control interface (`nyx_core::control`), for Nyx Assistant:
//! look people up, list upcoming birthdays and lists, add someone, change or add details,
//! and put someone on a list. Nothing is removed: no delete, no merge, no clearing a
//! field; notes are added to, never replaced.

use crate::model::{self, Address, Contact, Field};
use crate::ui;
use nyx_core::control::text;
use serde_json::{json, Value};

/// Most emails or phones one call may add.
const MAX_VALUES: usize = 20;

/// `nyx-contacts --find QUERY`: the same answer as `contacts.find`, printed
/// as JSON without opening a window (Mail's recipient suggestions, Assistant).
pub fn find_headless(query: &str) -> anyhow::Result<Value> {
    let book = model::Store::read(&nyx_core::paths::data_dir().join("contacts.db"))?.unwrap_or_default();
    let found: Vec<Value> = book.contacts.iter().filter(|c| c.matches(query)).take(10).map(card).collect();
    Ok(json!({"contacts": found, "untrusted_content": true}))
}

pub fn tools() -> Value {
    let list = json!({"type": "array", "items": {"type": "string"}, "maxItems": MAX_VALUES});
    json!([
        {"name": "contacts.find", "description": "Find contacts by name, company, email or phone: their details.", "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}},
        {"name": "contacts.birthdays", "description": "Birthdays in the next days (30 unless given), soonest first.", "inputSchema": {"type": "object", "properties": {"days": {"type": "integer"}}}},
        {"name": "contacts.lists", "description": "The contact lists, with how many people are on each.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "contacts.add", "description": "Add a new contact; it needs a name, company, email or phone. Refused when someone already has the email or phone.", "inputSchema": {"type": "object", "properties": {"first": {"type": "string"}, "last": {"type": "string"}, "company": {"type": "string"}, "title": {"type": "string"}, "emails": list, "phones": list, "birthday": {"type": "string", "description": "YYYY-MM-DD, or --MM-DD without a year"}, "notes": {"type": "string"}}}},
        {"name": "contacts.update", "description": "Change a contact's details (by id from contacts.find), add an email or phone, or add to its notes; nothing is removed.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "first": {"type": "string"}, "last": {"type": "string"}, "company": {"type": "string"}, "title": {"type": "string"}, "birthday": {"type": "string"}, "notes": {"type": "string", "description": "Added to the contact's notes (never replaces them)"}, "add_email": {"type": "string"}, "add_phone": {"type": "string"}, "favorite": {"type": "boolean"}}, "required": ["id"]}},
        {"name": "contacts.add_to_list", "description": "Put a contact (by id) on a list (by name from contacts.lists).", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "list": {"type": "string"}}, "required": ["id", "list"]}},
    ])
}

pub fn card(c: &Contact) -> Value {
    let values = |f: &[Field]| f.iter().map(|x| json!({"label": x.label, "value": x.value})).collect::<Vec<_>>();
    let addresses: Vec<Value> = c
        .addresses
        .iter()
        .map(|a| json!({"label": a.label, "value": Address::parse(&a.value).display()}))
        .collect();
    json!({
        "id": c.id, "name": c.name(false), "company": c.company, "title": c.title, "birthday": c.birthday,
        "emails": values(&c.emails), "phones": values(&c.phones), "addresses": addresses,
        "websites": values(&c.websites), "notes": c.notes, "favorite": c.favorite,
    })
}

fn field(value: &str) -> Field {
    Field { label: "other".into(), value: value.trim().to_string() }
}

fn short(args: &Value, key: &str) -> Result<Option<String>, String> {
    match args[key].as_str() {
        Some(v) if v.len() > 2000 => Err(format!("{key} is too long")),
        Some(v) => Ok(Some(v.trim().to_string())),
        None => Ok(None),
    }
}

fn birthday(args: &Value) -> Result<Option<String>, String> {
    match short(args, "birthday")?.filter(|v| !v.is_empty()) {
        Some(v) => model::normalize_birthday(&v)
            .map(Some)
            .ok_or_else(|| format!("“{v}” isn't a date; use YYYY-MM-DD, or --MM-DD without a year")),
        None => Ok(None),
    }
}

/// Days from `today` (month, day, year) to the next `md` birthday.
fn days_until(md: (u32, u32), today: (u32, u32, i32)) -> Option<i64> {
    let date = |y: i32, (m, d): (u32, u32)| {
        // A 29 February birthday falls on 1 March in other years.
        days_from_civil(y, m, d).or_else(|| days_from_civil(y, 3, 1).filter(|_| (m, d) == (2, 29)))
    };
    let now = days_from_civil(today.2, today.0, today.1)?;
    let this = date(today.2, md)?;
    let next = if this >= now { this } else { date(today.2 + 1, md)? };
    Some(next - now)
}

/// A date as days since 1970-01-01, or None if it doesn't exist.
fn days_from_civil(y: i32, m: u32, d: u32) -> Option<i64> {
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let max = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) || d == 0 || d > max[m as usize - 1] {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Today's (month, day, year) in local time.
fn today() -> (u32, u32, i32) {
    let now = gtk::glib::DateTime::now_local().ok();
    now.map(|n| (n.month() as u32, n.day_of_month() as u32, n.year())).unwrap_or((1, 1, 1970))
}

/// Days to a birthday's next return, and the age it brings (None without a year).
pub fn next_birthday(birthday: &str) -> Option<(i64, Option<i32>)> {
    let md = model::birthday_md(birthday)?;
    let t = today();
    let n = days_until(md, t)?;
    let next_year = if days_from_civil(t.2, md.0, md.1).zip(days_from_civil(t.2, t.0, t.1)).is_some_and(|(b, now)| b >= now) {
        t.2
    } else {
        t.2 + 1
    };
    let born = birthday.get(..4).filter(|_| !birthday.starts_with("--")).and_then(|y| y.parse::<i32>().ok());
    Some((n, born.map(|y| next_year - y)))
}

/// Contacts with a birthday in the next `days` days, soonest first, with how many days off.
pub fn upcoming(book: &model::Book, days: i64) -> Vec<(i64, Contact)> {
    let t = today();
    let mut out: Vec<(i64, Contact)> = book
        .contacts
        .iter()
        .filter_map(|c| {
            let n = days_until(model::birthday_md(&c.birthday)?, t)?;
            (n <= days).then(|| (n, c.clone()))
        })
        .collect();
    out.sort_by_key(|(n, c)| (*n, c.name(false)));
    out
}

pub fn handle(name: &str, args: Value) -> Result<Value, String> {
    let ui = ui::current().ok_or("Contacts isn't ready")?;
    match name {
        "contacts.find" => {
            let q = text(&args, "query")?;
            let book = ui.book.borrow();
            let found: Vec<Value> = book.contacts.iter().filter(|c| c.matches(&q)).take(10).map(card).collect();
            Ok(json!({"contacts": found, "untrusted_content": true}))
        }
        "contacts.birthdays" => {
            let days = args["days"].as_i64().unwrap_or(30).clamp(0, 366);
            let book = ui.book.borrow();
            let found: Vec<Value> = upcoming(&book, days)
                .into_iter()
                .map(|(n, c)| json!({"id": c.id, "name": c.name(false), "birthday": c.birthday, "in_days": n}))
                .collect();
            Ok(json!({"birthdays": found, "untrusted_content": true}))
        }
        "contacts.lists" => {
            let book = ui.book.borrow();
            let lists: Vec<Value> = book.lists.iter().map(|l| json!({"name": l.name, "people": l.members.len()})).collect();
            Ok(json!({"lists": lists, "untrusted_content": true}))
        }
        "contacts.add" => {
            let mut c = Contact::default();
            for (key, slot) in [
                ("first", &mut c.first),
                ("last", &mut c.last),
                ("company", &mut c.company),
                ("title", &mut c.title),
                ("notes", &mut c.notes),
            ] {
                if let Some(v) = short(&args, key)? {
                    *slot = v;
                }
            }
            c.birthday = birthday(&args)?.unwrap_or_default();
            let strings = |key: &str| -> Result<Vec<Field>, String> {
                let a = args[key].as_array().cloned().unwrap_or_default();
                if a.len() > MAX_VALUES {
                    return Err(format!("at most {MAX_VALUES} {key}"));
                }
                Ok(a.iter().filter_map(Value::as_str).filter(|s| !s.trim().is_empty() && s.len() <= 2000).map(field).collect())
            };
            c.emails = strings("emails")?;
            c.phones = strings("phones")?;
            if !c.valid() {
                return Err("a contact needs a name, company, email or phone".into());
            }
            let mut book = ui.book.borrow().clone();
            if let Some(same) = book.contacts.iter().find(|o| model::duplicates(o, &c)) {
                return Err(format!("{} already has that email or phone (id {})", same.name(false), same.id));
            }
            let added = card(&c);
            book.contacts.push(c);
            if !ui::commit(&ui, book) {
                return Err("couldn't save the address book".into());
            }
            Ok(json!({"added": added}))
        }
        "contacts.update" => {
            let id = text(&args, "id")?;
            let mut book = ui.book.borrow().clone();
            let new_email = short(&args, "add_email")?.filter(|v| !v.is_empty());
            let new_phone = short(&args, "add_phone")?.filter(|v| !v.is_empty());
            // The same email or phone on someone else is a duplicate, not a detail.
            let probe = Contact {
                emails: new_email.iter().map(|e| field(e)).collect(),
                phones: new_phone.iter().map(|p| field(p)).collect(),
                ..Contact::default()
            };
            if let Some(other) = book.contacts.iter().find(|o| o.id != id && model::duplicates(o, &probe)) {
                return Err(format!("{} already has that email or phone (id {})", other.name(false), other.id));
            }
            let bday = birthday(&args)?;
            let c = book.contacts.iter_mut().find(|c| c.id == id).ok_or("no contact with that id")?;
            let mut renamed = false;
            for (key, slot) in [("first", &mut c.first), ("last", &mut c.last), ("company", &mut c.company), ("title", &mut c.title)] {
                // Change, never clear.
                if let Some(v) = short(&args, key)?.filter(|v| !v.is_empty()) {
                    renamed |= matches!(key, "first" | "last") && *slot != v;
                    *slot = v;
                }
            }
            // A card's own display name would hide the new name.
            if renamed {
                c.display.clear();
            }
            if let Some(b) = bday {
                c.birthday = b;
            }
            if let Some(note) = short(&args, "notes")?.filter(|v| !v.is_empty()) {
                if !c.notes.is_empty() {
                    c.notes.push_str("\n\n");
                }
                c.notes.push_str(&note);
            }
            if let Some(e) = new_email
                && !model::duplicates(c, &Contact { emails: vec![field(&e)], ..Contact::default() })
            {
                c.emails.push(field(&e));
            }
            if let Some(p) = new_phone
                && !model::duplicates(c, &Contact { phones: vec![field(&p)], ..Contact::default() })
            {
                c.phones.push(field(&p));
            }
            if let Some(f) = args["favorite"].as_bool() {
                c.favorite = f;
            }
            let updated = card(c);
            if !ui::commit(&ui, book) {
                return Err("couldn't save the address book".into());
            }
            Ok(json!({"updated": updated}))
        }
        "contacts.add_to_list" => {
            let id = text(&args, "id")?;
            let list = text(&args, "list")?;
            let mut book = ui.book.borrow().clone();
            if !book.contacts.iter().any(|c| c.id == id) {
                return Err("no contact with that id".into());
            }
            let l = book
                .lists
                .iter_mut()
                .find(|l| l.name.eq_ignore_ascii_case(list.trim()))
                .ok_or_else(|| format!("no list named “{list}”"))?;
            l.members.insert(id);
            let name = l.name.clone();
            if !ui::commit(&ui, book) {
                return Err("couldn't save the address book".into());
            }
            Ok(json!({"added_to": name}))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_days_to_birthdays() {
        assert_eq!(days_until((10, 20), (10, 20, 2026)), Some(0));
        assert_eq!(days_until((10, 21), (10, 20, 2026)), Some(1));
        assert_eq!(days_until((10, 19), (10, 20, 2026)), Some(364));
        // 29 February: 1 March in a common year.
        assert_eq!(days_until((2, 29), (2, 28, 2027)), Some(1));
        assert_eq!(days_until((2, 29), (2, 28, 2028)), Some(1));
    }
}
