//! A snapshot of European Central Bank euro reference rates. The launcher downloads and caches
//! the ECB file; the engine only parses text, so searching never touches the network.
use super::catalog::by_code;

const MIN_RATES: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct ExchangeRates {
    date: String,
    /// Units of each currency per euro, e.g. `("USD", 1.08)`. The euro itself is implicit.
    per_euro: Vec<(&'static str, f64)>,
    local: Option<&'static str>,
}

impl ExchangeRates {
    /// Parses `eurofxref-daily.xml`: one `time='YYYY-MM-DD'` and `currency='USD' rate='1.08'`
    /// pairs. Unknown currencies are skipped; malformed files are rejected.
    pub fn from_ecb_xml(xml: &str) -> Result<Self, &'static str> {
        let date = attribute(xml, "time").ok_or("The exchange rate file has no date")?;
        if !is_iso_date(date) {
            return Err("The exchange rate file has an invalid date");
        }
        let mut per_euro = Vec::new();
        for tag in xml.split('<') {
            let (Some(code), Some(rate)) = (attribute(tag, "currency"), attribute(tag, "rate"))
            else {
                continue;
            };
            let rate: f64 = rate
                .parse()
                .map_err(|_| "An exchange rate is not a number")?;
            if !(rate.is_finite() && rate > 0.) {
                return Err("An exchange rate is out of range");
            }
            if let Some(currency) = by_code(code).filter(|currency| currency.code != "EUR") {
                per_euro.push((currency.code, rate));
            }
        }
        if per_euro.len() < MIN_RATES {
            return Err("The exchange rate file has too few rates");
        }
        Ok(Self {
            date: date.to_owned(),
            per_euro,
            local: None,
        })
    }

    /// The user's own currency, used when a query names only the source (`100 usd`).
    pub fn with_local_currency(mut self, code: &str) -> Self {
        self.local = by_code(code).map(|currency| currency.code);
        self
    }

    pub fn date(&self) -> &str {
        &self.date
    }

    pub fn local(&self) -> Option<&'static str> {
        self.local
    }

    pub fn per_euro(&self, code: &str) -> Option<f64> {
        if code == "EUR" {
            return Some(1.);
        }
        self.per_euro
            .iter()
            .find(|(known, _)| *known == code)
            .map(|(_, rate)| *rate)
    }
}

/// Value of `name='…'` or `name="…"` in `text`.
fn attribute<'text>(text: &'text str, name: &str) -> Option<&'text str> {
    let mut search = text;
    while let Some(index) = search.find(name) {
        let after = &search[index + name.len()..];
        let preceded_by_word = search[..index]
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_alphanumeric());
        if let Some(value) = after.strip_prefix('=').filter(|_| !preceded_by_word) {
            let quote = value
                .chars()
                .next()
                .filter(|quote| matches!(quote, '\'' | '"'))?;
            let value = &value[1..];
            return value.find(quote).map(|end| &value[..end]);
        }
        search = after;
    }
    None
}

fn is_iso_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

#[cfg(test)]
pub(crate) const SAMPLE_ECB_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01" xmlns="http://www.ecb.int/vocabulary/2002-08-01/eurofxref">
  <gesmes:subject>Reference rates</gesmes:subject>
  <Cube>
    <Cube time='2026-09-21'>
      <Cube currency='USD' rate='1.1000'/>
      <Cube currency='JPY' rate='160.00'/>
      <Cube currency='GBP' rate='0.8500'/>
      <Cube currency='CHF' rate='0.9400'/>
      <Cube currency='INR' rate='92.000'/>
      <Cube currency='KRW' rate='1500.00'/>
      <Cube currency='XXX' rate='3.0'/>
    </Cube>
  </Cube>
</gesmes:Envelope>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecb_reference_rates_parse_and_skip_unknown_currencies() {
        let rates = ExchangeRates::from_ecb_xml(SAMPLE_ECB_XML).unwrap();
        assert_eq!(rates.date(), "2026-09-21");
        assert_eq!(rates.per_euro("USD"), Some(1.1));
        assert_eq!(rates.per_euro("EUR"), Some(1.));
        assert_eq!(rates.per_euro("XXX"), None);
        assert_eq!(rates.with_local_currency("gbp").local(), Some("GBP"));
    }

    #[test]
    fn malformed_rate_files_are_rejected() {
        for xml in [
            "",
            "<Cube time='2026-09-21'></Cube>",
            "<Cube time='yesterday'><Cube currency='USD' rate='1'/></Cube>",
            &SAMPLE_ECB_XML.replace("rate='1.1000'", "rate='abc'"),
            &SAMPLE_ECB_XML.replace("rate='1.1000'", "rate='-1'"),
        ] {
            assert!(ExchangeRates::from_ecb_xml(xml).is_err(), "{xml}");
        }
    }
}
