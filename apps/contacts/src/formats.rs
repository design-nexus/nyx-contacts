//! File interchange stays independent of GTK so previews and round trips share one parser.
use crate::model::{self, Contact, Field};
use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};

#[derive(Default)]
pub struct Import {
    pub contacts: Vec<Contact>,
    pub warnings: Vec<String>,
}
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace(';', "\\;").replace(',', "\\,").replace('\r', "")
}
fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(c) = chars.next() {
                out.push(if c == 'n' || c == 'N' { '\n' } else { c });
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn parts(s: &str) -> Vec<String> {
    let mut v = vec![String::new()];
    let mut escaped = false;
    for c in s.chars() {
        if c == ';' && !escaped {
            v.push(String::new());
        } else {
            v.last_mut().unwrap().push(c);
        }
        if c == '\\' {
            escaped = !escaped;
        } else {
            escaped = false;
        }
    }
    v.iter().map(|s| unesc(s)).collect()
}
/// TYPE values that say nothing a person would label a value with.
const NOISE: &[&str] = &["internet", "voice", "pref", "x400", "msg", "uri", "text", "x-internet"];

/// A property's label from all its TYPE values: `TYPE=a,b`, repeated `TYPE=`, and vCard
/// 2.1's bare `HOME`/`CELL` params. The first meaningful one wins; `other` if none is.
fn label_of(params: &[&str]) -> String {
    params
        .iter()
        .flat_map(|p| match p.split_once('=') {
            Some((k, v)) if k.eq_ignore_ascii_case("TYPE") => v.trim_matches('"').split(',').collect::<Vec<_>>(),
            Some(_) => vec![],
            None => vec![*p],
        })
        .map(|t| t.trim().to_lowercase())
        .find(|t| !t.is_empty() && !NOISE.contains(&t.as_str()) && !t.starts_with("x-"))
        .map(|t| if t == "mobile" { "cell".into() } else { t })
        .unwrap_or_else(|| "other".into())
}

/// The property name of a stored raw line (`item1.ORG;CHARSET=x:…` → `ORG`).
fn property_of(line: &str) -> String {
    let key = line.split(':').next().unwrap_or("");
    key.split(';').next().unwrap_or("").rsplit('.').next().unwrap_or("").to_uppercase()
}

/// Quoted-printable (vCard 2.1) to text: `=XX` bytes, read as UTF-8 (Latin-1 if not).
fn quoted_printable(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' && i + 2 < b.len() {
            // Bytes, not a str slice: the two after `=` may not be a char boundary.
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out.clone()).unwrap_or_else(|_| out.iter().map(|&c| c as char).collect())
}

