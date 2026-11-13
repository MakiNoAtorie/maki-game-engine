# UX pass: general findings

From a run of `maki-editor` on `game/` (4K screen, `[ui] scale = 1.0`) and a read of the shell and the editor panels. These are the main problems. The detailed problems of each file come from the file reviews.

## Structure

1. **File menu order.** "Quit" is in the middle. "Open", and "Save as" with its text field, come after it. Expected order: New room, Open ▸, Save, Save as, then the project items (Build the scripts, Open the project in the code editor, New script), then Quit at the end.
2. **Name fields inside menus.** Save as (File), Make prefab (Edit), New routine, New menu, and New data asset (Assets) are a text field and a button in a menu row. Enter does not submit, the order of the field and the button differs between menus, and "New data asset" is a submenu whose kind is the menu label (`beacon_style ▸`). Make them consistent: one row with the field first, Enter submits, and the button stays disabled while the name is empty.
3. **Play mode shows "Running events" twice.** The outliner and the inspector both list the running events. The inspector should show the game state only.
4. **Unlabelled snap values.** The tool row shows `8 per cell` and `15.0 deg` with no label or tooltip. They need "Grid" and "Angle" tooltips.
5. **No UI scale control.** On a 4K screen at scale 1.0 the 13 pt text is very small. `[ui] scale` is only in the config file. Add View > Zoom in / Zoom out / Reset zoom (Ctrl+= / Ctrl+- / Ctrl+0).
6. **Empty bottom dock.** With nothing selected, the Events tab shows one dim line in a 280 pt dock. The placeholder text should say what to do: select an object, or "Add an event".

## Visual

7. **Low contrast of weak text.** Hints, placeholders, and "no problem in the events" are hard to read on the dark background.
8. **Icon-only tool buttons.** Move, rotate, and scale are single glyphs. They need a tooltip with the name and the key.
9. **Wording.** Some labels are stilted and do not use the usual terms of an editor: "nothing has this text" (No results), "no problem in the events" (No problems), "Load all again" (Reload all), "Frame in the view" (Frame selection), "View of the game camera" (Game camera view). Use the short, usual term of an editor (Unity, Godot) for a label. Keep doc comments as they are.

## Bugs

See `BUGS.md`.
