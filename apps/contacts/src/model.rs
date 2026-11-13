use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Field {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Contact {
    pub id: String,
    pub first: String,
    pub last: String,
    pub middle: String,
    pub prefix: String,
    pub suffix: String,
    pub display: String,
    pub company: String,
    pub title: String,
    pub birthday: String,
    pub notes: String,
    pub emails: Vec<Field>,
    pub phones: Vec<Field>,
    pub addresses: Vec<Field>,
    pub websites: Vec<Field>,
    pub favorite: bool,
    /// Stored as base64 (older address books have a JSON number array; both read).
    #[serde(with = "photo_b64")]
    pub photo: Vec<u8>,
    pub extra: Vec<String>,
}

mod photo_b64 {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Text(String),
            Bytes(Vec<u8>),
        }
        match Either::deserialize(d)? {
            Either::Text(t) => STANDARD.decode(t).map_err(D::Error::custom),
            Either::Bytes(b) => Ok(b),
        }
    }
}

/// The seven vCard address parts, in order. A free-form address (one written without
/// parts) is kept whole as the street.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Address {
    pub po_box: String,
    pub extended: String,
    pub street: String,
    pub city: String,
    pub region: String,
    pub postal: String,
    pub country: String,
}

impl Address {
    /// From a stored value: the seven parts joined by newlines.
    pub fn parse(value: &str) -> Self {
        let p: Vec<&str> = value.split('\n').collect();
        if p.len() != 7 {
            return Self { street: value.to_string(), ..Self::default() };
        }
        Self {
            po_box: p[0].into(),
            extended: p[1].into(),
            street: p[2].into(),
            city: p[3].into(),
            region: p[4].into(),
            postal: p[5].into(),
            country: p[6].into(),
        }
    }

    /// The stored value. Street may hold several lines; they're kept on one, comma-joined,
    /// so the seven parts stay seven.
    pub fn value(&self) -> String {
        let clean = |s: &str| s.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(", ");
        [&self.po_box, &self.extended, &self.street, &self.city, &self.region, &self.postal, &self.country]
            .map(|s| clean(s))
            .join("\n")
    }

