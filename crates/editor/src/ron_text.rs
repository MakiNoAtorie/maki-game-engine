//! The text of a room file as the editor writes it, and the comments of a room file.
//!
//! The `ron` crate writes a value on one line, or with each field on a line of its
//! own, and it does not keep comments. A room file is written by people too: one item
//! on one line where it fits, with notes on the design as `//` comments. So this
//! module has two parts:
//!
//! - [`pretty`] gets the one-line text of `ron` and puts line breaks into it.
//! - [`Notes`] holds the comments of a file. [`read_notes`] takes them from a text,
//!   and [`pretty`] writes them back.
//!
//! Where a comment belongs: the room file is one struct, and most of its fields are
//! lists (`props: [...]`). A comment before an element of such a list, or in it,
//! belongs to that element. Each other comment belongs to the room and is written at
//! the top of the file.

use std::collections::BTreeMap;
use std::iter::Peekable;
use std::str::Chars;

/// A line is not longer than this, if a break is possible.
const WIDTH: usize = 110;
const INDENT: usize = 4;

/// The comments of a room file, as lines of text without the `//`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Notes {
    /// Of the room: written before the first line of the file.
    pub room: Vec<String>,
    /// Of an element of a list of the room: the name of the field (`props`) and the
    /// index of the element. Written before the element.
    pub items: BTreeMap<(String, usize), Vec<String>>,
}

impl Notes {
    /// The notes of an element. Empty if it has none.
    pub fn of(&self, list: &str, index: usize) -> &[String] {
        let key = (list.to_owned(), index);
        self.items.get(&key).map_or(&[], Vec::as_slice)
    }

    /// Sets the notes of an element. No lines: the element has no notes.
    pub fn set(&mut self, list: &str, index: usize, lines: Vec<String>) {
        let key = (list.to_owned(), index);
        if lines.is_empty() {
            self.items.remove(&key);
        } else {
            self.items.insert(key, lines);
        }
    }

    /// An element was removed from a list (`by` is -1) or put into it (`by` is 1) at
    /// `index`: the notes of the elements behind it move with them. The notes of a
    /// removed element are dropped.
    pub fn shift(&mut self, list: &str, index: usize, by: isize) {
        let items = std::mem::take(&mut self.items);
        for ((name, at), lines) in items {
            let at = if name != list || at < index {
                Some(at)
            } else if by < 0 && at == index {
                None
            } else {
                at.checked_add_signed(by)
            };
            if let Some(at) = at {
                self.items.insert((name, at), lines);
            }
        }
    }
}

/// The comments in the text of a room file.
pub fn read_notes(text: &str) -> Notes {
    let mut notes = Notes::default();
    // Comments between the elements of a list: they wait for the next element.
    let mut pending = Vec::new();
    // Open brackets. 1: in the room struct. 2: in a list of it. 3 and more: in an
    // element of that list.
    let mut depth = 0usize;
    // The field of the room that the scan is in, and its elements up to here.
    let (mut field, mut elements) = (String::new(), 0usize);
    // The last word in the room struct: the name of a field, when a `:` follows.
    let mut word = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let comment = match (c, chars.peek().copied()) {
            ('"', _) => {
                skip_string(&mut chars, &mut String::new());
                continue;
            }
            ('/', Some('/')) => {
                chars.next();
                let line: String = chars.by_ref().take_while(|c| *c != '\n').collect();
                vec![clean(&line)]
            }
            ('/', Some('*')) => {
                chars.next();
                let mut block = String::new();
                while let Some(c) = chars.next() {
                    if c == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                    block.push(c);
                }
                block.trim().lines().map(clean).collect()
            }
            ('(' | '[' | '{', _) => {
                if depth == 2 {
                    let key = (field.clone(), elements);
                    if !pending.is_empty() {
                        notes.items.entry(key).or_default().append(&mut pending);
                    }
                    elements += 1;
                }
                depth += 1;
                continue;
            }
            (')' | ']' | '}', _) => {
                depth = depth.saturating_sub(1);
                // After the last element of a list: no element to belong to.
                if depth == 1 {
                    notes.room.append(&mut pending);
                }
                continue;
            }
            (':', _) if depth == 1 => {
                field = std::mem::take(&mut word);
                elements = 0;
                continue;
            }
            _ => {
                if depth == 1 && (c.is_alphanumeric() || c == '_') {
                    word.push(c);
                } else {
                    word.clear();
                }
                continue;
            }
        };
        match depth {
            2 => pending.extend(comment),
            3.. => {
                let key = (field.clone(), elements - 1);
                notes.items.entry(key).or_default().extend(comment);
            }
            _ => notes.room.extend(comment),
        }
    }
    notes.room.append(&mut pending);
    notes
}

