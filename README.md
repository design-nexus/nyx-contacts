# Contacts

A local address book: contacts, favorites, lists, upcoming birthdays, duplicates, and
vCard and CSV import and export. Address books from accounts connected in Online
Accounts can be read alongside it.

## Install

On NyxOS, choose Contacts in Welcome when you set up, or install it later from the Store or with `nyx install nyx-contacts`. On Omarchy:

```sh
curl -fsSL https://raw.githubusercontent.com/design-nexus/nyx-contacts/main/install.sh | bash -s -- contacts
```

This builds with Cargo and installs `nyx-contacts` to `~/.local/bin` with a launcher
entry. To remove it, run the same line with `uninstall.sh` in place of `install.sh`.

## Features

- **Contacts** with labeled emails, phones, postal addresses (street, city, region,
  postal code and country), websites, birthday, notes and a photo. Labels are typed or
  picked from the usual ones. Photos are scaled to 512 px when chosen.
- **Lists and favorites.** The toolbar menu adds the selection to a list, removes it,
  deletes it (with Undo), emails a list, and imports or exports.
- **Birthdays:** everyone with a birthday, soonest first, with how many days away; the
  sidebar counts the next 30 days.
- **Duplicates:** contacts that share an email or phone (phones match on their last ten
  digits, so `+1 555-123-4567` and `555-123-4567` match). Review a merge field by field, or
  merge every exact group at once from the toolbar menu (with Undo). Merges keep every
  distinct email, phone, address, website, note and list.
- **Online books:** contacts from Online Accounts address books (through Nyx
  Online Accounts), read-only.
- **Import:** vCard 2.1, 3.0 and 4.0 (including quoted-printable and the labels Google and
  Apple write) and CSV with a column mapping screen, in UTF-8, UTF-16 or Windows-1252. A
  card carrying the UID of one already here updates it instead of adding a copy.
- **Export:** vCard 3.0, keeping unknown properties; or Outlook-style CSV (three email
  columns, mobile/home/business phones, home/business addresses, one website; cells that
  would run as spreadsheet formulas are quoted). CSV leaves out photos, lists, favorites
  and custom properties. Export uses the selection, or the whole current view when
  nothing is selected.
- **Nyx Assistant** works Contacts over the session bus: `contacts.find`,
  `contacts.birthdays`, `contacts.lists`, `contacts.add` (refused for an email or phone
  someone already has), `contacts.update` (changes or adds; notes are added to, never
  replaced) and `contacts.add_to_list`.

## Usage

```
nyx-contacts [--section all|favorites|birthdays|duplicates|settings] [--toggle] [FILE.vcf|FILE.csv …]
```

A file argument opens its import preview.

| Shortcut | Action |
| --- | --- |
| Ctrl+N | New contact |
| Ctrl+S | Save contact in the editor |
| Ctrl+F | Search contacts |
| Ctrl+B | Collapse sidebar |
| Ctrl+, | Settings |
| Ctrl+Shift+P | Command palette |
| Esc | Close search/card or return to list |
| F1 / ? | Shortcuts |
| Ctrl+Q / Ctrl+W | Close window |

## Files

- Address book: `~/.local/share/nyx-contacts/contacts.db`. Photos are stored in it; back
  it up while Contacts is closed.
- Settings: `~/.config/nyx-contacts/settings.toml`
