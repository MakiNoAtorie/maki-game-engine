# UX pass: changes

The changes of the UX pass, by fixer (Opus, medium effort; one writer per file). Each section has the changes, the findings that the fixer skipped and why, and the tests it reported.

Sources: `FINDINGS.md` (the lead's pass), `DETAILS.md` (the file reviews), `BUGS.md` (bugs, fixed in a separate pass).

## Event panel

- `crates/editor/src/routine_panel.rs`: New routine now uses `maki_ui::widgets::name_row` with the hint "Routine name": Enter submits, and the button stays disabled while the name is empty. Open routine marks the open file as selected. The empty list says "No routines yet. Use New routine." The disabled menu shows "Stop the game to open or create a routine." on hover.
- `crates/editor/src/routine_panel.rs`: The name error now matches the rule: "Use letters, numbers, spaces, `_`, and `-` …". "Exists already" names the routine, not the full path. The success status is `Routine "<name>" created.` and no longer prints the absolute path.
- `crates/editor/src/routine_panel.rs`: The panel header puts Close first, then the file name, with the relative path as a tooltip. There is now an "Arguments" heading with a weak hint. Remove is a ✖ icon with the tooltip "Remove argument", and "Add an argument" is now "Add argument". `open_routine` clears `self.menu`, which covers BUGS #29.
- `crates/editor/src/menu_panel.rs`: New menu now uses `name_row` with the hint "Menu name". Open menu marks the open file as selected. The empty list text and disabled hover match the routine menu. The name error and "exists already" texts changed the same way. The success status is `Menu "<name>" created.`
- `crates/editor/src/menu_panel.rs`: The panel header puts Close first, with the file name and the path as a tooltip. The row buttons are now "Add row", "Remove" (with a tooltip about conditions and commands), "Move up" and "Move down". The items row label is now "items: (the items of the player)", so each row kind is easier to tell apart. The Items hint now names the item kind's "use_routine" field in plain words. The empty state says "No rows. Use Add row." I updated the row_label test to match.
- `crates/editor/src/text_panel.rs`: The search hint is "Search keys and texts". Escape clears the search while the field has focus, and an ✖ button clears it too. A "N of M keys" count shows while a search is active.
- `crates/editor/src/text_panel.rs`: "New key" and "New language" are now "Add key" and "Add language", both through `name_row`. Add key is disabled while there is no language. Each add sets a status: `Key "x" added.` or `Language "x" added.`
- `crates/editor/src/text_panel.rs`: The empty states are clearer: no languages, "No keys yet. Use Add key.", and "No results." when a search matches nothing. Each language header shows a red "N missing" count. The header is "Key", and the code error text is reworded.
- `crates/editor/src/text_csv.rs`: A missing CSV now gives "No file data/text/texts.csv. Use Assets > Export texts to make one." The export status is "Texts exported to data/text/texts.csv." The import status is "N texts imported from data/text/texts.csv (empty cells skipped)."
- `crates/editor/src/timeline_panel.rs`: "Key" is now "Add key". Its tooltip says a key at the same time gets the new value, and a disabled hover says to select a track first. "Delete key" has a tooltip. A new "Delete track" button removes the selected track.
- `crates/editor/src/timeline_panel.rs`: The play head time is now an editable DragValue in seconds, clamped, and a change previews the timeline. The play head is clamped to the span, which covers BUGS #44. The warning note is reworded and clears when the selected track changes.
- `crates/editor/src/timeline_panel.rs`: The two pickers have "Object" and "Channel" labels, and "Add track" has a tooltip. The channel resets to the first channel of a newly picked object, which covers BUGS #41.
- `crates/editor/src/timeline_panel.rs`: An empty timeline shows "No tracks. Pick an object and a channel, then press Add track." Track names are clipped to the names column. The rows are in a vertical ScrollArea. The no-timeline text points to the Inspector. An invalid timeline shows "The timeline component is invalid: …" with a hint to fix it in the Inspector or undo. I updated the test to click "Add key".

Skipped:
- Delete and Escape keys on the timeline: the editor's global DELETE and CANCEL actions also fire on these keys, so pressing Delete would also delete the selected object (the timeline's own object). This needs a focus claim like the event panel's `on_rows`, in `lib.rs`, which is not my file.
- Closing a name menu only on success: `name_row` (ui, not my file) always closes the menu and clears the name on submit.
- Confirm or undo on Remove for menu rows and routine arguments, and committing an argument rename on Enter or focus loss: each needs undo state or a dialog, which is large for these panels. Duplicate argument names are BUGS #30.
- Escape to close an open menu or routine file: it clashes with the global CANCEL action.
- Right-click Rename / Delete / Show in folder on routine files: large for the gain.
- Text panel: write on focus loss with undo, rename or delete of keys and languages, "Copy from fallback", "Show missing only", a sticky header, and scroll and focus to a new key. Each is a larger feature. The row hiding during a search is BUGS #36.
- text_csv: confirmation or preview before export and import, a backup, and a warning for new languages. There is no dialog or confirm seam in `ToolCommand`, and that is a larger change. The header trim is BUGS #34. Command tooltips are not possible because `ToolCommand` has no hover field (ui, not my file).
- Timeline: Play/Stop and previous/next key buttons, and an "Add timeline to selected object" button (adding a component goes through other files). The span, scrub, key-merge and drag-origin items are BUGS #39, #40, #42 and #43.
- A kind prefix for Commands rows: the bare key keeps rows short, and the Text and Items rows now carry their kind.