    /// For reading: street, then "City, Region Postal", then country, one per line.
    pub fn display(&self) -> String {
        let locality = [self.city.as_str(), &[self.region.as_str(), &self.postal].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ")]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        [self.po_box.as_str(), &self.extended, &self.street, &locality, &self.country]
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.value().chars().all(|c| c == '\n')
    }
}

/// A birthday as `YYYY-MM-DD`, or `--MM-DD` without a year, from the forms address
/// books write: those two, `YYYYMMDD`, `--MMDD`, ISO with a time, and US `M/D/YYYY`.
/// None when it isn't a real date.
pub fn normalize_birthday(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    let date = s.split(['T', ' ']).next().unwrap_or(s);
    let (y, m, d): (Option<u32>, u32, u32) = if let Some(rest) = date.strip_prefix("--") {
        let digits: String = rest.chars().filter(char::is_ascii_digit).collect();
        if digits.len() != 4 {
            return None;
        }
        (None, digits[..2].parse().ok()?, digits[2..].parse().ok()?)
    } else if let Some((a, rest)) = date.split_once('/') {
        let (b, c) = rest.split_once('/')?;
        (Some(c.parse().ok()?), a.parse().ok()?, b.parse().ok()?)
    } else {
        let digits: String = date.chars().filter(char::is_ascii_digit).collect();
        if digits.len() != 8 || !(date.len() == 8 || date.len() == 10) {
            return None;
        }
        (Some(digits[..4].parse().ok()?), digits[4..6].parse().ok()?, digits[6..].parse().ok()?)
    };
    let year = y.unwrap_or(2000);
    if year == 0 || !(1..=12).contains(&m) {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max = match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if d == 0 || d > max {
        return None;
    }
    Some(match y {
        Some(y) => format!("{y:04}-{m:02}-{d:02}"),
        None => format!("--{m:02}-{d:02}"),
    })
}

/// Month and day of a normalized birthday.
pub fn birthday_md(s: &str) -> Option<(u32, u32)> {
    let md = s.strip_prefix("--").or_else(|| s.get(5..))?;
    let (m, d) = md.split_once('-')?;
    Some((m.parse().ok()?, d.parse().ok()?))
}
impl Default for Contact {
    fn default() -> Self {
        Self {
            id: id(),
            first: String::new(),
            last: String::new(),
            middle: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            display: String::new(),
            company: String::new(),
            title: String::new(),
            birthday: String::new(),
            notes: String::new(),
            emails: vec![],
            phones: vec![],
            addresses: vec![],
            websites: vec![],
            favorite: false,
            photo: vec![],
            extra: vec![],
        }
    }
}
impl Contact {
    pub fn name(&self, last_first: bool) -> String {
        if !self.display.is_empty() {
            return self.display.clone();
        }
        let n = if last_first { [self.last.as_str(), self.first.as_str()] } else { [self.first.as_str(), self.last.as_str()] };
        let n = n.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        if !n.is_empty() {
            n
        } else if !self.company.is_empty() {
            self.company.clone()
        } else {
            self.emails.first().map(|e| e.value.clone()).unwrap_or_else(|| "Unnamed contact".into())
        }
    }
    pub fn valid(&self) -> bool {
        [&self.first, &self.last, &self.display, &self.company].iter().any(|s| !s.trim().is_empty())
            || !self.emails.is_empty()
            || !self.phones.is_empty()
    }
    pub fn matches(&self, query: &str) -> bool {
        let text = format!(
            "{} {} {} {} {}",
            self.name(false),
            self.first,
            self.last,
            self.company,
            self.emails.iter().chain(&self.phones).map(|f| f.value.as_str()).collect::<Vec<_>>().join(" ")
        )
        .to_lowercase();
        query.split_whitespace().all(|q| {
            text.contains(&q.to_lowercase())
                || (!phone(q).is_empty() && self.phones.iter().any(|p| phone(&p.value).contains(&phone(q))))
        })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct List {
    pub id: String,
    pub name: String,
    pub members: BTreeSet<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub contacts: Vec<Contact>,
    pub lists: Vec<List>,
}

pub struct Store {
    db: Connection,
    /// What each contact row holds now (its JSON), so a save writes only what changed.
    saved: HashMap<String, String>,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let db = Connection::open(path)?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 1 {
            bail!("This address book was made by a newer Contacts version.");
        }
        db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS contacts(id TEXT PRIMARY KEY, data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS lists(id TEXT PRIMARY KEY, name TEXT NOT NULL); CREATE TABLE IF NOT EXISTS members(list_id TEXT REFERENCES lists(id) ON DELETE CASCADE, contact_id TEXT REFERENCES contacts(id) ON DELETE CASCADE, PRIMARY KEY(list_id,contact_id)); PRAGMA user_version=1;")?;
        Ok(Self { db, saved: HashMap::new() })
    }
    /// Reads the book without changing anything (`--find`, with no window).
    /// `None` when there's no address book yet.
    pub fn read(path: &Path) -> Result<Option<Book>> {
        if !path.exists() {
            return Ok(None);
        }
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        Self { db, saved: HashMap::new() }.load().map(Some)
    }
    pub fn load(&mut self) -> Result<Book> {
        let mut book = Book::default();
        let mut saved = HashMap::new();
        for row in self.db.prepare("SELECT id, data FROM contacts ORDER BY rowid")?.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (id, data) = row?;
            book.contacts
                .push(serde_json::from_str(&data).context("Couldn't read a contact; the database has been kept unchanged")?);
            saved.insert(id, data);
        }
        self.saved = saved;
        for row in self
            .db
            .prepare("SELECT id,name FROM lists ORDER BY name COLLATE NOCASE")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (id, name) = row?;
            let members = self
                .db
                .prepare("SELECT contact_id FROM members WHERE list_id=?")?
                .query_map([&id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<BTreeSet<_>>>()?;
            book.lists.push(List { id, name, members });
        }
        Ok(book)
    }
    /// Write `book`: contacts that changed, were added or went; lists and members whole
    /// (they're small). All in one transaction.
    pub fn save(&mut self, book: &Book) -> Result<()> {
        let rows: HashMap<String, String> =
            book.contacts.iter().map(|c| Ok((c.id.clone(), serde_json::to_string(c)?))).collect::<Result<_>>()?;
        let tx = self.db.transaction()?;
        tx.execute_batch("DELETE FROM members; DELETE FROM lists;")?;
        for id in self.saved.keys().filter(|id| !rows.contains_key(*id)) {
            tx.execute("DELETE FROM contacts WHERE id=?", [id])?;
        }
        for c in &book.contacts {
            let data = &rows[&c.id];
            if self.saved.get(&c.id) != Some(data) {
                tx.execute(
                    "INSERT INTO contacts VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
                    params![c.id, data],
                )?;
            }
        }
        for l in &book.lists {
            tx.execute("INSERT INTO lists VALUES(?,?)", params![l.id, l.name])?;
            for id in &l.members {
                tx.execute("INSERT INTO members VALUES(?,?)", params![l.id, id])?;
            }
        }
        tx.commit()?;
        self.saved = rows;
        Ok(())
    }
}
pub fn phone(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}
/// A phone number for matching: its last ten digits (so `+1 555-123-4567` and
/// `555-123-4567` match), or None when it's too short to be one.
pub fn phone_key(s: &str) -> Option<String> {
    let d = phone(s);
    (d.len() >= 7).then(|| d[d.len().saturating_sub(10)..].to_string())
}
fn email_key(s: &str) -> Option<String> {
    let e = s.trim().to_lowercase();
    (!e.is_empty()).then_some(e)
}
/// What two contacts share, for Duplicates: "phone number" or "email", or None.
pub fn shared(a: &Contact, b: &Contact) -> Option<&'static str> {
    let keys = |c: &Contact| c.phones.iter().filter_map(|f| phone_key(&f.value)).collect::<HashSet<_>>();
    if !keys(a).is_disjoint(&keys(b)) {
        return Some("phone number");
    }
    let mails = |c: &Contact| c.emails.iter().filter_map(|f| email_key(&f.value)).collect::<HashSet<_>>();
    (!mails(a).is_disjoint(&mails(b))).then_some("email")
}
pub fn duplicates(a: &Contact, b: &Contact) -> bool {
    a.emails
        .iter()
        .filter_map(|x| email_key(&x.value))
        .any(|x| b.emails.iter().any(|y| email_key(&y.value).as_ref() == Some(&x)))
        || a.phones
            .iter()
            .filter_map(|x| phone_key(&x.value))
            .any(|x| b.phones.iter().any(|y| phone_key(&y.value).as_ref() == Some(&x)))
}

/// Who shares an email or phone with whom, worked out once for a whole book.
#[derive(Default)]
pub struct DupIndex {
    by_key: HashMap<String, Vec<usize>>,
    ids: Vec<String>,
}
impl DupIndex {
    pub fn new(contacts: &[Contact]) -> Self {
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, c) in contacts.iter().enumerate() {
            let keys: HashSet<String> = c
                .emails
                .iter()
                .filter_map(|f| email_key(&f.value).map(|k| format!("e:{k}")))
                .chain(c.phones.iter().filter_map(|f| phone_key(&f.value).map(|k| format!("p:{k}"))))
                .collect();
            for k in keys {
                by_key.entry(k).or_default().push(i);
            }
        }
        Self { by_key, ids: contacts.iter().map(|c| c.id.clone()).collect() }
    }

    /// Ids of contacts that share something with another.
    pub fn flagged(&self) -> HashSet<&str> {
        self.by_key.values().filter(|v| v.len() > 1).flatten().map(|&i| self.ids[i].as_str()).collect()
    }

    /// Groups of contacts that are the same person by an exact email or phone, each
    /// group joined through shared values.
    pub fn groups(&self) -> Vec<Vec<&str>> {
        let n = self.ids.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        for v in self.by_key.values().filter(|v| v.len() > 1) {
            for w in v.windows(2) {
                let (a, b) = (root(&mut parent, w[0]), root(&mut parent, w[1]));
                parent[a] = b;
            }
        }
        let mut groups: HashMap<usize, Vec<&str>> = HashMap::new();
        for i in 0..n {
            let r = root(&mut parent, i);
            groups.entry(r).or_default().push(self.ids[i].as_str());
        }
        groups.into_values().filter(|g| g.len() > 1).collect()
    }
}

/// `b` folded into `a` for an automatic merge: `a`'s single values win, empty ones are
/// filled from `b`, and every distinct email, phone, address, website and note is kept.
pub fn combine(mut a: Contact, b: &Contact) -> Contact {
    for (x, y) in [
        (&mut a.first, &b.first),
        (&mut a.last, &b.last),
        (&mut a.middle, &b.middle),
        (&mut a.prefix, &b.prefix),
        (&mut a.suffix, &b.suffix),
        (&mut a.display, &b.display),
        (&mut a.company, &b.company),
        (&mut a.title, &b.title),
        (&mut a.birthday, &b.birthday),
    ] {
        if x.is_empty() {
            *x = y.clone();
        }
    }
    union_fields(&mut a.emails, &b.emails);
    union_fields(&mut a.phones, &b.phones);
    union_fields(&mut a.addresses, &b.addresses);
    union_fields(&mut a.websites, &b.websites);
    if !b.notes.is_empty() && a.notes != b.notes {
        if !a.notes.is_empty() {
            a.notes.push_str("\n\n");
        }
        a.notes.push_str(&b.notes);
    }
    a.favorite |= b.favorite;
    if a.photo.is_empty() {
        a.photo = b.photo.clone();
    }
    for extra in &b.extra {
        if !a.extra.contains(extra) && !extra.to_uppercase().starts_with("UID:") {
            a.extra.push(extra.clone());
        }
    }
    a
}
pub fn union_fields(a: &mut Vec<Field>, b: &[Field]) {
    for f in b {
        if !a.contains(f) {
            a.push(f.clone());
        }
    }
}
pub fn merge(book: &mut Book, primary: Contact, other: &str) {
    let target = primary.id.clone();
    book.contacts.retain(|c| c.id != *other && c.id != target);
    book.contacts.push(primary);
    for l in &mut book.lists {
        if l.members.remove(other) {
            l.members.insert(target.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_only_load_changes_nothing() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("nyx-contacts-read-{}", std::process::id()));
        let path = dir.join("contacts.db");
        assert!(Store::read(&path)?.is_none(), "no book yet: nothing created");
        assert!(!path.exists());
        let mut store = Store::open(&path)?;
        let sam = Contact {
            first: "Sam".into(),
            emails: vec![Field { label: "home".into(), value: "sam@example.org".into() }],
            ..Default::default()
        };
        store.save(&Book { contacts: vec![sam], lists: Vec::new() })?;
        drop(store);
        let before = std::fs::read(&path)?;
        let book = Store::read(&path)?.unwrap();
        assert!(book.contacts[0].matches("sam@exa"));
        assert_eq!(std::fs::read(&path)?, before);
        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }
    #[test]
    fn persistence_membership_and_merge() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let a = Contact {
            first: "Ada".into(),
            emails: vec![Field { label: "work".into(), value: "ada@example.org".into() }],
            ..Default::default()
        };
        let b = Contact {
            first: "Ada".into(),
            emails: vec![Field { label: "home".into(), value: "ADA@example.org".into() }],
            ..Default::default()
        };
        assert!(duplicates(&a, &b));
        assert!(!duplicates(&a, &Contact { first: "Ada".into(), ..Default::default() }));
        let mut book = Book {
            contacts: vec![a.clone(), b.clone()],
            lists: vec![List { id: id(), name: "Work".into(), members: [b.id.clone()].into() }],
        };
        store.save(&book)?;
        assert_eq!(store.load()?, book);
        merge(&mut book, a.clone(), &b.id);
        store.save(&book)?;
        assert_eq!(store.load()?.contacts, vec![a.clone()]);
        assert!(store.load()?.lists[0].members.contains(&a.id));
        let mut broken = book.clone();
        broken.lists[0].members.insert("missing".into());
        assert!(store.save(&broken).is_err());
        assert_eq!(store.load()?, book);
        Ok(())
    }

    #[test]
    fn saves_only_changes_and_reads_old_photos() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let mut book = Book { contacts: vec![Contact { first: "Ada".into(), photo: vec![1, 2, 3], ..Default::default() }], lists: vec![] };
        store.save(&book)?;
        book.contacts.push(Contact { first: "Bea".into(), ..Default::default() });
        book.contacts.remove(0);
        store.save(&book)?;
        assert_eq!(store.load()?, book);
        // Photos written as a number array before base64 still read.
        let old: Contact = serde_json::from_str(r#"{"id":"x","first":"Old","photo":[255,216]}"#)?;
        assert_eq!(old.photo, vec![255, 216]);
        assert!(serde_json::to_string(&old)?.contains("\"photo\":\"/9g=\""));
        Ok(())
    }

    #[test]
    fn normalizes_birthdays() {
        for (input, want) in [
            ("1985-04-12", Some("1985-04-12")),
            ("19850412", Some("1985-04-12")),
            ("1985-04-12T00:00:00Z", Some("1985-04-12")),
            ("--0412", Some("--04-12")),
            ("--04-12", Some("--04-12")),
            ("4/12/1985", Some("1985-04-12")),
            ("2023-02-29", None),
            ("tomorrow", None),
            ("", Some("")),
        ] {
            assert_eq!(normalize_birthday(input).as_deref(), want, "{input}");
        }
        assert_eq!(birthday_md("--04-12"), Some((4, 12)));
        assert_eq!(birthday_md("1985-04-12"), Some((4, 12)));
    }

    #[test]
    fn addresses_keep_their_parts() {
        let a = Address::parse("\n\n1 Main St\nSpringfield\nIL\n62701\nUSA");
        assert_eq!(a.city, "Springfield");
        assert_eq!(a.value(), "\n\n1 Main St\nSpringfield\nIL\n62701\nUSA");
        assert_eq!(a.display(), "1 Main St\nSpringfield, IL 62701\nUSA");
        assert_eq!(Address::parse("somewhere").street, "somewhere");
    }

    #[test]
    fn duplicate_index_groups_by_email_and_phone() {
        let c = |first: &str, email: &str, phone: &str| Contact {
            first: first.into(),
            emails: if email.is_empty() { vec![] } else { vec![Field { label: "home".into(), value: email.into() }] },
            phones: if phone.is_empty() { vec![] } else { vec![Field { label: "cell".into(), value: phone.into() }] },
            ..Default::default()
        };
        let book = vec![c("A", "a@x.org", ""), c("B", "A@X.org ", "+1 555-123-4567"), c("C", "", "555 123 4567"), c("D", "d@x.org", "")];
        let idx = DupIndex::new(&book);
        assert_eq!(idx.flagged().len(), 3);
        let groups = idx.groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 3);
    }
}
