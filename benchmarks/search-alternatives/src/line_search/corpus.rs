use std::io;
use std::path::Path;

#[derive(Clone, Copy)]
pub struct Span {
    pub first: usize,
    pub end: usize,
}

#[derive(Default)]
pub struct Corpus {
    pub bytes: Vec<u8>,
    pub lines: Vec<Span>,
    pub files: Vec<Span>,
}

impl Corpus {
    pub fn append(&mut self, text: &str) {
        let first = self.lines.len();
        for line in text.lines() {
            let start = self.bytes.len();
            self.bytes.extend_from_slice(line.as_bytes());
            self.lines.push(Span {
                first: start,
                end: self.bytes.len(),
            });
            self.bytes.push(b'\n');
        }
        self.files.push(Span {
            first,
            end: self.lines.len(),
        });
    }

    pub fn line(&self, identifier: usize) -> &[u8] {
        let line = self.lines[identifier];
        &self.bytes[line.first..line.end]
    }

    pub fn from_manifest(path: &Path) -> io::Result<Self> {
        let manifest = std::fs::read_to_string(path)?;
        let mut result = Self::default();
        for filename in manifest.lines().filter(|line| !line.is_empty()) {
            let text = std::fs::read_to_string(filename.trim_start_matches('\u{feff}'))?;
            result.append(&text);
        }
        Ok(result)
    }
}

pub fn synthetic(file_count: usize) -> Corpus {
    let mut corpus = Corpus::default();
    for file in 0..file_count {
        let mut text = String::new();
        for line in 0..512 {
            let body = match line % 8 {
                0 => "pub fn configure_service(settings: &Settings) -> Result<Client, Error> { // abc marker",
                1 => "    let client = connect_to_endpoint(&settings.url)?; // bcd marker",
                2 => "    return Ok(client); // ordinary code and documentation",
                3 => "// café 東京: configure service timeout, retry count and connection pool",
                4 => "    let selection = candidates.iter().filter(|candidate| candidate.visible);",
                5 => "    update_state(notification); // the same words occur in many different files",
                6 => "    validate_input(request.body)?; // expected errors should be propagated",
                _ => "} // end of generated function",
            };
            text.push_str(&format!("{body} // id {file:04x}_{line:04x}\n"));
        }
        if file == file_count / 2 {
            text.push_str("// needle_tag_7fd9 only exists in a file body\n");
        }
        if file % 17 == 0 {
            text.push_str("// abcd is contiguous here\n");
        }
        corpus.append(&text);
    }
    corpus
}
