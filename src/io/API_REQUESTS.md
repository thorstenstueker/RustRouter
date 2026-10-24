# API requests / notes from the `io::specctra` port

All items below are worked around locally inside `src/io/`; nothing outside `src/io/` was changed.

1. **Via rules and via infos are referenced by index** (`ViaRuleId`, `ViaInfoId`), the original holds
   object references.
   * `Network.addViaRule` removes an existing rule with the same name and appends the new one. In
     Rust the indices of the following rules shift, so `io::specctra::network::add_via_rule` remaps
     the `via_rule` of all net classes (a class referencing the replaced rule now references the new
     rule with the same name; in Java it keeps the removed object). This only matters when reading
     `.rules` files (during DSN reading the net classes get their via rules afterwards).
   * `ViaInfos` has no `remove`. `RulesReader.applyViaInfo` removes an existing via info and
     appends the new one; `rules_file::replace_via_info` rebuilds `ViaInfos` and the via rule lists.
   * Request: `ViaInfos::remove(id)` / `BoardRules::replace_via_rule(..)` helpers, or stable ids.

2. **No meander constraints / differential pairs in `BoardRules`, `NetClass`, `Net`.** The DSN
   meander descriptors (`length_amplitude`, `length_gap`, `corner_style`, `radius`,
   `single_sided`, `meander_pattern`) are parsed (token compatible) but dropped. The rules writer
   therefore never writes meander properties.

3. **`Package.outline: Vec<Shape>` cannot hold `null`.** The original keeps a `null` entry for
   outline shapes, which cannot be transformed (`polyline_path` outlines). These are skipped together
   with their `outline_widths`/`outline_is_closed` entries. Only the courtyard heuristic
   (`outline.length > 1`) could differ for packages with such outlines; no item is inserted for
   them in either implementation.

4. **`Keepout.area: Area` cannot be `null`.** Package keepouts whose area cannot be transformed are
   dropped (the original keeps them, but inserts no board item for them).

5. **Coordinate transform:** `board::structure::CoordinateTransform` matches
   `io/CoordinateTransform.java` (scale factor + base x/y). The point/tuple transforms
   (`dsnToBoard(double[])`, `boardToDsn(FloatPoint)`, ...) are free functions in
   `io/specctra/shapes.rs`. No change needed.

6. **Autoroute settings** of the DSN (`(structure (autoroute_settings ...))`) and of `.rules`
   files are returned as the plain struct `DsnAutorouteSettings` (values as given in the file,
   via costs clamped to >= 1 like the `RouterSettings` setters); they are not applied to any
   router settings object.

7. Unchecked exceptions of the original (NullPointerException / NumberFormatException /
   ClassCastException inside the parser, e.g. an integer literal > 2^31-1) abort the read with
   `DsnError::Parse` instead of a crash.