/// A comment line without the space after `//` and the white space at its end.
fn clean(line: &str) -> String {
    let line = line.trim_end();
    line.strip_prefix(' ').unwrap_or(line).to_owned()
}

/// Moves the rest of a string literal, with its closing quote, from `chars` to `out`.
/// The opening quote is taken already.
fn skip_string(chars: &mut Peekable<Chars>, out: &mut String) {
    while let Some(c) = chars.next() {
        out.push(c);
        match c {
            '\\' => out.extend(chars.next()),
            '"' => return,
            _ => {}
        }
    }
}

/// A part of a RON text.
enum Node {
    /// Text with no bracket in it: `width: 8.0`.
    Leaf(String),
    /// A bracket pair with the text before it (`pose: (`, `mesh: Sphere(`) and the
    /// parts in it, which commas divide.
    Group {
        head: String,
        open: char,
        items: Vec<Node>,
    },
}

/// The parts up to the closing bracket, which is taken, or up to the end of the text.
fn parse_items(chars: &mut Peekable<Chars>) -> Vec<Node> {
    let mut items = Vec::new();
    loop {
        match chars.peek() {
            None => break,
            Some(')' | ']' | '}') => {
                chars.next();
                break;
            }
            Some(c) if *c == ',' || c.is_whitespace() => {
                chars.next();
            }
            Some(_) => items.push(parse_item(chars)),
        }
    }
    items
}

fn parse_item(chars: &mut Peekable<Chars>) -> Node {
    let mut head = String::new();
    loop {
        match chars.peek().copied() {
            None | Some(',' | ')' | ']' | '}') => return Node::Leaf(head),
            Some(open @ ('(' | '[' | '{')) => {
                chars.next();
                let items = parse_items(chars);
                return Node::Group { head, open, items };
            }
            Some(c) => {
                chars.next();
                match c {
                    '"' => {
                        head.push(c);
                        skip_string(chars, &mut head);
                    }
                    ':' => head.push_str(": "),
                    c if c.is_whitespace() => {}
                    c => head.push(c),
                }
            }
        }
    }
}

fn close(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

/// The node on one line.
fn flat(node: &Node, out: &mut String) {
    match node {
        Node::Leaf(text) => out.push_str(text),
        Node::Group { head, open, items } => {
            out.push_str(head);
            out.push(*open);
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                flat(item, out);
            }
            out.push(close(*open));
        }
    }
}

/// Writes a node at a depth of brackets. On one line if it fits; else each of its
/// parts on a line of its own.
fn write(node: &Node, depth: usize, notes: &Notes, out: &mut String) {
    let mut line = String::new();
    flat(node, &mut line);
    let Node::Group { head, open, items } = node else {
        out.push_str(&line);
        return;
    };
    // A field of the room that is a list of items: one item on each line, as a
    // person writes it, also when the list is short.
    let list = depth == 1 && items.iter().any(|item| matches!(item, Node::Group { .. }));
    // One more for the comma after it.
    let fits = depth * INDENT + line.len() < WIDTH;
    if depth > 0 && fits && !list || items.is_empty() {
        out.push_str(&line);
        return;
    }
    let field = head.trim_end_matches(": ");
    let indent = " ".repeat((depth + 1) * INDENT);
    out.push_str(head);
    out.push(*open);
    out.push('\n');
    for (index, item) in items.iter().enumerate() {
        if depth == 1 {
            comment(notes.of(field, index), &indent, out);
        }
        out.push_str(&indent);
        write(item, depth + 1, notes, out);
        out.push_str(",\n");
    }
    out.push_str(&" ".repeat(depth * INDENT));
    out.push(close(*open));
}

