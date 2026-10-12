# Stage 33: choices and the text box

Plan: [README.md](README.md). Report: `docs/part5/stage-33.md`. Item: M6 of [`../part4/MISSING.md`](../part4/MISSING.md).

Goal: an event asks the player a question, and the text of the game shows in a box of the game, not in a debug window.
Gate: a question with two answers, made in the editor; each answer runs its list.

Written after the report of stage 32, from the code at `37c730a`.

## Context in the code

### Lists in a command

- A command is a value with a hidden type (`maki_events::Command`). `Builtin::If(conditions, then, else)` holds two lists (`crates/events/src/builtin.rs`). Its handler gives one of them to `Step::open` (`crates/events/src/command.rs`), and the interpreter runs it (`RunningEvent::step`, `crates/events/src/interpreter.rs`).
- Each place that walks or changes all commands matches `Builtin::If`:
  - `each_command` (`crates/events/src/check.rs:78`): the checks of the loader and the names of the editor.
  - `list`, `edit_list`, `rows`, `Row::insert_place` (`crates/editor/src/events.rs`): a `ListPath` is `Vec<(usize, bool)>`, the bool is "the second list"; `RowKind::Else` is the line between the two lists.
  - `rename_in_events` and `spawn_users` (`crates/editor/src/ops.rs`).
  - `arguments` and `new_command` of the event panel (`crates/editor/src/event_panel.rs`).
- A module gives its commands with `CommandRegistry::add`, a check with `add_check`, and texts with `add_texts` (stages 29 and 32). A list in a command of a module is not walked and not shown as rows (`OPEN_ITEMS.md`, "Limits of the event format").

### The message

- `MessageBox(Option<String>)` (`interpreter.rs`) is the text that the game shows. `Builtin::Message` sets it and waits for `INTERACT`.
- `EventsUiModule` (`crates/ui/src/events.rs`) shows it in an egui window with a title bar (`add_window`), open while there is a message.
- `add_overlay` draws on the picture with no window around it; `maki_platform::Picture` is the place of the picture.
- `ResetPlay` (stage 30) sets `MessageBox` back.
- Input: `INTERACT` is an action of the events crate with its keys (`crates/events/src/lib.rs`). The events crate is before `gameplay` in the crate order: it cannot use the move actions.

## Prompt

```text
Stage 33: choices and the text box.
Read docs/part5/stage-33-choices-and-text-box.md and item 6 of
docs/part4/MISSING.md first.

Lists in a command
- A command set tells which lists its commands hold, with a label for
  each, and makes a command again with other lists
  (CommandRegistry::add_lists). The built-in If uses it as a module does.
- The checks, the names, the rows of the event panel, a rename, and the
  search for the users of a spawn point walk the lists by the registry.
  No code outside crates/events/src/builtin.rs matches Builtin::If to
  find its lists.
- A list path names a list by its index, not by "first or second".

Choice
- Choice(question, [(answer, commands), ...]): the game shows the
  question and the answers, the player moves the mark and presses
  interact, and the list of that answer runs.
- The keys are actions of the events module. The mark does not wrap.
- A Choice with no answer is an error of the loader.
- The event panel: the question and the answers as arguments (add,
  remove, text), and the lists as rows with the answer as their label.

Text box
- The message and the answers show in a box at the bottom of the
  picture, drawn by the UI module: no title bar, readable text, the
  mark at the selected answer, the key that goes on.
- MessageBox stays the interface of a message.
- The play signal clears a question.

Out of scope: a portrait, text that types itself, more than one box,
a graph view.

Done when: a test runs a Choice with keys and each answer runs its list;
a test makes the Choice in the editor (the command, an answer, a command
in a list) and the saved text reads as the same events; trip_720p is
byte-equal. Update OPEN_ITEMS.md ("Message window", "Dialogue with
choices", "Limits of the event format"). Report.
```

## What to watch for

- **One walk.** `rg "Builtin::If" crates/editor crates/events/src/check.rs`: only the argument widget and the "Add" entry of the event panel are left.
- **The line of a command in a list of a module.** The order of a command comes from the read (stage 29), so the line is right also for a list that the walk does not know. Check with a `Choice` that has a wrong name in its second answer.
- **The keys of the mark.** Up and down are also the walk keys of the game. The player has no control while an event runs: the two do not fight. Try a `Choice` in a `Parallel` event: the player walks and the mark moves.
- **A press that does two things.** The interact press that closes a message must not also pick the first answer of a `Choice` that comes after it.
- **The box and the picture size.** In the editor the picture is not 16:9: the box has the width of the picture, with a limit.
- **Row kinds.** A command with three answers has three label rows; `End` closes it. A new command goes into the list of the label row that is selected.
