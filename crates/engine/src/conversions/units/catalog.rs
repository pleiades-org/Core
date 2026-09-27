//! Every supported unit. Scales map to one base unit per dimension:
//! metres, kilograms, seconds, litres, kelvin, square metres, metres per second, bytes,
//! bytes per second, joules, watts, pascals, degrees, hertz and kilometres per litre.
use super::{Dimension, Dimension::*, Unit};

const fn unit(
    dimension: Dimension,
    symbol: &'static str,
    scale: f64,
    aliases: &'static [&'static str],
) -> Unit {
    Unit {
        exact: &[],
        aliases,
        symbol,
        dimension,
        scale,
        offset: 0.,
        inverse: false,
    }
}

/// Case matters for `exact` spellings (`MB` megabyte, `Mb` megabit); aliases ignore case.
const fn cased(
    dimension: Dimension,
    symbol: &'static str,
    scale: f64,
    exact: &'static [&'static str],
    aliases: &'static [&'static str],
) -> Unit {
    Unit {
        exact,
        ..unit(dimension, symbol, scale, aliases)
    }
}

const US_GALLON: f64 = 3.785411784;
const UK_GALLON: f64 = 4.54609;
const INCH: f64 = 0.0254;
const FOOT: f64 = 0.3048;
const MILE: f64 = 1_609.344;
const POUND: f64 = 0.45359237;

