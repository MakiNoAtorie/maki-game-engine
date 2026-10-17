//! Export and import of the text tables as one CSV file, for translators: a column
//! `key`, a column for each language, a row for each key. UTF-8, RFC 4180.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bevy_ecs::world::World;
use maki_assets::{data_text, read_data};
use maki_events::{TextEntry, TextTable, Texts};
use maki_ui::ToolCommand;

use crate::Editor;

/// The CSV file, in the directory of the room files.
const CSV_PATH: &str = "data/text/texts.csv";
/// Written at the start of the export: spreadsheet programs then read UTF-8.
const BOM: char = '\u{feff}';

/// The text of a CSV file. A field with a comma, a quote, or a line end is quoted.
pub fn write_csv(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .map(|cell| {
                if cell.contains([',', '"', '\r', '\n']) {
                    format!("\"{}\"", cell.replace('"', "\"\""))
                } else {
                    cell.clone()
                }
            })
            .collect();
        out.push_str(&cells.join(","));
        out.push_str("\r\n");
    }
    out
}

/// The rows of a CSV file. `Err`: what is wrong, with the line.
pub fn read_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let text = text.strip_prefix(BOM).unwrap_or(text);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut line = 1;
    // The line where the open quote started: `Some` while inside a quoted field.
    let mut quoted_at: Option<usize> = None;
    // True after the closing quote of a field.
    let mut closed = false;
    while let Some(c) = chars.next() {
        if quoted_at.is_some() {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => {
                    quoted_at = None;
                    closed = true;
                }
                _ => {
                    line += usize::from(c == '\n');
                    field.push(c);
                }
            }
            continue;
        }
        match c {
            ',' => {
                row.push(std::mem::take(&mut field));
                closed = false;
            }
            // The line feed of "\r\n" ends the row.
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                closed = false;
                line += 1;
            }
            '"' if field.is_empty() && !closed => quoted_at = Some(line),
            _ if closed => return Err(format!("line {line}: text after a closing quote")),
            '"' => return Err(format!("line {line}: quote in a field with no quotes")),
            _ => field.push(c),
        }
    }
    if let Some(start) = quoted_at {
        return Err(format!("line {start}: a quote is not closed"));
    }
    if !field.is_empty() || !row.is_empty() || closed {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

/// Each text table under `directory`, with its path, in the order of the paths.
fn read_tables(directory: &Path) -> Result<Vec<(PathBuf, TextTable)>, String> {
    let mut tables = Vec::new();
    for path in maki_events::table_files(directory) {
        let path = directory.join(path);
        tables.push((path.clone(), read_data::<TextTable>(&path)?));
    }
    Ok(tables)
}

/// Writes the CSV file of all text tables: its path. The fallback language is the
/// first column, the others follow by name.
pub fn export(directory: &Path, fallback: &str) -> Result<PathBuf, String> {
    let mut tables = read_tables(directory)?;
    // Stable: the tables of one language keep the order of their paths.
    tables.sort_by(|a, b| {
        let rank = |t: &TextTable| (t.language != fallback, t.language.clone());
        rank(&a.1).cmp(&rank(&b.1))
    });
    // A table with no language has no column: the check reports it.
    tables.retain(|(_, table)| !table.language.is_empty());
    let mut languages: Vec<&str> = Vec::new();
    for (_, table) in &tables {
        if !languages.contains(&table.language.as_str()) {
            languages.push(&table.language);
        }
    }
    let mut keys: Vec<&str> = Vec::new();
    let mut seen = HashSet::new();
    for entry in tables.iter().flat_map(|(_, table)| &table.texts) {
        if seen.insert(entry.key.as_str()) {
            keys.push(&entry.key);
        }
    }
    let mut header = vec!["key".to_owned()];
    header.extend(languages.iter().map(|language| (*language).to_owned()));
    let mut rows = vec![header];
    for key in keys {
        let mut row = vec![key.to_owned()];
        for language in &languages {
            let found = tables
                .iter()
                .filter(|(_, table)| table.language == *language)
                .flat_map(|(_, table)| &table.texts)
                .find(|entry| entry.key == key);
            row.push(found.map(|e| e.text.clone()).unwrap_or_default());
        }
        rows.push(row);
    }
    let path = directory.join(CSV_PATH);
    std::fs::create_dir_all(path.parent().unwrap_or(directory))
        .map_err(|error| format!("{} not made: {error}", path.display()))?;
    std::fs::write(&path, format!("{BOM}{}", write_csv(&rows)))
        .map_err(|error| format!("{} not written: {error}", path.display()))?;
    Ok(path)
}

/// Reads the CSV file into the tables: the count of texts that changed. A filled
/// cell that differs from the table changes it; an empty cell changes nothing; no key
/// is removed. Only a table that changed is written.
pub fn import(directory: &Path) -> Result<usize, String> {
    let path = directory.join(CSV_PATH);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("{} not read: {error}", path.display()))?;
    let rows = read_csv(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    let (header, rows) = rows.split_first().ok_or("The CSV file is empty.")?;
    if header.first().map(String::as_str) != Some("key") {
        return Err("The first column of the CSV file must be \"key\".".to_owned());
    }
    // A language name is a file name: no path in it.
    let valid = |l: &String| {
        !l.is_empty()
            && l.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    };
    if let Some(bad) = header[1..].iter().find(|l| !valid(l)) {
        return Err(format!("\"{bad}\" is not a language code."));
    }

    let mut tables = read_tables(directory)?;
    let mut changed = HashSet::new();
    let mut count = 0;
    // A row with no key is no text of a key.
    for row in rows.iter().filter(|row| !row[0].trim().is_empty()) {
        let key = &row[0];
        for (column, language) in header.iter().enumerate().skip(1) {
            let cell = row.get(column).map_or("", String::as_str);
            if cell.is_empty() {
                continue;
            }
            // The table of the language that has the key, else the first of the language.
            let index = tables
                .iter()
                .position(|(_, t)| t.language == *language && t.texts.iter().any(|e| e.key == *key))
                .or_else(|| tables.iter().position(|(_, t)| t.language == *language));
            let index = match index {
                Some(index) => index,
                None => {
                    let path = maki_events::table_path(directory, language);
                    // A file of that name is a table of another language: not written over.
                    if path.exists() {
                        return Err(format!(
                            "{} is a table of another language than \"{language}\".",
                            path.display()
                        ));
                    }
                    tables.push((path, TextTable::new(language)));
                    tables.len() - 1
                }
            };
            let table = &mut tables[index].1;
            match table.texts.iter_mut().find(|e| e.key == *key) {
                Some(entry) if entry.text == cell => continue,
                Some(entry) => entry.text = cell.to_owned(),
                None => table.texts.push(TextEntry {
                    key: key.clone(),
                    text: cell.to_owned(),
                }),
            }
            changed.insert(index);
            count += 1;
        }
    }
    for index in changed {
        let (path, table) = &tables[index];
        std::fs::create_dir_all(path.parent().unwrap_or(directory))
            .and_then(|()| std::fs::write(path, data_text(table)))
            .map_err(|error| format!("{} not written: {error}", path.display()))?;
    }
    Ok(count)
}

fn export_texts(world: &mut World) {
    let directory = world.resource::<Editor>().document.directory();
    let fallback = world
        .get_resource::<Texts>()
        .map_or("en".to_owned(), |texts| texts.fallback().to_owned());
    let result = export(&directory, &fallback);
    let mut editor = world.resource_mut::<Editor>();
    match result {
        Ok(path) => editor.status = format!("{} written.", path.display()),
        Err(error) => editor.error = Some(error),
    }
}

fn import_texts(world: &mut World) {
    let directory = world.resource::<Editor>().document.directory();
    let result = import(&directory);
    let mut editor = world.resource_mut::<Editor>();
    match result {
        Ok(count) => editor.status = format!("{count} texts changed."),
        Err(error) => editor.error = Some(error),
    }
}

/// The commands "Export texts" and "Import texts" of the Assets menu.
pub(crate) fn add_commands(app: &mut maki_core::App) {
    let commands: [(_, _, fn(&mut World)); 2] = [
        ("assets.export_texts", "Export texts", export_texts),
        ("assets.import_texts", "Import texts", import_texts),
    ];
    for (id, name, run) in commands {
        maki_ui::add_command(
            app,
            ToolCommand {
                menu: "Assets",
                can_run: |world| !crate::playing(world),
                ..ToolCommand::new(id, name, run)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    fn table(language: &str, texts: &[(&str, &str)]) -> String {
        let texts = texts
            .iter()
            .map(|(key, text)| TextEntry {
                key: (*key).into(),
                text: (*text).into(),
            })
            .collect();
        data_text(&TextTable {
            language: language.into(),
            texts,
        })
    }

    /// A fresh room directory with the tables en and ja.
    fn project(name: &str, en: &[(&str, &str)], ja: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maki-csv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data/text")).unwrap();
        std::fs::write(dir.join("data/text/en.text.ron"), table("en", en)).unwrap();
        std::fs::write(dir.join("data/text/ja.text.ron"), table("ja", ja)).unwrap();
        dir
    }

    fn tables_of(dir: &Path) -> Vec<TextTable> {
        let tables = read_tables(dir).unwrap();
        tables.into_iter().map(|(_, t)| t).collect()
    }

    fn time(dir: &Path, file: &str) -> SystemTime {
        let data = std::fs::metadata(dir.join(file)).unwrap();
        data.modified().unwrap()
    }

    #[test]
    fn an_export_and_an_import_with_no_change_write_no_table() {
        let dir = project("same", &[("a", "A"), ("b", "B")], &[("a", "ja-a")]);
        let times = |dir: &Path| {
            (
                time(dir, "data/text/en.text.ron"),
                time(dir, "data/text/ja.text.ron"),
            )
        };
        let before = times(&dir);
        export(&dir, "en").unwrap();
        assert_eq!(import(&dir), Ok(0));
        assert_eq!(before, times(&dir));
        let csv = std::fs::read_to_string(dir.join(CSV_PATH)).unwrap();
        assert!(csv.starts_with("\u{feff}key,en,ja\r\na,A,ja-a\r\nb,B,\r\n"));
    }

    #[test]
    fn a_text_with_comma_quote_and_line_end_goes_out_and_back() {
        let tricky = "Hi, \"you\"\nline two\r\nend";
        let dir = project("tricky", &[("a", tricky)], &[]);
        export(&dir, "en").unwrap();
        // The table changes on the disk, then the CSV brings the text back.
        std::fs::write(
            dir.join("data/text/en.text.ron"),
            table("en", &[("a", "x")]),
        )
        .unwrap();
        assert_eq!(import(&dir), Ok(1));
        assert_eq!(tables_of(&dir)[0].texts[0].text, tricky);
    }

    #[test]
    fn an_empty_cell_changes_nothing_and_no_key_is_removed() {
        let dir = project("empty", &[("a", "A"), ("b", "B")], &[("a", "ja-a")]);
        std::fs::write(dir.join(CSV_PATH), "key,en,ja\na,,\n").unwrap();
        assert_eq!(import(&dir), Ok(0));
        let tables = tables_of(&dir);
        assert_eq!(tables[0].texts.len(), 2);
        assert_eq!(tables[1].texts[0].text, "ja-a");
    }

    #[test]
    fn a_new_key_and_a_new_language_go_into_the_tables() {
        let dir = project("new", &[("a", "A")], &[]);
        std::fs::write(dir.join(CSV_PATH), "key,en,ja,fr\na,A,,Ah\nb,B,ja-b,\n").unwrap();
        assert_eq!(import(&dir), Ok(3));
        let tables = tables_of(&dir);
        let find = |lang: &str| tables.iter().find(|t| t.language == lang).unwrap();
        let en: Vec<_> = find("en").texts.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(en, ["a", "b"]);
        let ja = &find("ja").texts;
        assert_eq!((ja[0].key.as_str(), ja[0].text.as_str()), ("b", "ja-b"));
        assert_eq!(find("fr").texts[0].text, "Ah");
        assert!(dir.join("data/text/fr.text.ron").exists());
    }

    #[test]
    fn a_row_with_no_key_is_skipped_and_a_file_of_another_language_stays() {
        let dir = project("other", &[("a", "A")], &[]);
        std::fs::write(dir.join(CSV_PATH), "key,en\n,lost\na,A\n").unwrap();
        assert_eq!(import(&dir), Ok(0));
        // `fr.text.ron` holds the language `de`: an import of `fr` does not write over it.
        let other = TextTable::new("de");
        std::fs::write(dir.join("data/text/fr.text.ron"), data_text(&other)).unwrap();
        std::fs::write(dir.join(CSV_PATH), "key,fr\na,Ah\n").unwrap();
        assert!(import(&dir).unwrap_err().contains("another language"));
    }

    #[test]
    fn a_file_with_bom_and_crlf_reads() {
        let rows = read_csv("\u{feff}key,en\r\na,\"x\r\ny\"\r\nb,B").unwrap();
        let want = [vec!["key", "en"], vec!["a", "x\r\ny"], vec!["b", "B"]];
        assert_eq!(rows, want);
    }

    #[test]
    fn a_broken_quote_names_its_line() {
        let error = read_csv("key,en\na,B\nb,\"open\nmore\n").unwrap_err();
        assert!(error.starts_with("line 3:"), "{error}");
        let error = read_csv("key,en\na,\"x\"y\n").unwrap_err();
        assert!(error.starts_with("line 2:"), "{error}");
    }
}