Tests failing outside my files:
- `event_panel::tests::the_state_component_is_drawn_from_its_field_table` (event_panel.rs:1861)
- `event_panel::tests::each_condition_form_is_drawn_from_its_field_table` (event_panel.rs:1828)
- `ops::tests::a_doorway_and_its_door_are_one_object` (ops.rs:978)

All tests in my files pass. Clippy on maki-editor is clean.

## Document and data

- `crates/editor/src/event_panel.rs`: name lists (`described_list`) clear their search when the popup closes, so a stale filter no longer shows the next time the list opens (DETAILS :304).
- `crates/editor/src/event_panel.rs`: in a list with New..., a typed name that is not in the list shows as the first entry `New: <text>`. Enter with no match creates that name directly (:316).
- `crates/editor/src/event_panel.rs`: a picker with an empty value shows a red italic "(none)". An empty list shows "Nothing to pick: this room has none." A search with no match shows "No results" (:404).
- `crates/editor/src/event_panel.rs`: a key pick whose text is empty now shows a warning: "Empty text: write it in the Texts panel." (:443, the warning part only)
- `crates/editor/src/event_panel.rs`: each page tab has a tooltip with its number, trigger and condition count, for example "2: Touch, 1 condition". The star is explained once by a weak "* active" legend next to the tabs, not on every tab (:1086).
- `crates/editor/src/event_panel.rs`: the page buttons are now "Add page", "Duplicate page", "Delete page", "Move earlier" and "Move later", each with a tooltip. They stay visible while a wire is shown, disabled with the tooltip "Select a page first" (:1104).
- `crates/editor/src/event_panel.rs`: "Add a wire" is now "Add wire". It is disabled when the object has no outputs, with the tooltip "This object has no outputs" (:1208). "Delete the wire" is now "Delete wire".
- `crates/editor/src/event_panel.rs`: deleting a page or a wire that has commands shows a notice, "Deleted page N with K commands. Undo brings it back." The document's undo history already covers it, so I added no confirmation dialog (:1119). This uses a new private field `EventUi::notice`, which `event_panel` passes to `maki_ui::notify`.
- `crates/editor/src/event_panel.rs`: Add, Paste and Duplicate now use one rule for where the new command goes, the new fn `new_place`. It goes after the selected rows, inside a branch only when an Else or answer row is selected, and at the end of the page when nothing is selected (:1270).
- `crates/editor/src/event_panel.rs`: there is a drop zone with an insertion line below the last row, so a drag can move a command to the end of the page (:1326).
- `crates/editor/src/event_panel.rs`: new "Move up" and "Move down" buttons, and Alt+Up / Alt+Down while the command rows have focus. These are the new `RowCommand::MoveUp` / `MoveDown` variants and the fn `move_rows`, and they work for a Shift-selected range (:1326).
- `crates/editor/src/event_panel.rs`: the add-command combo is now labelled "Add command...". The button row wraps (`horizontal_wrapped`). The tooltips of Delete, Duplicate, Copy and Paste now read "<Action> the selected commands  <key>" (:1280 label, :1310).
- `crates/editor/src/event_panel.rs`: Copy and Paste show the notices "Copied N commands" and "Pasted N commands". This also works from the keyboard shortcuts, because the notice is set in `row_command` (:1310).
- `crates/editor/src/event_panel.rs`: the empty command list now says: No commands. Add one with "Add command...".

Skipped:
- :654, placeholder keys of new Choice and Answer: making keys in the text table on Add would write to disk before save or undo. That is the BUGS #11 area. The red pick correctly shows that a key is still needed, and with the new `New: <text>` entry the user can type a key fast.
- :443, a text field for the key's text under the pick: this would be a second editor of the text table beside the Texts panel. Large for the gain; the warning points the user to the Texts panel.
- :443 / BUGS #11, the `add_key` error shown only in the log: not fixed. It is a later pass.
- :1119, a confirmation or notice when an answer is removed: `arguments()` has no `EventUi`, and the remove button's tooltip already says "with its commands". Undo restores it.
- :1280, grouping the Add command list by module: the registry has no module of a command. It would need an API change in `maki-events`, which is outside my file.
- :1326, a drop at the end of a branch: dropping on an End or Else row still keeps its old meaning. Else-if nesting makes "end of the branch" ambiguous. Only the end of the page got a drop zone.
- :384, keeping the new-name field until Enter or Escape: the field takes focus back every frame, so keeping it after a click elsewhere would steal focus. An Enter on the name that is already set already keeps it.

Tests failing outside my files:
- `event_panel::tests::the_state_component_is_drawn_from_its_field_table` (crates/editor/src/event_panel.rs:1861) fails. It expects an "x" remove button from `crate::fields::show`, but the fields drawing, edited by another fixer, now draws "✖" and "▲▼" row buttons. The test is in my file but checks fields.rs output, so I did not change it: whoever owns fields.rs should update the expected text "x" to "✖". The other 94 tests pass, and `cargo clippy -p maki-editor` is clean.

