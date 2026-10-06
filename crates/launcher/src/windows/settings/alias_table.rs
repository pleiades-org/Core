//! Settings → Aliases: a short name and what it stands for.
use super::entry_table::{Column, EntryTable, TableEntry, TableSpec};
use core_engine::aliases::{Alias, MAX_ALIASES, MAX_EXPANSION_LENGTH, MAX_NAME_LENGTH};

pub const FIRST_CELL_ID: usize = 360;
pub const SCROLL_ID: usize = 380;
const NAME: usize = 0;
const EXPANSION: usize = 1;

pub type AliasTable = EntryTable<Alias>;

impl TableEntry for Alias {
    const SPEC: TableSpec = TableSpec {
        first_cell_id: FIRST_CELL_ID,
        scroll_id: SCROLL_ID,
        entry: "Alias",
        scroll: "Aliases scroll",
        // The short name first, as it is read: "d stands for Discord".
        columns: [
            Column {
                heading: "Alias",
                left: 208,
                width: 180,
                limit: MAX_NAME_LENGTH,
            },
            Column {
                heading: "Stands for",
                left: 400,
                width: 360,
                limit: MAX_EXPANSION_LENGTH,
            },
        ],
        duplicate: "this alias is already used.",
        maximum: MAX_ALIASES,
        too_many: "You can save up to 200 aliases.",
    };

    fn parse(cells: &[String; 2]) -> Result<Self, String> {
        Alias::new(&cells[NAME], &cells[EXPANSION])
    }

    fn key(&self) -> String {
        Alias::key(self)
    }

    fn typed_key(cells: &[String; 2]) -> &str {
        &cells[NAME]
    }

    fn cells(&self) -> [String; 2] {
        [self.name.to_string(), self.expansion.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::super::entry_table::tests::{check, draft, rows};
    use super::super::quicklink_table;
    use super::*;
    use core_engine::quicklinks::Quicklink;

    #[test]
    fn completed_rows_become_aliases_and_blank_rows_are_skipped() {
        let table = rows(&[("d", "Discord"), ("", ""), (" @s ", " @song "), ("", "")]);
        let aliases = draft::<Alias>(&table).unwrap();
        assert_eq!(
            aliases.to_vec(),
            [
                Alias::new("d", "Discord").unwrap(),
                Alias::new("@s", "@song").unwrap()
            ]
        );
        assert_eq!(aliases[1].cells(), ["@s", "@song"]);
    }

    #[test]
    fn a_row_is_explained_by_its_number_while_it_is_edited() {
        let table = rows(&[
            ("d", "Discord"),
            ("two words", "Docker"),
            ("x", ""),
            ("D", "Docker"),
        ]);
        assert_eq!(
            check::<Alias>(&table, 1).unwrap_err(),
            draft::<Alias>(&table).unwrap_err()
        );
        assert!(check::<Alias>(&table, 1)
            .unwrap_err()
            .starts_with("Row 2: "));
        assert!(check::<Alias>(&table, 2)
            .unwrap_err()
            .starts_with("Row 3: "));
        // The same alias in another case is the same alias, reported on the later row.
        let expected = Err("Row 4: this alias is already used.".to_owned());
        assert_eq!(check::<Alias>(&table, 0), expected);
        assert_eq!(check::<Alias>(&table, 3), expected);
    }

    #[test]
    fn the_two_tables_never_share_a_control() {
        let (aliases, quicklinks) = (&Alias::SPEC, &Quicklink::SPEC);
        for identifier in FIRST_CELL_ID..=SCROLL_ID {
            assert!(!quicklinks.is_edit(identifier) && !quicklinks.is_scrollbar(identifier));
        }
        for identifier in quicklink_table::FIRST_CELL_ID..=quicklink_table::SCROLL_ID {
            assert!(!aliases.is_edit(identifier) && !aliases.is_scrollbar(identifier));
        }
        assert!(aliases.is_edit(360) && aliases.is_edit(361) && !aliases.is_edit(362));
        // Both columns end before the remove button, as on the Quicklinks page.
        let last = &aliases.columns[1];
        assert_eq!(last.left + last.width, 760);
    }
}