fn comment(lines: &[String], indent: &str, out: &mut String) {
    for line in lines {
        out.push_str(indent);
        out.push_str(format!("// {line}").trim_end());
        out.push('\n');
    }
}

/// The text of a room file. `compact` is the room as `ron` writes it on one line.
pub fn pretty(compact: &str, notes: &Notes) -> String {
    let mut out = String::new();
    comment(&notes.room, "", &mut out);
    let room = parse_item(&mut compact.chars().peekable());
    write(&room, 0, notes, &mut out);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"// A room.
// Two lines.
(
    width: 8.0,
    doorways: [
        // The way out: (not a bracket.
        (wall: NegZ, offset: 3.0),
    ],
    props: [
        (mesh: Cube(1.0), pose: (position: (-1.0, 0.5, 0.0))),
        // A long one.
        (
            name: "a \"ball\" // no comment",
            mesh: Sphere(radius: 0.6, segments: 96, rings: 48),
            pose: (position: (1.6, 0.6, 1.9), rotation: (0.0, 45.0, 0.0), scale: (2.0, 2.0, 2.0)),
        ),
    ],
    spawns: {
        "start": (-0.5, 0.0, 2.6),
    },
    events: [
        // The first event.
        (name: "x", pages: [(trigger: Touch, commands: [Wait(0.5)])]),
    ],
)
"#;

    /// The text as `ron` writes it: no comments, no white space but in strings.
    fn compact(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => {
                    out.push(c);
                    skip_string(&mut chars, &mut out);
                }
                '/' => while chars.next().is_some_and(|c| c != '\n') {},
                c if c.is_whitespace() => {}
                c => out.push(c),
            }
        }
        out.replace(",)", ")").replace(",]", "]").replace(",}", "}")
    }

    #[test]
    fn a_file_in_the_style_of_the_writer_is_written_as_it_was() {
        let notes = read_notes(FILE);
        assert_eq!(notes.room, ["A room.", "Two lines."]);
        assert_eq!(notes.of("doorways", 0), ["The way out: (not a bracket."]);
        assert_eq!(notes.of("props", 1), ["A long one."]);
        assert_eq!(notes.of("events", 0), ["The first event."]);
        assert_eq!(notes.items.len(), 3);
        assert_eq!(pretty(&compact(FILE), &notes), FILE);
    }

    #[test]
    fn a_comment_at_another_place_goes_to_its_item_or_to_the_room() {
        let text = "( // at the room\n props: [ (mesh: /* in a prop */ Cube(1.0)),\n // after the last\n ],\n lights: [\n // one\n // two\n (radius: 1)] )\n// end";
        let notes = read_notes(text);
        assert_eq!(notes.room, ["at the room", "after the last", "end"]);
        assert_eq!(notes.of("props", 0), ["in a prop"]);
        assert_eq!(notes.of("lights", 0), ["one", "two"]);
        // What was written is read as the same notes.
        let written = pretty("(props:[(mesh:Cube(1.0))],lights:[(radius:1)])", &notes);
        assert_eq!(read_notes(&written), notes);
    }

    #[test]
    fn notes_move_with_their_items() {
        let mut notes = Notes::default();
        for index in 0..3 {
            notes.set("props", index, vec![index.to_string()]);
        }
        notes.set("lights", 1, vec!["light".to_owned()]);
        // The second prop is removed.
        notes.shift("props", 1, -1);
        assert_eq!(notes.of("props", 0), ["0"]);
        assert_eq!(notes.of("props", 1), ["2"]);
        assert!(notes.of("props", 2).is_empty());
        // A new first prop.
        notes.shift("props", 0, 1);
        assert_eq!(notes.of("props", 1), ["0"]);
        assert_eq!(notes.of("lights", 1), ["light"]);
    }
}