## Asset panels

## What changed
- `crates/editor/src/data_panel.rs`: **New data asset** is now a submenu with a "Type" label and one radio button per type, then a `name_row` (Name / Create; Enter creates it; Create is greyed out while the name is empty). Under the row is a live preview of the file, `data/<name>.<type>.ron`. If creation fails, the typed name is kept for the retry.
- `crates/editor/src/data_panel.rs`: **Open data asset** no longer shows the `data/` prefix. When nothing is there it says "No data assets. Use New data asset." In play mode both menu items say why they are greyed out ("Stop play mode to edit data assets.").
- `crates/editor/src/data_panel.rs`: the name error now names the bad character (letters, numbers, spaces, `_` and `-` are allowed). The "exists" error tells the user to pick another name or open the file.
- `crates/editor/src/data_panel.rs`: `open_data` sets the status "Opened <file>." A failed open says "Cannot open data asset: …".
- `crates/editor/src/data_panel.rs`: the inspector header shows the file name (full path on hover), the type, and a Close button with an Esc hint. Esc closes the asset when the pointer is over the panel and no field has focus. Two weak lines were added: "Changes are saved at once." and, in play mode, "Stop play mode…".
- `crates/editor/src/ops.rs`: renaming now trims the name, and an empty name means no name. The notice reads "Name 'x' is in use: renamed to 'x_2'." I updated the test in this file to match.
- `crates/editor/src/ops.rs`: Add > Doorway now names the new object "doorway". I updated the doorway test: a door added into it keeps the name "doorway".
- `crates/editor/src/ops.rs`: Add > Door now gives one of two errors: "This room has no doorway…" or "All doorways have a door…".
- `crates/editor/src/ops.rs`: Add > Button in a room with no door or mover gets an empty wire command list. Before, it was wired to `OpenDoor(0)`, which opened nothing.
- `crates/editor/src/ops.rs`: `make_prefab` rejects a file path with `..`, a root or an empty stem. If the selection mixes prefab instances and plain objects, it now fails with a message that gives the count, instead of leaving the instances out silently.
- `crates/editor/src/document.rs`: Save now writes a temporary file and renames it over the room file, and creates the parent folder first. A crash during the write keeps the old room.
- `crates/editor/src/document.rs`: the "no module registered the type" component error now ends with "build the scripts, or remove the component".
- `crates/editor/src/events.rs`: in the list of flag and variable uses, writes now show the readable row text (`command_text`) instead of Rust Debug text.
- `crates/editor/src/widgets.rs`: in `object_pick`, text that names no object keeps the old reference, and empty text clears it. Before, any unknown text silently cleared the reference.
- `crates/editor/src/widgets.rs`: a file picker with no matching files shows a weak "No files"; hovering it says "No .ext files in the project folder."
- `crates/editor/src/fields.rs`: each list item now has a header row with its number, ▲/▼ move buttons, and a remove button (hover text "Remove item N"). An empty list shows "(empty)", and the Add button has the hover text "Add an item".
- `crates/editor/src/fields.rs`: a text-key field with no text table loaded shows "No text table loaded: the key is not checked." A Pick over an empty list shows "No <list> yet".

## Skipped
- data_panel undo, or "Revert to last opened": too large for this pass. The weak line "Changes are saved at once." states the behaviour instead.
- data_panel grouping of Open data asset by type: low value; the type is already in each file name.
- ops `:504`, a confirm before `make_prefab` overwrites a prefab file: overwriting is the intended flow (unpack the prefab, then make it again with the same name, which the tests cover). The status line in `lib.rs` already shows the written path.
- ops `:25`, a notice when an unpack writes ids into the prefab file: the function has no status channel, and its callers are not in my files.
- ops `:272`, prefer the selected doorway for Add > Door: `add()` has no selection. It would need a signature change in a public API used from `lib.rs`.
- ops `:371`, warn when a deleted object is a command target; and ops `:666`, undo/redo labels: both need changes in the callers in `lib.rs`.
- ops `:390`, offset repeated Add/Duplicate; and ops `:425`, clamp a duplicated doorway to its wall: low gain. The clamp needs the wall length for each wall.
- document `:281`, show why an `area` does not parse: needs a change in the inspector or the problems list, which are not my files.
- ids `:23`, `:78` and `:88` (correction count, parse failure, missing files): the results are dropped in `lib.rs`, so fixing them means changing the callers there.
- events `:353` (unreadable room files) and `:344` (rooms in subfolders): need new fields in `OtherRoom` and problem rows in `event_panel.rs`. Medium size, and partly outside my files.
- events `:132`, a `with_names` replace limited to object-id arguments; and events `:179`, the Else / Else-if display: both need registry or row-model changes, which are risky.
- events `:305`, readable condition text: `condition_text` is private to `maki-events`, which is not my crate.
- widgets `:153`, "missing (id 7)" in event rows: the row colouring is done in `event_panel.rs`.
- fields: the remove button is still the text "x", and the Add button is still the text "Add". The tests in `event_panel.rs`, which I do not own, look for those exact texts.

