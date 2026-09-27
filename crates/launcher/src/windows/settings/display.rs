use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisplayChoice {
    #[default]
    Active,
    Device([u16; 32]),
}

impl DisplayChoice {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.eq_ignore_ascii_case("active") {
            return Ok(Self::Active);
        }
        let number = text
            .strip_prefix(r"\\.\DISPLAY")
            .ok_or("Invalid display identifier.")?;
        if number.is_empty()
            || !number.bytes().all(|byte| byte.is_ascii_digit())
            || text.len() >= 32
        {
            return Err("Invalid display identifier.".into());
        }
        let mut identifier = [0; 32];
        for (destination, character) in identifier.iter_mut().zip(text.encode_utf16()) {
            *destination = character;
        }
        Ok(Self::Device(identifier))
    }
}
impl fmt::Display for DisplayChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => formatter.write_str("Active"),
            Self::Device(identifier) => formatter.write_str(&String::from_utf16_lossy(
                &identifier[..identifier
                    .iter()
                    .position(|character| *character == 0)
                    .unwrap_or(identifier.len())],
            )),
        }
    }
}
