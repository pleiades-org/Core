//! Settings → Quicklinks: a link and the name it is searched by.
use super::entry_table::{Column, EntryTable, TableEntry, TableSpec};
use core_engine::quicklinks::{Quicklink, MAX_LINK_LENGTH, MAX_NAME_LENGTH, MAX_QUICKLINKS};

pub const FIRST_CELL_ID: usize = 300;
pub const SCROLL_ID: usize = 320;
const LINK: usize = 0;
const NAME: usize = 1;

pub type QuicklinkTable = EntryTable<Quicklink>;

impl TableEntry for Quicklink {
    const SPEC: TableSpec = TableSpec {
        first_cell_id: FIRST_CELL_ID,
        scroll_id: SCROLL_ID,
        entry: "Quicklink",
        scroll: "Quicklinks scroll",
        columns: [
            Column {
                heading: "Link",
                left: 208,
                width: 308,
                limit: MAX_LINK_LENGTH,
            },
            Column {
                heading: "Name",
                left: 528,
                width: 232,
                limit: MAX_NAME_LENGTH,
            },
        ],
        duplicate: "choose a unique name.",
        maximum: MAX_QUICKLINKS,
        too_many: "You can save up to 1000 quicklinks.",
    };

    fn parse(cells: &[String; 2]) -> Result<Self, String> {
        Quicklink::new(&cells[NAME], &cells[LINK])
    }

    fn key(&self) -> String {
        self.name.to_lowercase()
    }

    fn typed_key(cells: &[String; 2]) -> &str {
        &cells[NAME]
    }

    fn cells(&self) -> [String; 2] {
        [self.link.to_string(), self.name.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::super::entry_table::tests::{check, draft, rows};
    use super::*;

    #[test]
    fn the_edited_row_is_checked_alone_with_the_message_the_full_draft_gives() {
        let table = rows(&[
            ("https://example.com", "Docs"),
            ("not a link", "Broken"),
            ("https://example.com/b", ""),
            ("", ""),
        ]);
        assert_eq!(check::<Quicklink>(&table, 0), Ok(()));
        // Blank rows are skipped, as when saving.
        assert_eq!(check::<Quicklink>(&table, 3), Ok(()));
        let broken = check::<Quicklink>(&table, 1).unwrap_err();
        assert!(broken.starts_with("Row 2: "), "{broken}");
        assert_eq!(draft::<Quicklink>(&table).unwrap_err(), broken);
        assert!(check::<Quicklink>(&table, 2)
            .unwrap_err()
            .starts_with("Row 3: "));
    }

    #[test]
    fn a_name_used_twice_is_reported_on_the_second_row_whichever_is_edited() {
        let table = rows(&[
            ("https://example.com/a", "Docs"),
            ("https://example.com/b", "Other"),
            ("https://example.com/c", " DOCS "),
            ("", ""),
        ]);
        let expected = Err("Row 3: choose a unique name.".to_owned());
        assert_eq!(check::<Quicklink>(&table, 0), expected);
        assert_eq!(check::<Quicklink>(&table, 2), expected);
        assert_eq!(check::<Quicklink>(&table, 1), Ok(()));
        assert_eq!(draft::<Quicklink>(&table).map(|_| ()), expected);
    }

    #[test]
    fn names_compare_like_the_full_draft_including_non_ascii() {
        let table = rows(&[
            ("https://example.com/a", "Référence"),
            ("https://example.com/b", "RÉFÉRENCE"),
        ]);
        assert_eq!(
            check::<Quicklink>(&table, 0),
            Err("Row 2: choose a unique name.".to_owned())
        );
        assert!(draft::<Quicklink>(&table).is_err());
        // Final sigma: String::to_lowercase keeps these distinct, so the draft accepts both.
        let sigma = rows(&[
            ("https://example.com/a", "ΟΔΟΣ"),
            ("https://example.com/b", "οδοσ"),
        ]);
        assert_eq!(
            draft::<Quicklink>(&sigma).map(|entries| entries.len()),
            Ok(2)
        );
        assert_eq!(check::<Quicklink>(&sigma, 0), Ok(()));
        assert_eq!(check::<Quicklink>(&sigma, 1), Ok(()));
    }

    #[test]
    fn the_table_keeps_the_quicklinks_page_where_it_was() {
        let spec = &Quicklink::SPEC;
        assert_eq!((spec.first_cell_id, spec.scroll_id), (300, 320));
        assert!(spec.is_edit(300) && spec.is_edit(301) && !spec.is_edit(302));
        assert!(spec.is_edit(310) && !spec.is_edit(311) && !spec.is_edit(312));
        assert!(spec.is_scrollbar(320) && !spec.is_scrollbar(300));
        let entry = Quicklink::new("Docs", "https://example.com").unwrap();
        assert_eq!(entry.cells(), ["https://example.com", "Docs"]);
        assert_eq!(Quicklink::parse(&entry.cells()), Ok(entry));
    }
}