Checks: `cargo check`, `cargo clippy` and `cargo test -p maki-editor` all pass (95 tests).

Tests failing outside my files: none.

## Viewport

All six files are changed. `cargo check`, `cargo clippy --all-targets` and `cargo test -p maki-editor` pass (95 tests, one of them new), and rustfmt ran on my files only. Nothing is committed and nothing was tried in the running editor.

- `crates/editor/src/camera.rs`: renamed the bindings to Orbit view (drag), Pan view (drag), Zoom in, Zoom out, Top view, Game camera view.
- `crates/editor/src/camera.rs`: one `MIN_PITCH` limit for orbiting and for the game camera view (orbiting was 0.05, the game view 0.0). The closest zoom distance is now 0.1 m (was 0.5 m).
- `crates/editor/src/gizmo.rs`: the Scale tool has a centre square that scales all three axes. Dragging right or up enlarges; dragging one handle length doubles the size. It reuses `Handle::Floor`, so the public `Handle` enum keeps its variants. New test `a_drag_of_the_scale_square_scales_all_axes`.
- `crates/editor/src/gizmo.rs`: the Aim handle of a light is dragged on the plane at the handle's starting height, so a light can now aim up or sideways.
- `crates/editor/src/gizmo.rs`: the grab distance of a handle went from 9 px to 11 px.
- `crates/editor/src/overlay.rs`: the Scale tool now draws the centre square.
- `crates/editor/src/overlay.rs`: every light always has a small ring and its name, so a light with no mesh can be seen and clicked.
- `crates/editor/src/overlay.rs`: a doorway that does not parse shows a red "name: doorway: error" label at the object.
- `crates/editor/src/overlay.rs`: an object with no name is labelled "object N" (areas, spawn points, lights), as the pickers name it.
- `crates/editor/src/overlay.rs`: pose B of a mover is drawn dimmer and labelled "name B".
- `crates/editor/src/overlay.rs`: the game frame stays while the camera is near the game view (small orbit, pan or zoom). Its caption reads "Game view, W x H" in a readable grey.
- `crates/editor/src/overlay.rs`: play mode shows "Playing: <key> to stop" in the corner, with the key read from the bindings.
- `crates/editor/src/view.rs`: when areas overlap, a click selects the smallest one, so an inner area can be picked.
- `crates/editor/src/pictures.rs`: a file that cannot be previewed shows "?" with the tooltip "No preview: the file does not read (see the Log)", so failure no longer looks the same as loading.
- `crates/editor/src/pictures.rs`: hovering a thumbnail shows it at twice the size.
- `crates/editor/src/pictures.rs`: a thumbnail that is still loading asks for a repaint, which also fixes BUGS #28.
- `crates/editor/src/sources.rs`: new command Assets > "Convert sources again" (`assets.convert_again`), registered in `sources::start`. It clears the failed list and looks for Blender again. The failure notice now points to it and to the Log.
- `crates/editor/src/sources.rs`: if Blender is missing, the editor shows a warning at start when the asset folder has .blend or .obj files. A wrong `[editor] blender` path gets its own message.
- `crates/editor/src/sources.rs`: jobs, status and notices use the path inside the asset folder, so two files of the same name no longer clash.
- `crates/editor/src/sources.rs`: a finished conversion shows an Info notice, "X converted to .glb (2.1 s)".
- `crates/editor/src/sources.rs`: the first line of a Blender error is now its Python "Error" line, or else the last output line, not only the exit code.
- `crates/editor/src/sources.rs`: Photoshop errors name the colour mode (CMYK, Indexed, …) and say how to fix it in Photoshop.

Skipped:
- camera "focus on selection (F)": Frame (`editor.frame`) already does this. Its menu label is in lib.rs, which is not my file.
- camera, zoom toward the pointer: large for small gain, and Unity also zooms toward the centre.
- gizmo, doorway corner also changes height: changing both width and height is what a corner handle does; separate handles would be a large change.
- gizmo, rotation snap is relative: Unity and Godot also snap relative to the start.
- gizmo, scale clamps silently at 0.01: mirroring is risky; the new centre square covers the main need.
- gizmo, grey out Local/Global for Scale: that control is in panels.rs or lib.rs, not my files.
- gizmo, hover highlight of handles: it already exists in overlay.rs.
- gizmo, drag stops when the pointer ray misses the plane: low gain.
- overlay, error message for an invalid doorway in the inspector: not my file. The viewport label is done.
- view, click through a selected area and cycling through overlapping picks: clicking inside an area is how the editor selects it now; the smallest-area rule covers the common nesting case.
- view, warning for mirrors over 4096 triangles: `Mirrors::of` runs on every picture change and would log each time; low gain.
- sources, a converter overwrites a hand-made result: the rule "a result changed after the source stays" already covers the main case; the rest is risky.
- sources, "Open log" button on a notice: the notice API has no actions; the text says "See the Log" instead.
- BUGS #6, #16, #17, #18, #25, #31, #32, #33: left for the bug pass.

Notes for the lead:
- lib.rs still names the menu commands "View from the top" and "View of the game camera" (lib.rs:1181, 1187). They should become "Top view" and "Game camera view" to match the new key labels.