/// The vCard UID a contact came with, kept among its raw lines.
pub fn uid(c: &Contact) -> Option<String> {
    c.extra
        .iter()
        .find(|l| property_of(l) == "UID")
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub fn vcard_read(text: &str) -> Import {
    let mut lines: Vec<String> = vec![];
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim_end_matches('\r');
        let qp_open = lines.last().is_some_and(|l: &String| l.to_uppercase().contains("QUOTED-PRINTABLE") && l.ends_with('='));
        if qp_open {
            // A quoted-printable soft line break: `=` at the end continues the value.
            let last = lines.last_mut().unwrap();
            last.pop();
            last.push_str(line);
        } else if (line.starts_with(' ') || line.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&line[1..]);
        } else {
            lines.push(line.into());
        }
    }
    let mut result = Import::default();
    let mut card: Option<Contact> = None;
    let mut version = false;
    let mut bad = false;
    for line in lines {
        if line.eq_ignore_ascii_case("BEGIN:VCARD") {
            if card.is_some() {
                result.warnings.push("Skipped an unfinished contact card.".into());
            }
            card = Some(Contact::default());
            version = false;
            bad = false;
            continue;
        }
        if line.eq_ignore_ascii_case("END:VCARD") {
            if let Some(mut c) = card.take() {
                let display = std::mem::take(&mut c.display);
                if display != c.name(false) {
                    c.display = display;
                }
                if version && !bad && c.valid() {
                    result.contacts.push(c);
                } else {
                    result
                        .warnings
                        .push("Skipped a malformed, empty, or unsupported contact card (vCard 2.1, 3.0 or 4.0).".into());
                }
            }
            continue;
        }
        let Some(c) = card.as_mut() else { continue };
        let Some((key, raw_value)) = line.split_once(':') else {
            bad = true;
            continue;
        };
        let mut key_parts = key.split(';');
        let property = key_parts.next().unwrap().rsplit('.').next().unwrap().to_uppercase();
        let params: Vec<_> = key_parts.collect();
        let qp = params
            .iter()
            .any(|p| p.eq_ignore_ascii_case("ENCODING=QUOTED-PRINTABLE") || p.eq_ignore_ascii_case("QUOTED-PRINTABLE"));
        let decoded = if qp { quoted_printable(raw_value) } else { raw_value.to_string() };
        let value = decoded.as_str();
        let field = Field { label: label_of(&params), value: unesc(value) };
        match property.as_str() {
            "VERSION" => version = matches!(value.trim(), "2.1" | "3.0" | "4.0"),
            "UID" => {} // Imported cards always get a local identity; their UID is retained separately.
            "FN" => c.display = unesc(value),
            "N" => {
                let p = parts(value);
                c.last = p.first().cloned().unwrap_or_default();
                c.first = p.get(1).cloned().unwrap_or_default();
                c.middle = p.get(2).cloned().unwrap_or_default();
                c.prefix = p.get(3).cloned().unwrap_or_default();
                c.suffix = p.get(4).cloned().unwrap_or_default();
            }
            "ORG" => {
                let p = parts(value);
                c.company = p.first().cloned().unwrap_or_default();
                if p.len() > 1 {
                    c.extra.push(line.clone());
                }
            }
            "TITLE" => c.title = unesc(value),
            "BDAY" => {
                let raw = unesc(value);
                c.birthday = model::normalize_birthday(&raw).unwrap_or_else(|| {
                    result.warnings.push(format!("Kept the birthday “{raw}” as it is; it isn't a date Contacts reads."));
                    raw
                });
            }
            "NOTE" => c.notes = unesc(value),
            "EMAIL" => c.emails.push(field),
            "TEL" => c.phones.push(Field { value: field.value.trim_start_matches("tel:").to_string(), ..field }),
            "URL" => c.websites.push(field),
            "ADR" => c.addresses.push(Field { value: parts(value).join("\n"), ..field }),
            "PHOTO" => {
                let encoded = value.split_once(";base64,").map(|(_, s)| s).or_else(|| {
                    params
                        .iter()
                        .any(|p| {
                            p.eq_ignore_ascii_case("ENCODING=b")
                                || p.eq_ignore_ascii_case("ENCODING=BASE64")
                                || p.eq_ignore_ascii_case("BASE64")
                        })
                        .then_some(value)
                });
                if let Some(data) = encoded {
                    match STANDARD.decode(data.trim()) {
                        Ok(bytes) => c.photo = bytes,
                        Err(_) => {
                            result
                                .warnings
                                .push("A contact photo could not be decoded; its original property was retained.".into());
                            c.extra.push(line.clone());
                        }
                    }
                } else {
                    c.extra.push(line.clone());
                }
            }
            // Cards exported before the suite was called Nyx say X-NIXE-FAVORITE.
            "X-NYX-FAVORITE" | "X-NIXE-FAVORITE" => c.favorite = value == "1",
            _ => c.extra.push(line.clone()),
        }
        if property == "UID" {
            c.extra.push(line);
        }
    }
    if card.is_some() {
        result.warnings.push("Skipped an unfinished contact card.".into());
    }
    if result.contacts.is_empty() && result.warnings.is_empty() {
        result.warnings.push("No vCard contacts found.".into());
    }
    result
}
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out.push_str("\r\n");
    out
}
pub fn vcard_write(contacts: &[Contact]) -> String {
    let mut out = String::new();
    for c in contacts {
        let mut lines = vec![
            "BEGIN:VCARD".into(),
            "VERSION:3.0".into(),
            format!("FN:{}", esc(&c.name(false))),
            format!("N:{};{};{};{};{}", esc(&c.last), esc(&c.first), esc(&c.middle), esc(&c.prefix), esc(&c.suffix)),
        ];
        if !c.extra.iter().any(|s| property_of(s) == "UID") {
            lines.push(format!("UID:{}", c.id));
        }
        for (key, v) in [("ORG", &c.company), ("TITLE", &c.title), ("BDAY", &c.birthday), ("NOTE", &c.notes)] {
            if !v.is_empty() {
                lines.push(format!("{key}:{}", esc(v)));
            }
        }
        for (key, fields) in [("EMAIL", &c.emails), ("TEL", &c.phones), ("URL", &c.websites), ("ADR", &c.addresses)] {
            for f in fields {
                let label: String = f.label.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ',' || *c == '-').collect();
                let value = if key == "ADR" {
                    let p: Vec<_> = f.value.split('\n').collect();
                    if p.len() == 7 {
                        p.into_iter().map(esc).collect::<Vec<_>>().join(";")
                    } else {
                        format!(";;{};;;;", esc(&f.value))
                    }
                } else {
                    esc(&f.value)
                };
                lines.push(format!("{key};TYPE={}:{}", if label.is_empty() { "other" } else { &label }, value));
            }
        }
        if !c.photo.is_empty() {
            let mime = if c.photo.starts_with(b"\x89PNG") { "PNG" } else { "JPEG" };
            lines.push(format!("PHOTO;ENCODING=b;TYPE={mime}:{}", STANDARD.encode(&c.photo)));
        }
        if c.favorite {
            lines.push("X-NYX-FAVORITE:1".into());
        }
        // ORG may carry departments; retain it only when it still agrees with the edited company.
        for extra in &c.extra {
            if property_of(extra) == "ORG" {
                if extra.split_once(':').map(|(_, v)| parts(v)).and_then(|p| p.first().cloned()) == Some(c.company.clone()) {
                    lines.retain(|l| !l.starts_with("ORG:"));
                    lines.push(extra.clone());
                }
            } else {
                lines.push(extra.clone());
            }
        }
        lines.push("END:VCARD".into());
        for line in lines {
            out.push_str(&fold(&line));
        }
    }
    out
}

