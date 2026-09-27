//! Currencies with European Central Bank reference rates, plus the euro itself.
use std::sync::LazyLock;

pub struct Currency {
    pub code: &'static str,
    /// Prefix used when displaying amounts; `None` shows the code after the number.
    pub symbol: Option<&'static str>,
    pub decimals: usize,
    /// Lowercase names and symbols accepted in queries, including multi-word names.
    pub names: &'static [&'static str],
    /// Names that also mean something else (`pounds` is a mass unit). They count as this
    /// currency only when the other side of the conversion is unambiguously a currency.
    pub ambiguous: &'static [&'static str],
}

const fn currency(
    code: &'static str,
    symbol: Option<&'static str>,
    decimals: usize,
    names: &'static [&'static str],
) -> Currency {
    Currency {
        code,
        symbol,
        decimals,
        names,
        ambiguous: &[],
    }
}

pub const CURRENCIES: &[Currency] = &[
    currency("EUR", Some("€"), 2, &["eur", "€", "euro", "euros"]),
    currency(
        "USD",
        Some("$"),
        2,
        &[
            "usd",
            "$",
            "us$",
            "dollar",
            "dollars",
            "us dollar",
            "us dollars",
            "buck",
            "bucks",
        ],
    ),
    Currency {
        ambiguous: &["pound", "pounds"],
        ..currency(
            "GBP",
            Some("£"),
            2,
            &[
                "gbp",
                "£",
                "quid",
                "sterling",
                "pound sterling",
                "british pound",
                "british pounds",
                "uk pound",
                "uk pounds",
            ],
        )
    },
    currency(
        "JPY",
        Some("¥"),
        0,
        &["jpy", "¥", "yen", "japanese yen", "円"],
    ),
    currency(
        "CNY",
        None,
        2,
        &["cny", "rmb", "yuan", "renminbi", "chinese yuan", "元"],
    ),
    currency(
        "INR",
        Some("₹"),
        2,
        &[
            "inr",
            "₹",
            "rupee",
            "rupees",
            "indian rupee",
            "indian rupees",
        ],
    ),
    currency(
        "KRW",
        Some("₩"),
        0,
        &["krw", "₩", "won", "korean won", "south korean won"],
    ),
    currency(
        "CHF",
        None,
        2,
        &["chf", "franc", "francs", "swiss franc", "swiss francs"],
    ),
    currency(
        "CAD",
        None,
        2,
        &["cad", "c$", "ca$", "canadian dollar", "canadian dollars"],
    ),
    currency(
        "AUD",
        None,
        2,
        &[
            "aud",
            "a$",
            "au$",
            "australian dollar",
            "australian dollars",
        ],
    ),
    currency(
        "NZD",
        None,
        2,
        &["nzd", "nz$", "new zealand dollar", "new zealand dollars"],
    ),
    currency(
        "HKD",
        None,
        2,
        &["hkd", "hk$", "hong kong dollar", "hong kong dollars"],
    ),
    currency(
        "SGD",
        None,
        2,
        &["sgd", "s$", "singapore dollar", "singapore dollars"],
    ),
    currency(
        "MXN",
        None,
        2,
        &[
            "mxn",
            "mex$",
            "peso",
            "pesos",
            "mexican peso",
            "mexican pesos",
        ],
    ),
    currency(
        "BRL",
        None,
        2,
        &[
            "brl",
            "r$",
            "real",
            "reais",
            "brazilian real",
            "brazilian reais",
        ],
    ),
    currency(
        "SEK",
        None,
        2,
        &["sek", "kronor", "swedish krona", "swedish kronor"],
    ),
    currency(
        "NOK",
        None,
        2,
        &["nok", "norwegian krone", "norwegian kroner"],
    ),
    currency("DKK", None, 2, &["dkk", "danish krone", "danish kroner"]),
    currency(
        "ISK",
        None,
        0,
        &["isk", "icelandic krona", "icelandic kronur"],
    ),
    currency(
        "PLN",
        None,
        2,
        &["pln", "zł", "zloty", "zlotys", "złoty", "polish zloty"],
    ),
    currency(
        "CZK",
        None,
        2,
        &["czk", "kč", "koruna", "korun", "czech koruna"],
    ),
    currency(
        "HUF",
        None,
        0,
        &["huf", "forint", "forints", "hungarian forint"],
    ),
    currency("RON", None, 2, &["ron", "lei", "leu", "romanian leu"]),
    currency("BGN", None, 2, &["bgn", "lev", "leva", "bulgarian lev"]),
    currency(
        "TRY",
        Some("₺"),
        2,
        &["try", "₺", "lira", "liras", "turkish lira"],
    ),
    currency(
        "ZAR",
        None,
        2,
        &["zar", "rand", "rands", "south african rand"],
    ),
    currency("THB", Some("฿"), 2, &["thb", "฿", "baht", "thai baht"]),
    currency(
        "ILS",
        Some("₪"),
        2,
        &["ils", "₪", "nis", "shekel", "shekels", "israeli shekel"],
    ),
    currency("MYR", None, 2, &["myr", "ringgit", "malaysian ringgit"]),
    currency("IDR", None, 0, &["idr", "rupiah", "indonesian rupiah"]),
    currency(
        "PHP",
        Some("₱"),
        2,
        &["php", "₱", "philippine peso", "philippine pesos"],
    ),
];

/// Currency for a lowercase name; the flag is true when the name is ambiguous.
pub fn find_currency(name: &str) -> Option<(&'static Currency, bool)> {
    let name = name.trim();
    CURRENCIES.iter().find_map(|currency| {
        if currency
            .names
            .iter()
            .any(|known| name.eq_ignore_ascii_case(known))
        {
            Some((currency, false))
        } else if currency
            .ambiguous
            .iter()
            .any(|known| name.eq_ignore_ascii_case(known))
        {
            Some((currency, true))
        } else {
            None
        }
    })
}

pub fn by_code(code: &str) -> Option<&'static Currency> {
    CURRENCIES
        .iter()
        .find(|currency| currency.code.eq_ignore_ascii_case(code))
}

/// Symbols that may be written directly against an amount: `$100`, `100€`, `£1.5k`.
/// Longer symbols first so `us$` is not read as `$`. Built once; queries only read it.
pub fn attached_symbols() -> &'static [(&'static str, &'static Currency)] {
    static SYMBOLS: LazyLock<Vec<(&'static str, &'static Currency)>> = LazyLock::new(|| {
        let mut symbols: Vec<_> = CURRENCIES
            .iter()
            .flat_map(|currency| {
                currency
                    .names
                    .iter()
                    .filter(|name| {
                        !name
                            .bytes()
                            .all(|byte| byte.is_ascii_alphabetic() || byte == b' ')
                    })
                    .map(move |name| (*name, currency))
            })
            .collect();
        symbols.sort_by_key(|(symbol, _)| std::cmp::Reverse(symbol.len()));
        symbols
    });
    &SYMBOLS
}