Tests failing outside my files: none.

## Engine and devtools

## Changes
- `crates/ui/src/commands.rs`: `ToolCommand` gets `order: i32` (its place in its menu, default 0) and `shortcut: &'static str` (key text shown when `action` is None). `ToolCommand::new` sets both, so every `..ToolCommand::new(..)` caller still compiles.
- `crates/ui/src/lib.rs`: two new public functions, `add_menu_item(app, menu, order, draw)` and `add_menu_separator(app, menu, order)`. Either can be called before `add_menu`. `add_menu(app, name, extra)` keeps its signature and behaviour: the extra goes at order 0 after a line. `Panels` now stores menu names and menu parts separately, so calling `add_menu_item` does not change the order of the menus in the menu row.
- `crates/ui/src/lib.rs`: View > Zoom in / Zoom out / Reset zoom are now commands (`ui.zoom_in`, `ui.zoom_out`, `ui.zoom_reset`, menu "View", order 100 after a line). They show Ctrl+= / Ctrl+- / Ctrl+0 and appear in the command search. Steps are 0.1, clamped to 0.5..3.0. Reset goes back to `[ui] scale`. The zoom lasts until the program ends and is not written to the config. egui's own keyboard zoom is turned off (finding 5).
- `crates/ui/src/lib.rs`: new test `a_menu_sorts_its_rows_and_keeps_lines_between_rows`.
- `crates/ui/src/shell.rs`: a menu now draws its commands and parts sorted by `order`. At an equal order the commands come first. A line is drawn only between two rows, never at the start, the end or twice (`menu_order`). `zoom_keys` reads Ctrl+= / Ctrl++ / Ctrl+- / Ctrl+0 through egui and runs the zoom commands, because the platform `Key` has no `=` or `-` and I do not own that crate.
- `crates/ui/src/shell.rs`: command search:
  - "nothing has this text" is now "No results".
  - The arrow keys skip rows that cannot run.
  - The selected row scrolls into view.
- `crates/ui/src/shell.rs`: Window menu:
  - Opening a panel there also brings its tab to the front.
  - "Reset layout" resets the layout in use, not layout 0.
- `crates/ui/src/theme.rs`:
  - Weak text (hints, placeholders, notes) is lighter: `THEME.weak` = gray 150 (finding 7).
  - Small text goes from 10.5 to 11.5.
  - The tool theme is now always dark (`set_theme(Dark)`), so the accent colours stay readable on a system in light mode.
- `crates/ui/src/render_panel.rs` (finding 9 wording, plus structure):
  - View is a combo box.
  - Short Title Case labels. The long explanations moved to hover text.
  - Resolution moved into "Display".
  - Exposure moved to "Look".
  - Display and Look are open by default.
  - When the config fixes the render size, the render scale slider shows disabled with a "this run only" button under it.
  - Lift / Gamma / Gain: the label comes first and each value has an R/G/B prefix.
  - A weak note says the changes are not saved to the config.
- `crates/ui/src/events.rs`: `game_state_panel` now shows only flags and variables. The running events moved to a private `running_events` that only the game's "Game state" panel draws. This makes the editor inspector stop listing the running events (finding 3).
- `crates/ui/src/events.rs`:
  - The name field has a hint.
  - The Add buttons are disabled while the name is empty.
  - Enter adds a flag.
  - The field is cleared after an add.
  - Empty lists say "No flags" / "No variables".
- `crates/ui/src/help.rs`:
  - The Help panel is unfolded at the start.
  - A key keeps a stable command name: the command that can run now first, otherwise the first command with that key.
  - Keys are drawn in monospace.
- `crates/ui/src/widgets.rs` (`name_row` kept):
  - `text()`: Escape cancels the edit instead of committing it.
  - `tree_filter`: clear button, Escape clears, and the hint is now "Filter". `tree_shows` trims the filter.
  - `tree_row`: an empty or whitespace rename is cancelled, and the name is trimmed.
  - `tool_button`: hover text also shows when the button is disabled ("…: not available now").
  - `color()`: R/G/B prefixes and a hover text on linear RGB.
  - `choice()`: "Unknown" instead of "?".
  - `jobs_label`: "Idle" instead of "no job".

Checks: `cargo check` (maki-ui and maki-engine), `cargo clippy -p maki-ui --all-targets` and `cargo fmt` are clean. `cargo test -p maki-ui`: 11 passed.

## For the main-panels fixer
- Choose each row's place with `order`. A menu sorts commands and parts by `order`, and at an equal order commands come first. A line only shows between two rows.
- A command: `ToolCommand { menu: "File", order: N, ..ToolCommand::new(id, name, run) }`.
- A non-command row (submenu, `name_row`): `maki_ui::add_menu_item(app, "File", N, |ui, world| { ... })`.
- A line: `maki_ui::add_menu_separator(app, "File", N)`.
- Keep `add_menu(app, "File", None)` for the menu's place in the menu row. Pass `None` and move the old extra closure into `add_menu_item` calls.
- Suggested File orders:
  - 0: New room (command)
  - 10: Open ▸ (`add_menu_item`, with the submenu inside: `ui.menu_button("Open", ..)`)
  - 20: Save (command)
  - 30: Save as (`add_menu_item` with `name_row`)
  - 40: separator
  - 50: Build the scripts, Open the project in the code editor, New script. These are set in `crates/engine/src/reload.rs` and `crates/engine/src/new_script.rs` with `order: 50`. That is another fixer's file: tell them, otherwise they stay at order 0 next to New room.
  - 90: separator
  - 100: Quit (in `crates/editor/src/lib.rs`)
