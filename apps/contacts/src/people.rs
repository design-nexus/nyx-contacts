//! Everyone in every shown source, once: copies of the same person (sharing an email or
//! a phone, or with the same name and neither) combined into one, their details without
//! repeats.
use crate::model::{self, Contact};
use std::collections::{HashMap, HashSet};

/// Where a copy of a person is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Local,
    Book(String),
}

#[derive(Clone, Debug)]
pub struct Person {
    /// The combined card. Its id is the copy on this computer's, when there is one, so
    /// lists and Favorites keep working; else `p:<book>:<id>` of its first copy.
    pub contact: Contact,
    /// Each copy and where it's kept, in source order.
    pub members: Vec<(Origin, Contact)>,
}

impl Person {
    pub fn local(&self) -> Option<&Contact> {
        self.members.iter().find(|(o, _)| *o == Origin::Local).map(|(_, c)| c)
    }
}

/// What makes two cards the same person.
fn keys(c: &Contact) -> Vec<String> {
    let mut keys: Vec<String> = c
        .emails
        .iter()
        .map(|f| f.value.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .map(|e| format!("e:{e}"))
        .collect();
    keys.extend(c.phones.iter().filter_map(|f| model::phone_key(&f.value)).map(|p| format!("p:{p}")));
    // The name: a full one (two words or more) always, a single word only when there's
    // nothing else to go on. An email written as the name isn't one.
    let name = c.name(false).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let words = name.split(' ').count();
    if !name.is_empty() && !name.contains('@') && name != "unnamed contact" && (words >= 2 || keys.is_empty()) {
        keys.push(format!("n:{name}"));
    }
    keys
}

/// The id a person goes by.
pub fn key_of(origin: &Origin, c: &Contact) -> String {
    match origin {
        Origin::Local => c.id.clone(),
        Origin::Book(book) => format!("p:{book}:{}", c.id),
    }
}

/// `sources` in order of preference: this computer, then books you can change, then
/// read-only ones. A person's single values come from the first copy that has them.
pub fn combine(sources: Vec<(Origin, Vec<Contact>)>) -> Vec<Person> {
    let all: Vec<(Origin, Contact)> =
        sources.into_iter().flat_map(|(o, cards)| cards.into_iter().map(move |c| (o.clone(), c))).collect();
    let mut parent: Vec<usize> = (0..all.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut first: HashMap<String, usize> = HashMap::new();
    for (i, (_, c)) in all.iter().enumerate() {
        for k in keys(c) {
            match first.get(&k) {
                Some(&j) => {
                    let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                    if a != b {
                        // The earlier card leads the group.
                        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                        parent[hi] = lo;
                    }
                }
                None => {
                    first.insert(k, i);
                }
            }
        }
    }
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut at: HashMap<usize, usize> = HashMap::new();
    for i in 0..all.len() {
        let r = root(&mut parent, i);
        match at.get(&r) {
            Some(&g) => groups[g].1.push(i),
            None => {
                at.insert(r, groups.len());
                groups.push((r, vec![i]));
            }
        }
    }
    groups
        .into_iter()
        .map(|(_, members)| {
            let members: Vec<(Origin, Contact)> = members.into_iter().map(|i| all[i].clone()).collect();
            let lead = members.iter().find(|(o, _)| *o == Origin::Local).unwrap_or(&members[0]);
            let id = key_of(&lead.0, &lead.1);
            let mut contact = members.iter().skip(1).fold(members[0].1.clone(), |a, (_, b)| model::combine(a, b));
            contact.id = id;
            tidy(&mut contact);
            Person { contact, members }
        })
        .collect()
}

/// One of each detail: emails by address, phones by number, addresses and websites once.
fn tidy(c: &mut Contact) {
    for list in [&mut c.emails, &mut c.phones, &mut c.websites] {
        list.retain(|f| !f.value.trim().is_empty());
    }
    c.addresses.retain(|f| !model::Address::parse(&f.value).display().trim().is_empty());
    let mut seen = HashSet::new();
    c.emails.retain(|f| seen.insert(f.value.trim().to_lowercase()));
    let mut seen = HashSet::new();
    c.phones.retain(|f| seen.insert(model::phone_key(&f.value).unwrap_or_else(|| f.value.trim().to_string())));
    let mut seen = HashSet::new();
    c.addresses.retain(|f| seen.insert(model::Address::parse(&f.value).display().to_lowercase()));
    let mut seen = HashSet::new();
    c.websites.retain(|f| {
        let v = f.value.trim().trim_end_matches('/').to_lowercase();
        seen.insert(v.trim_start_matches("https://").trim_start_matches("http://").to_string())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Field;
    fn card(id: &str, name: &str, email: &str, phone: &str) -> Contact {
        Contact {
            id: id.into(),
            first: name.into(),
            emails: if email.is_empty() { vec![] } else { vec![Field { label: "home".into(), value: email.into() }] },
            phones: if phone.is_empty() { vec![] } else { vec![Field { label: "cell".into(), value: phone.into() }] },
            ..Default::default()
        }
    }
    #[test]
    fn copies_combine_without_repeats() {
        let people = combine(vec![
            (Origin::Local, vec![card("a", "Sam", "sam@x.example", "555-987-6543")]),
            (Origin::Book("g".into()), vec![card("1", "Sam", "SAM@x.example", "+1 555-987-6543"), card("2", "Ada", "", "")]),
            (Origin::Book("old".into()), vec![card("9", "Ada", "", ""), card("8", "Bea", "bea@x.example", "")]),
        ]);
        assert_eq!(people.len(), 3);
        let sam = &people[0];
        assert_eq!(sam.contact.id, "a");
        assert_eq!(sam.members.len(), 2);
        assert_eq!(sam.contact.emails.len(), 1);
        assert_eq!(sam.contact.phones.len(), 1);
        let ada = &people[1];
        assert_eq!(ada.contact.id, "p:g:2");
        assert_eq!(ada.members.len(), 2);
    }
}