pub const UNITS: &[Unit] = &[
    // Length
    unit(
        Length,
        "nm",
        1e-9,
        &["nm", "nanometer", "nanometers", "nanometre", "nanometres"],
    ),
    unit(
        Length,
        "µm",
        1e-6,
        &[
            "µm",
            "um",
            "micron",
            "microns",
            "micrometer",
            "micrometers",
            "micrometre",
            "micrometres",
        ],
    ),
    unit(
        Length,
        "mm",
        0.001,
        &[
            "mm",
            "millimeter",
            "millimeters",
            "millimetre",
            "millimetres",
        ],
    ),
    unit(
        Length,
        "cm",
        0.01,
        &[
            "cm",
            "centimeter",
            "centimeters",
            "centimetre",
            "centimetres",
        ],
    ),
    unit(
        Length,
        "m",
        1.,
        &["m", "meter", "meters", "metre", "metres"],
    ),
    unit(
        Length,
        "km",
        1_000.,
        &["km", "kilometer", "kilometers", "kilometre", "kilometres"],
    ),
    unit(Length, "in", INCH, &["in", "inch", "inches", "\"", "″"]),
    unit(Length, "ft", FOOT, &["ft", "foot", "feet", "'", "′"]),
    unit(Length, "yd", 0.9144, &["yd", "yard", "yards"]),
    unit(Length, "mi", MILE, &["mi", "mile", "miles"]),
    unit(
        Length,
        "nmi",
        1_852.,
        &["nmi", "nauticalmile", "nauticalmiles"],
    ),
    // Mass
    unit(Mass, "µg", 1e-9, &["µg", "ug", "microgram", "micrograms"]),
    unit(Mass, "mg", 1e-6, &["mg", "milligram", "milligrams"]),
    unit(
        Mass,
        "g",
        0.001,
        &["g", "gram", "grams", "gramme", "grammes"],
    ),
    unit(
        Mass,
        "kg",
        1.,
        &["kg", "kilo", "kilos", "kilogram", "kilograms"],
    ),
    unit(
        Mass,
        "t",
        1_000.,
        &["t", "tonne", "tonnes", "metricton", "metrictons"],
    ),
    unit(Mass, "ct", 0.0002, &["ct", "carat", "carats"]),
    unit(Mass, "oz", 0.028349523125, &["oz", "ounce", "ounces"]),
    unit(Mass, "lb", POUND, &["lb", "lbs", "pound", "pounds"]),
    unit(Mass, "st", 14. * POUND, &["st", "stone", "stones"]),
    unit(
        Mass,
        "US ton",
        2_000. * POUND,
        &["ton", "tons", "uston", "ustons", "shortton"],
    ),
    unit(
        Mass,
        "UK ton",
        2_240. * POUND,
        &["ukton", "uktons", "longton"],
    ),
    // Time
    unit(Duration, "ns", 1e-9, &["ns", "nanosecond", "nanoseconds"]),
    unit(
        Duration,
        "µs",
        1e-6,
        &["µs", "us", "microsecond", "microseconds"],
    ),
    unit(
        Duration,
        "ms",
        0.001,
        &["ms", "millisecond", "milliseconds"],
    ),
    unit(
        Duration,
        "s",
        1.,
        &["s", "sec", "secs", "second", "seconds"],
    ),
    unit(Duration, "min", 60., &["min", "mins", "minute", "minutes"]),
    unit(Duration, "h", 3_600., &["h", "hr", "hrs", "hour", "hours"]),
    unit(Duration, "days", 86_400., &["d", "day", "days"]),
    unit(Duration, "weeks", 604_800., &["wk", "wks", "week", "weeks"]),
    unit(Duration, "months", 2_629_746., &["month", "months", "mo"]),
    unit(
        Duration,
        "years",
        31_556_952.,
        &["yr", "yrs", "year", "years"],
    ),
    // Volume (litres). Imperial units need a `uk` prefix; plain names are US customary.
    unit(
        Volume,
        "ml",
        0.001,
        &[
            "ml",
            "millilitre",
            "millilitres",
            "milliliter",
            "milliliters",
            "cc",
            "cm3",
            "cm³",
        ],
    ),
    unit(
        Volume,
        "cl",
        0.01,
        &[
            "cl",
            "centilitre",
            "centilitres",
            "centiliter",
            "centiliters",
        ],
    ),
    unit(
        Volume,
        "dl",
        0.1,
        &["dl", "decilitre", "decilitres", "deciliter", "deciliters"],
    ),
    unit(
        Volume,
        "L",
        1.,
        &["l", "litre", "litres", "liter", "liters"],
    ),
    unit(
        Volume,
        "m³",
        1_000.,
        &[
            "m3",
            "m³",
            "cubicmeter",
            "cubicmeters",
            "cubicmetre",
            "cubicmetres",
        ],
    ),
    unit(
        Volume,
        "US tsp",
        US_GALLON / 768.,
        &["tsp", "teaspoon", "teaspoons"],
    ),
    unit(
        Volume,
        "US tbsp",
        US_GALLON / 256.,
        &["tbsp", "tablespoon", "tablespoons"],
    ),
    unit(
        Volume,
        "US fl oz",
        US_GALLON / 128.,
        &["floz", "fluidounce", "fluidounces"],
    ),
    unit(Volume, "US cup", US_GALLON / 16., &["cup", "cups"]),
    unit(Volume, "US pt", US_GALLON / 8., &["pt", "pint", "pints"]),
    unit(Volume, "US qt", US_GALLON / 4., &["qt", "quart", "quarts"]),
    unit(
        Volume,
        "US gal",
        US_GALLON,
        &["gal", "gallon", "gallons", "usgal"],
    ),
    unit(Volume, "UK fl oz", UK_GALLON / 160., &["ukfloz"]),
    unit(
        Volume,
        "UK pt",
        UK_GALLON / 8.,
        &["ukpt", "ukpint", "ukpints"],
    ),
    unit(
        Volume,
        "UK qt",
        UK_GALLON / 4.,
        &["ukqt", "ukquart", "ukquarts"],
    ),
    unit(
        Volume,
        "UK gal",
        UK_GALLON,
        &["ukgal", "ukgallon", "ukgallons", "impgal"],
    ),
    unit(Volume, "in³", 1.6387064e-2, &["in3", "in³", "cuin"]),
    unit(Volume, "ft³", 28.316846592, &["ft3", "ft³", "cuft"]),
    // Temperature (special offset handling)
    Unit {
        offset: 273.15,
        ..unit(Temperature, "°C", 1., &["c", "°c", "celsius", "centigrade"])
    },
    Unit {
        offset: 273.15 - 32. * 5. / 9.,
        ..unit(Temperature, "°F", 5. / 9., &["f", "°f", "fahrenheit"])
    },
    unit(Temperature, "K", 1., &["k", "kelvin"]),
    // Area
    unit(Area, "mm²", 1e-6, &["mm2", "mm²", "sqmm"]),
    unit(Area, "cm²", 1e-4, &["cm2", "cm²", "sqcm"]),
    unit(Area, "m²", 1., &["m2", "m²", "sqm", "sqmeter", "sqmetre"]),
    unit(Area, "ha", 1e4, &["ha", "hectare", "hectares"]),
    unit(Area, "km²", 1e6, &["km2", "km²", "sqkm"]),
    unit(Area, "in²", INCH * INCH, &["in2", "in²", "sqin"]),
    unit(Area, "ft²", FOOT * FOOT, &["ft2", "ft²", "sqft"]),
    unit(Area, "yd²", 0.83612736, &["yd2", "yd²", "sqyd"]),
    unit(Area, "acres", 4_046.8564224, &["ac", "acre", "acres"]),
    unit(Area, "mi²", MILE * MILE, &["mi2", "mi²", "sqmi"]),
    // Speed
    unit(Speed, "m/s", 1., &["m/s", "mps"]),
    unit(Speed, "km/h", 1. / 3.6, &["km/h", "kmh", "kph", "kmph"]),
    unit(Speed, "mph", MILE / 3_600., &["mph", "mi/h"]),
    unit(Speed, "ft/s", FOOT, &["ft/s", "fps"]),
    unit(
        Speed,
        "kn",
        1_852. / 3_600.,
        &["kn", "kt", "kts", "knot", "knots"],
    ),
    // Data storage (bytes). SI prefixes are powers of 1000; IEC prefixes are powers of 1024.
    cased(Data, "bit", 0.125, &["b"], &["bit", "bits"]),
    cased(Data, "B", 1., &["B"], &["byte", "bytes"]),
    cased(
        Data,
        "kbit",
        125.,
        &["Kb"],
        &["kbit", "kbits", "kilobit", "kilobits"],
    ),
    cased(
        Data,
        "kB",
        1e3,
        &["kB", "KB"],
        &["kb", "kilobyte", "kilobytes"],
    ),
    cased(
        Data,
        "Mbit",
        1.25e5,
        &["Mb"],
        &["mbit", "mbits", "megabit", "megabits"],
    ),
    cased(Data, "MB", 1e6, &["MB"], &["mb", "megabyte", "megabytes"]),
    cased(
        Data,
        "Gbit",
        1.25e8,
        &["Gb"],
        &["gbit", "gbits", "gigabit", "gigabits"],
    ),
    cased(Data, "GB", 1e9, &["GB"], &["gb", "gigabyte", "gigabytes"]),
    cased(
        Data,
        "Tbit",
        1.25e11,
        &["Tb"],
        &["tbit", "tbits", "terabit", "terabits"],
    ),
    cased(Data, "TB", 1e12, &["TB"], &["tb", "terabyte", "terabytes"]),
    cased(Data, "PB", 1e15, &["PB"], &["pb", "petabyte", "petabytes"]),
    unit(Data, "KiB", 1_024., &["kib", "kibibyte", "kibibytes"]),
    unit(Data, "MiB", 1_048_576., &["mib", "mebibyte", "mebibytes"]),
    unit(
        Data,
        "GiB",
        1_073_741_824.,
        &["gib", "gibibyte", "gibibytes"],
    ),
    unit(
        Data,
        "TiB",
        1_099_511_627_776.,
        &["tib", "tebibyte", "tebibytes"],
    ),
    unit(
        Data,
        "PiB",
        1_125_899_906_842_624.,
        &["pib", "pebibyte", "pebibytes"],
    ),
    // Data rate (bytes per second). Lowercase `mbps` and `mb/s` mean megabits, as ISPs write.
    unit(DataRate, "bps", 0.125, &["bps", "bit/s"]),
    unit(DataRate, "kbps", 125., &["kbps", "kbit/s", "kb/s"]),
    cased(
        DataRate,
        "Mbps",
        1.25e5,
        &["Mb/s"],
        &["mbps", "mbit/s", "mb/s"],
    ),
    cased(
        DataRate,
        "Gbps",
        1.25e8,
        &["Gb/s"],
        &["gbps", "gbit/s", "gb/s"],
    ),
    unit(DataRate, "Tbps", 1.25e11, &["tbps", "tbit/s"]),
    cased(DataRate, "B/s", 1., &["B/s", "Bps"], &[]),
    cased(
        DataRate,
        "kB/s",
        1e3,
        &["kB/s", "KB/s", "kBps", "KBps"],
        &[],
    ),
    cased(DataRate, "MB/s", 1e6, &["MB/s", "MBps"], &[]),
    cased(DataRate, "GB/s", 1e9, &["GB/s", "GBps"], &[]),
    unit(DataRate, "KiB/s", 1_024., &["kib/s"]),
    unit(DataRate, "MiB/s", 1_048_576., &["mib/s"]),
    unit(DataRate, "GiB/s", 1_073_741_824., &["gib/s"]),
    // Energy (joules)
    unit(Energy, "J", 1., &["j", "joule", "joules"]),
    unit(Energy, "kJ", 1e3, &["kj", "kilojoule", "kilojoules"]),
    unit(Energy, "MJ", 1e6, &["mj", "megajoule", "megajoules"]),
    cased(Energy, "cal", 4.184, &["cal"], &["calorie", "calories"]),
    cased(
        Energy,
        "kcal",
        4_184.,
        &["Cal"],
        &["kcal", "kilocalorie", "kilocalories"],
    ),
    unit(Energy, "Wh", 3_600., &["wh", "watthour", "watthours"]),
    unit(
        Energy,
        "kWh",
        3.6e6,
        &["kwh", "kilowatthour", "kilowatthours"],
    ),
    unit(
        Energy,
        "MWh",
        3.6e9,
        &["mwh", "megawatthour", "megawatthours"],
    ),
    unit(Energy, "BTU", 1_055.05585262, &["btu", "btus"]),
    unit(
        Energy,
        "eV",
        1.602176634e-19,
        &["ev", "electronvolt", "electronvolts"],
    ),
    // Power (watts)
    unit(Power, "W", 1., &["w", "watt", "watts"]),
    unit(Power, "kW", 1e3, &["kw", "kilowatt", "kilowatts"]),
    unit(Power, "MW", 1e6, &["mw", "megawatt", "megawatts"]),
    unit(Power, "GW", 1e9, &["gw", "gigawatt", "gigawatts"]),
    unit(Power, "hp", 745.699_871_582_270_2, &["hp", "horsepower"]),
    unit(Power, "BTU/h", 0.29307107, &["btu/h", "btuh"]),
    // Pressure (pascals)
    unit(Pressure, "Pa", 1., &["pa", "pascal", "pascals"]),
    unit(
        Pressure,
        "hPa",
        100.,
        &["hpa", "hectopascal", "hectopascals"],
    ),
    unit(Pressure, "kPa", 1e3, &["kpa", "kilopascal", "kilopascals"]),
    unit(Pressure, "MPa", 1e6, &["mpa", "megapascal", "megapascals"]),
    unit(Pressure, "mbar", 100., &["mbar", "millibar", "millibars"]),
    unit(Pressure, "bar", 1e5, &["bar", "bars"]),
    unit(Pressure, "psi", 6_894.757293168, &["psi"]),
    unit(
        Pressure,
        "atm",
        101_325.,
        &["atm", "atmosphere", "atmospheres"],
    ),
    unit(Pressure, "mmHg", 133.322387415, &["mmhg", "torr"]),
    unit(Pressure, "inHg", 3_386.388640341, &["inhg"]),
    // Angle (degrees)
    unit(Angle, "°", 1., &["°", "deg", "degree", "degrees"]),
    unit(
        Angle,
        "rad",
        180. / std::f64::consts::PI,
        &["rad", "radian", "radians"],
    ),
    unit(Angle, "grad", 0.9, &["grad", "gradian", "gradians", "gon"]),
    unit(
        Angle,
        "turns",
        360.,
        &["turn", "turns", "rev", "revs", "revolution", "revolutions"],
    ),
    unit(
        Angle,
        "arcmin",
        1. / 60.,
        &["arcmin", "arcminute", "arcminutes"],
    ),
    unit(
        Angle,
        "arcsec",
        1. / 3_600.,
        &["arcsec", "arcsecond", "arcseconds"],
    ),
    // Frequency (hertz)
    unit(Frequency, "Hz", 1., &["hz", "hertz"]),
    unit(Frequency, "kHz", 1e3, &["khz", "kilohertz"]),
    unit(Frequency, "MHz", 1e6, &["mhz", "megahertz"]),
    unit(Frequency, "GHz", 1e9, &["ghz", "gigahertz"]),
    unit(Frequency, "rpm", 1. / 60., &["rpm", "bpm"]),
    // Fuel economy (kilometres per litre). L/100km is inverse: more fuel means lower economy.
    unit(FuelEconomy, "km/L", 1., &["km/l", "kmpl"]),
    unit(
        FuelEconomy,
        "mpg (US)",
        MILE / 1_000. / US_GALLON,
        &["mpg", "usmpg", "mpgus"],
    ),
    unit(
        FuelEconomy,
        "mpg (UK)",
        MILE / 1_000. / UK_GALLON,
        &["ukmpg", "mpguk", "impmpg"],
    ),
    Unit {
        inverse: true,
        ..unit(FuelEconomy, "L/100km", 100., &["l/100km", "l/100"])
    },
];