- The View menu already gets the zoom commands at order 100 after a line. Put the editor's View commands at orders below 100.
- `game_state_panel` no longer draws the running events, so the inspector shows only game state. The outliner keeps its own list.

## Skipped
- commands.rs: notices for a key press or queued command that cannot run. Several commands share one action (copy, paste) and some keys are also read by systems, so a notice would come up wrongly.
- commands.rs: a `why_not` reason for each command. It needs a reason from every caller, a big change for little gain. Disabled tool buttons now show a generic "not available now" hover.
- commands.rs: ranking search results. Medium work and it changes the row order that callers and tests see.
- shell.rs:
  - Second play row when the window is narrow: layout work that I cannot test headless.
  - Save layout as / undo of a layout change: a new feature.
  - Notices for layout file errors: belongs with the bug pass.
  - The search key toggling the search closed: the text field has focus, so I am not sure the action fires.
  - The duplicate layout list in the Window menu: kept.
- widgets.rs:
  - `optional()` remembering its value: needs a stored value for each type.
  - Notice dedupe and pause on hover.
  - Reset button with a fixed width.
  - Clickable `toggle` label.
  - Progress for jobs.
  These are larger changes for small gain.
- render_panel.rs: "Reset to config". The original config values are not kept anywhere once the resource changes.
- events.rs: deleting or renaming flags, and a readable trigger and command label instead of the `{:?}` output. The events crate has no display names; that needs an API there.
- stats.rs, game_ui.rs: not part of findings 5/7/9 or the menu work; left for later.
- theme.rs: separate glyphs for SOURCE and RESET. Each glyph must be checked against the egui font tables, which I cannot do quickly.
- lib.rs:
  - Esc on the modal question and modal titles: not covered by the asked findings.
  - Wording of the scale error: cosmetic, not covered.
  - A resizable game column: not covered.

## Tests failing outside my files
None seen (only maki-ui tests were run).

## Ui shell