pub const COLUMNS: &[&str] = &[
    "First Name",
    "Last Name",
    "Middle Name",
    "Title",
    "Suffix",
    "Company",
    "Job Title",
    "Birthday",
    "Notes",
    "E-mail Address",
    "E-mail 2 Address",
    "E-mail 3 Address",
    "Mobile Phone",
    "Home Phone",
    "Business Phone",
    "Home Street",
    "Business Street",
    "Web Page",
];
pub struct CsvData {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub warnings: Vec<String>,
}
pub fn csv_data(text: &str) -> Result<CsvData> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let headers = reader.headers()?.iter().map(str::to_string).collect();
    let mut rows = vec![];
    let mut warnings = vec![];
    for (i, row) in reader.records().enumerate() {
        match row {
            Ok(row) => rows.push(row.iter().map(str::to_string).collect()),
            Err(e) => warnings.push(format!("CSV row {}: {e}", i + 2)),
        }
    }
    Ok(CsvData { headers, rows, warnings })
}
pub fn mapping(headers: &[String]) -> Vec<Option<usize>> {
    headers
        .iter()
        .map(|h| {
            let normalized = h.to_lowercase().replace([' ', '_', '-'], "");
            COLUMNS.iter().position(|c| c.to_lowercase().replace([' ', '-'], "") == normalized).or(match normalized.as_str() {
                "givenname" => Some(0),
                "surname" | "familyname" => Some(1),
                "organization" => Some(5),
                "email" | "emailaddress" => Some(9),
                "phone" | "telephone" => Some(12),
                "website" => Some(17),
                _ => None,
            })
        })
        .collect()
}
pub fn csv_contacts(data: &CsvData, map: &[Option<usize>]) -> Import {
    let mut out = Import { warnings: data.warnings.clone(), ..Default::default() };
    for (i, row) in data.rows.iter().enumerate() {
        let mut c = Contact::default();
        for (value, column) in row.iter().zip(map) {
            if value.trim().is_empty() {
                continue;
            }
            match column {
                Some(0) => c.first = value.clone(),
                Some(1) => c.last = value.clone(),
                Some(2) => c.middle = value.clone(),
                Some(3) => c.prefix = value.clone(),
                Some(4) => c.suffix = value.clone(),
                Some(5) => c.company = value.clone(),
                Some(6) => c.title = value.clone(),
                Some(7) => c.birthday = model::normalize_birthday(value).unwrap_or_else(|| value.clone()),
                Some(8) => c.notes = value.clone(),
                Some(9..=11) => c.emails.push(Field { label: "other".into(), value: value.clone() }),
                // Several numbers in one column are written "; "-separated.
                Some(12..=14) => c.phones.extend(value.split("; ").map(str::trim).filter(|v| !v.is_empty()).map(|v| {
                    Field {
                        label: match column {
                            Some(12) => "cell",
                            Some(13) => "home",
                            _ => "work",
                        }
                        .into(),
                        value: v.to_string(),
                    }
                })),
                // One address per line; each is kept as free-form street text.
                Some(15..=16) => c.addresses.extend(value.lines().map(str::trim).filter(|v| !v.is_empty()).map(|v| Field {
                    label: if *column == Some(15) { "home" } else { "work" }.into(),
                    value: model::Address { street: v.to_string(), ..Default::default() }.value(),
                })),
                Some(17) => c.websites.push(Field { label: "other".into(), value: value.clone() }),
                _ => {}
            }
        }
        if c.valid() {
            out.contacts.push(c);
        } else {
            out.warnings.push(format!("CSV row {} has no mapped contact identity and was skipped.", i + 2));
        }
    }
    out
}
pub fn csv_write(contacts: &[Contact]) -> Result<Vec<u8>> {
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record(COLUMNS)?;
    for c in contacts {
        let mut row = vec![String::new(); COLUMNS.len()];
        for (i, s) in [
            (0, &c.first),
            (1, &c.last),
            (2, &c.middle),
            (3, &c.prefix),
            (4, &c.suffix),
            (5, &c.company),
            (6, &c.title),
            (7, &c.birthday),
            (8, &c.notes),
        ] {
            row[i] = s.clone();
        }
        if row[0].is_empty() && row[1].is_empty() {
            row[0] = c.name(false);
        }
        for (i, f) in c.emails.iter().take(3).enumerate() {
            row[9 + i] = f.value.clone();
        }
        for f in &c.phones {
            let i = match f.label.as_str() {
                "cell" | "mobile" => 12,
                "work" => 14,
                _ => 13,
            };
            if !row[i].is_empty() {
                row[i].push_str("; ");
            }
            row[i].push_str(&f.value);
        }
        for f in &c.addresses {
            let i = if f.label == "work" { 16 } else { 15 };
            if !row[i].is_empty() {
                row[i].push('\n');
            }
            // One line per address: "1 Main St, Springfield, IL 62701, USA".
            row[i].push_str(&model::Address::parse(&f.value).display().replace('\n', ", "));
        }
        row[17] = c.websites.first().map(|f| f.value.clone()).unwrap_or_default();
        // A cell starting with = + - @ would run as a formula in a spreadsheet.
        for cell in &mut row {
            if cell.starts_with(['=', '+', '-', '@']) {
                cell.insert(0, '\'');
            }
        }
        w.write_record(row)?;
    }
    Ok(w.into_inner()?)
}
pub fn read(path: &std::path::Path) -> Result<(String, bool)> {
    let meta = path.metadata()?;
    if meta.len() > 50 * 1024 * 1024 {
        bail!("Import files must be smaller than 50 MB.");
    }
    Ok((decode(&std::fs::read(path)?), path.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv"))))
}

/// File bytes as text: UTF-8, UTF-16 with a byte-order mark, or else Windows-1252
/// (what Outlook and Excel write CSV in).
pub fn decode(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let utf16 = |le: bool| {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, ..] => return utf16(true),
        [0xFE, 0xFF, ..] => return utf16(false),
        _ => {}
    }
    // Windows-1252's 0x80–0x9F; the rest is Latin-1.
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“',
        '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    bytes.iter().map(|&b| if (0x80..0xA0).contains(&b) { HIGH[(b - 0x80) as usize] } else { b as char }).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vcard_roundtrip_and_malformed() {
        let text = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Zoë Example\r\nN:Example;Zoë;;;\r\nEMAIL;TYPE=work:zoe@example.org\r\nNOTE:Line 1\\nLine 2\\;yes\r\nX-CUSTOM:preserve me\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nFN:Unfinished";
        let i = vcard_read(text);
        assert_eq!(i.contacts.len(), 1);
        assert_eq!(i.warnings.len(), 1);
        let mut c = i.contacts[0].clone();
        c.notes.push_str(&"🙂".repeat(80));
        c.photo = vec![1, 2, 3];
        let encoded = vcard_write(&[c.clone()]);
        assert!(encoded.lines().all(|l| l.len() <= 75));
        let next = vcard_read(&encoded);
        assert_eq!(next.contacts[0].notes, c.notes);
        assert_eq!(next.contacts[0].photo, c.photo);
        assert!(next.contacts[0].extra.iter().any(|s| s == "X-CUSTOM:preserve me"));
    }
    #[test]
    fn reads_google_apple_and_old_cards() {
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada\r\nN:;Ada;;;\r\nEMAIL;TYPE=INTERNET;TYPE=HOME:ada@home.org\r\nTEL;type=CELL;type=VOICE;type=pref:+1 555\r\nitem1.ADR;type=WORK:;;1 Main St;Springfield;IL;62701;USA\r\nBDAY:19850412\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:2.1\r\nN;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:;Zo=C3=\r\n=AB;;;\r\nTEL;CELL:12345678\r\nEND:VCARD\r\n";
        let i = vcard_read(text);
        assert_eq!(i.contacts.len(), 2, "{:?}", i.warnings);
        let ada = &i.contacts[0];
        assert_eq!(ada.emails[0].label, "home");
        assert_eq!(ada.phones[0].label, "cell");
        assert_eq!(ada.birthday, "1985-04-12");
        let adr = model::Address::parse(&ada.addresses[0].value);
        assert_eq!((adr.street.as_str(), adr.city.as_str(), adr.country.as_str()), ("1 Main St", "Springfield", "USA"));
        // And back out with its parts.
        assert!(vcard_write(std::slice::from_ref(ada)).contains("ADR;TYPE=work:;;1 Main St;Springfield;IL;62701;USA"));
        let zoe = &i.contacts[1];
        assert_eq!(zoe.first, "Zoë");
        assert_eq!(zoe.phones[0].label, "cell");
    }

    #[test]
    fn decodes_windows_text() {
        assert_eq!(decode(b"Jos\xe9 \x93q\x94"), "José “q”");
        assert_eq!(decode(&[0xFF, 0xFE, b'h', 0, b'i', 0]), "hi");
        assert_eq!(decode("Zoë".as_bytes()), "Zoë");
    }

    #[test]
    fn csv_splits_phones_and_defuses_formulas() -> Result<()> {
        let c = Contact {
            first: "=cmd".into(),
            phones: vec![
                Field { label: "home".into(), value: "111 1111".into() },
                Field { label: "home".into(), value: "222 2222".into() },
            ],
            ..Default::default()
        };
        let bytes = csv_write(&[c])?;
        let data = csv_data(std::str::from_utf8(&bytes)?)?;
        let back = csv_contacts(&data, &mapping(&data.headers));
        assert_eq!(back.contacts[0].phones.len(), 2);
        assert_eq!(back.contacts[0].first, "'=cmd");
        Ok(())
    }

    #[test]
    fn csv_quotes_mapping_and_roundtrip() -> Result<()> {
        let data = csv_data("Given Name,Surname,Email,Notes\r\nAda,Lovelace,ada@example.org,\"Hello, world\nNext line\"\r\n,,,")?;
        let parsed = csv_contacts(&data, &mapping(&data.headers));
        assert_eq!(parsed.contacts.len(), 1);
        assert_eq!(parsed.warnings.len(), 1);
        let bytes = csv_write(&parsed.contacts)?;
        let data = csv_data(std::str::from_utf8(&bytes)?)?;
        let next = csv_contacts(&data, &mapping(&data.headers));
        assert_eq!(next.contacts[0].notes, "Hello, world\nNext line");
        assert_eq!(next.contacts[0].emails, parsed.contacts[0].emails);
        Ok(())
    }
}