- `crates/devtools/src/gameplay_panel.rs`: added the note "Changes last until the game stops. They are not saved." at the top. Sections now start open (`CollapsingHeader::default_open`). Labels are now "Field of view, deg", "Follow time, s", "Wall margin, m", "Walk/Run speed, m/s". Position and target put the label first, and each value has an `x`/`y`/`z` prefix and an ` m` suffix. When no section draws, a line says to play the game or add the gameplay modules.
- `crates/devtools/src/log_panel.rs`: the level toggles show counts (new `LogStore::counts()`). The empty state says "No log lines." or "No lines match the filters.". The source field hint is now "module", with a tooltip saying it is a prefix match. Escape clears the search field. A "Reset filters" button appears when a filter is set. Clear has the tooltip "Delete all lines, also hidden ones". The level icon has a tooltip with the level. Message text is truncated, shows the full text on hover, and has a right-click menu "Copy line" / "Copy all". The repeat count `×N` comes after the text, with the tooltip "Repeated N times in a row".
- `crates/engine/src/editor_program.rs`: rewrote USAGE with one option per line, including --resave and --input-shots (fixes BUGS #1). `-h`/`--help` prints the usage and exits with 0. Any leading `-` in the project argument is an unknown option. `--input-shots` without `--input-script` is an error. Before the first build the log says "building the script crate ..., can take some minutes". The startup failure notice now says how to recover (save, or File > Build scripts, Ctrl+B). If `cargo locate-project` fails, an error line names the folder and the likely cause, and the project folder is used in place of an empty path.
- `crates/engine/src/lib.rs`: an invalid `MAKI_LOG` prints a warning on stderr that names the value and the fallback "info".
- `crates/engine/src/project.rs`: a missing project file gives the absolute path it tried, plus "Give the project folder as the first argument, or start from the folder above it".
- `crates/engine/src/loader.rs`: messages show file names, not full paths. They say "Build the scripts; if the engine changed, restart the editor", so the command has one name everywhere.
- `crates/engine/src/reload.rs`:
  - Commands are renamed "Build scripts" (new Ctrl+B binding in the editor input mode, action `project_build`) and "Open in code editor".
  - A failed build opens the Build panel only on the first failure after a good build. The notice is "Script build failed: see the Build panel."
  - A build that ends while the game runs gives the Info notice "Scripts built: they load when you stop the game."
  - A successful reload gives the Info notice "Scripts reloaded (built in X s)." in the new app.
  - A refused load says the old scripts still run, and what to do.
  - `shell()` waits for the command on a thread and logs an error naming the `[editor] code_editor` setting when the command fails.
  - Build panel: the "Build" button comes first, with a tooltip. Status texts are "Building... N s", "Build succeeded (X s).", "Build failed.". New "Copy all" button. Error rows wrap. A failed build with no rows points to the Log panel.
  - `rows()` drops cargo's summary lines ("error: could not compile", "warning: build failed"). I updated its test to match, and it now also checks that a line with no place, like a linker error, is still a row.
- `crates/engine/src/new_script.rs`:
  - Name and Template are now a labelled grid. The hint is "e.g. door bell".
  - The name field takes focus when the command opens the panel, and Enter creates.
  - The panel shows the template's first doc paragraph under the combo, and as a tooltip on each item.
  - Only a template that adds a script (`add_script::<ScriptName>`) is a component: the preview text and the "component exists" check follow that, so Event command and Engine module are no longer blocked or described as components.
  - Create shows the reason it is disabled on hover.
  - The result line clears when the name or template changes and shows a path relative to the script crate. On success it adds "add the component `x` to an object in the inspector after the build", and an Info notice "Created x.rs. Building the scripts...". An error result has a "Copy" button.
  - The "file exists" error is now "src/x.rs exists already. Choose another name.". The lib.rs fallback says where each line goes.
  - When the command reloads the templates, the template index is clamped to the list (covers BUGS #47).
- `crates/engine/templates/empty.rs`: the header says to add the component `script_name` to an object in the inspector.
- `crates/engine/templates/interactable.rs`: the `message` doc says that a key no text table has shows as it is, and the log warns.
- `editor.bat`: echoes a "Building ... can take some minutes" line. The host triple comes from `rustc -vV`, not hard-coded (checked: it gives x86_64-pc-windows-msvc).
- `game/project.toml`: the `start_spawn` comment says it is the object id, not the name, of the spawn point in `start_room`.

Skipped:
- config.toml, editor window defaults and saved layout: a large change in the platform/window code, outside my files.
- editor_program, File > Open Project and a recent-projects list: large (it needs a restart into another project). Not done.
- editor_program, "Play clears the log": the usual behavior (Unity's Clear on Play). Build errors stay in the Build panel, so the gain is small.
- loader, delete old DLL copies after each load: on Windows every earlier copy stays loaded and locked (it is never unloaded), so the delete would always fail.
- reload, CREATE_NO_WINDOW: risky for terminal code editors. The failure log from the wait thread covers the missing-editor case.
- reload, Cancel button for the build: it needs the cargo child handle across threads. Elapsed seconds were added.
- reload, "do not mark the DLL as loaded when refused" and "Retry load": changes the load logic. A new save builds and loads again.
- new_script, `maki_ui::widgets::name_row`: not used. It disables the field together with the button and clears the name on submit, so a name that is not valid could not be typed or kept. I used the same behavior by hand: Enter submits, and the button is disabled with a reason.
- log_panel, separate icons for debug and trace: a tooltip with the level is enough.
- log_panel, feedback when a file:line link fails: the open handler (`open_in_code_editor`) now logs a command failure.
- gameplay_panel, Reset and Copy values buttons: they need defaults that a panel cannot know. BUGS #2 and #3 were not fixed.
- project.toml, spawn by name or one path base: the project format and its test reject a name on purpose, so it would be a format change.
- BUGS #46, #48, #49, #50, #51, #52 are not touched.

Checks: `cargo check` and `cargo clippy` are clean for maki-devtools and maki-engine, in both the static build and the dynamic one (`--profile dynamic --features maki-engine/dynamic`). `cargo test -p maki-devtools -p maki-engine` passes, and the dynamic `--lib` tests (reload) pass 11/11. My files are formatted with rustfmt.

Tests failing outside my files: none.

## Main panels and tests

## Changes
- `crates/editor/src/panels.rs`: File menu order (finding 1). It uses the ui fixer's `add_menu_item` / `add_menu_separator`:
  - Open ▸ at -90.
  - Save as at -70.
  - Lines at -60 and 90.
  - The commands are New room -100, Save -80 and Quit 100, set in lib.rs.
  - The project rows of reload.rs and new_script.rs stay at order 0, so they fall between the room rows and Quit. Their files did not need a change.
- `crates/editor/src/panels.rs`: the Edit, Add and Assets menus now use `add_menu(.., None)`. Their extra rows are added with `add_menu_item` at order 10, after a line.
- `crates/editor/src/panels.rs`: Save as is now a `name_row` with the hint "Room name" (finding 2). It adds `.ron` only when the name lacks it, so "level.1" saves as "level.1.ron". It rejects `/`, `\` and `..` with a status message. The new private `room_file_name` has a test.
- `crates/editor/src/panels.rs`: File > Open marks the open room and shows "No room files in <folder>" when the list is empty.
- `crates/editor/src/panels.rs`: Make prefab is now a `name_row` with the hint "Prefab name" (finding 2). The explanation is the hover text of the whole row. When nothing that can be a prefab is selected, a weak line says "Select objects to make a prefab." Before, the hover text did not show on the disabled button.
- `crates/editor/src/panels.rs`: snap values (finding 4):
  - Grid shows "Grid N / cell" with the tooltip "Grid snap: steps per room cell".
  - Angle shows "Angle N°" with the tooltip "Rotation snap, in degrees".
  - The Snap tooltip is reworded.
- `crates/editor/src/panels.rs`: Events dock placeholder (finding 6):
  - Nothing selected: a centred "Select an object to edit its event."
  - An object with no event: "This object has no event." and a "➕ Add event" button.
  - In play mode the dock says "The game runs: the Outliner lists the running events."
- `crates/editor/src/panels.rs`: confirm modal:
  - Its title is "Confirm" (was "Question"). The const `CONFIRM` is used only in lib.rs.
  - The text is a question that names the action: "Save the changes to X before you open Y / make a new room / quit?" or "Replace X? The room in it is lost."
  - The hint names the Enter answer.
- `crates/editor/src/panels.rs`: status bar problems (finding 9):
  - "no problem in the events" is now "No problems".
  - The count has icons and correct plurals ("1 error, 2 warnings").
  - An object with no name uses its outliner label (new fn `object_label`, shared with `sections`).
  - The list scrolls, with a max height of 400.
  - A click on a problem also opens the Events and Inspector tabs. `status_bar` now takes `&mut World`.
- `crates/editor/src/panels.rs`: outliner:
  - Ctrl+click toggles a row.
  - Shift+click selects the range from the primary item, and the primary item stays the anchor. New fns `select_range` and `range`, with a test.
  - A group's count counts only the rows the filter shows. "No match" shows when the filter hides all rows, and "Empty. Use the Add menu." when a group is empty.
  - In play mode, "none" is now "No running events".
- `crates/editor/src/panels.rs`: inspector:
  - With several items selected it says "Editing <name> (N selected; changes apply to this one only)".
  - A selected item that no longer exists clears the selection and shows "Select an item in the Outliner to edit it."
  - A rejected value sets the status "Invalid value, not applied: …".
  - The pose checkbox is "Edit pose B", with a tooltip.
  - In Where used, the section titles are bold and "none" is now "None".
- `crates/editor/src/panels.rs`: prefab instance:
  - Errors are red. A missing or unreadable prefab file adds "Restore the file X, or use Edit > Unpack prefab."
  - An object with no id shows as "not linked".
  - A changed field shows "Overridden: field" with a "↺ Revert" small button and a tooltip.
  - "↺ Revert all" shows when a component has more than one overridden field.
- `crates/editor/src/lib.rs`: wording (finding 9):
  - "Load all again" is now "Reload all".
  - "Frame in the view" is now "Frame selection".
  - "View from the top" is now "Top view".
  - "View of the game camera" is now "Game camera view".
  - "Tool: move/rotate/scale" is now "Move/Rotate/Scale tool". These are the names in the gizmo tooltips; `tool_button` already shows the name and key (finding 8).
  - The Local, Step, Run and Cancel names, and the key-binding labels, are shorter.
- `crates/editor/src/lib.rs`: the File commands New room, Save and Quit are added with an `order`.
- `crates/editor/src/lib.rs`: status texts:
  - Command rows: "Commands deleted/pasted/…" or "No change", in place of the Debug text.
  - Item counts have plurals: "Deleted 2 items (Undo brings it back)", "Copied 1 item".
  - Copy or Cut with nothing selected keeps the buffer and says "Nothing selected".
  - Undo says "N more to undo, M to redo".
  - A failed Open says "Could not open X: the current room stays open".
  - Save says "Saved <file>".
  - Play with no spawn point says that the player starts at the view centre.
- `crates/editor/src/lib.rs`: Add, Make prefab and Unpack prefab failures now also show a warning notice (new `Editor::warn`). An asset file that fails to read also shows a warning notice.
- `crates/editor/src/tests.rs`: no change needed. `cargo test -p maki-editor` gives 97 passed. The tests that other fixers reported failing (event_panel field-table tests, ops doorway) pass now.
- Finding 3: nothing to do in my files. The ui fixer already took the running events out of `game_state_panel`.
- Checks: `cargo check -p maki-editor --tests` and `cargo check -p maki-engine` are clean. `cargo clippy -p maki-editor --all-targets` is clean. rustfmt ran on lib.rs, which also formats the editor's submodules.

Skipped:
- Overwrite confirmation for Make prefab: overwriting is the intended flow, which the ops fixer also noted.
- A link in Where used that opens another room and goes to the event: two steps (open, then go to the event) across room loads; medium work.
- Frame distance from the item's bounds: it needs bounds for each item kind.
- The Quit hint while playing ("Stopped. Close again to quit"): low gain.
- A name prompt on the first Save of an untitled room: Save now names the file in the status.
- Inline reset at the changed field: `fields::show` draws the fields. The revert buttons stay under the component, with a tooltip and Revert all.
- An inline red mark at a rejected field: the status text is clearer, but the error is not shown at the field.
- The test of an event whose object is gone retries forever (lib.rs:1588): behaviour change, left for the bug pass.
- Greying out Local for the Scale tool (gizmo fixer's note): `tool_button` takes its enabled state from `can_run`, and the gain is small.
- BUGS #26 ("At the view" entry in Play from): not done, because `play_spawn` falls back to the first spawn when `play_from` is None. The combo now explains the view-centre start when there is no spawn point. The other BUGS items (#20, #21, #27) are left for the bug pass.

Tests failing outside my files: none.
