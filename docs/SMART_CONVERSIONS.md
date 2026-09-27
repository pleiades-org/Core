# Smart conversions and calculations

Type these directly in Core's search box (or after `@calc`). Each answer is a result row; Enter copies that row's value without thousands separators. Unrecognised text falls through to arithmetic and application search.

## Units

`<number> <unit> to|in|into|as <unit>`, with or without a space (`100cm to m`, `4.7GB in MiB`). Results show up to 12 significant figures.

| Kind | Units |
| --- | --- |
| Length | nm, µm (um), mm, cm, m, km, in, ft, yd, mi, nmi |
| Mass | µg, mg, g, kg, t (tonne), ct (carat), oz, lb, st (stone), ton (US short), ukton (long) |
| Time | ns, µs, ms, s, min, h, day, week, month (30.44 days), year (365.2425 days) |
| Volume | ml (cc, cm3), cl, dl, L, m3, tsp, tbsp, floz, cup, pt, qt, gal, in3, ft3 |
| Temperature | °C (c), °F (f), K |
| Area | mm2, cm2, m2 (sqm), ha, km2, in2, ft2 (sqft), yd2, acre, mi2 |
| Speed | m/s, km/h (kph), mph, ft/s, kn (knots) |
| Data | b/bit, B/byte, kB, MB, GB, TB, PB, Kb/kbit, Mb/Mbit, Gb/Gbit, Tb/Tbit, KiB, MiB, GiB, TiB, PiB |
| Data rate | bps, kbps, Mbps, Gbps, Tbps, B/s, kB/s, MB/s, GB/s, KiB/s, MiB/s, GiB/s |
| Energy | J, kJ, MJ, cal, kcal (Cal), Wh, kWh, MWh, BTU, eV |
| Power | W, kW, MW, GW, hp, BTU/h |
| Pressure | Pa, hPa, kPa, MPa, mbar, bar, psi, atm, mmHg (torr), inHg |
| Angle | deg (°), rad, grad, turn, arcmin, arcsec |
| Frequency | Hz, kHz, MHz, GHz, rpm (bpm) |
| Fuel economy | km/L, mpg (US), ukmpg, L/100km (inverse) |

Notes:

- **Bits and bytes:** `MB` is megabytes and `Mb` is megabits. Lowercase `mb` means megabytes; lowercase `mbps` and `mb/s` mean megabits per second, as internet providers write them. `MB/s` and `MBps` are megabytes per second.
- **SI and binary prefixes:** `GB` is 1,000³ bytes; `GiB` is 1,024³ bytes. `1 TB to GiB` gives 931.32.
- **US and UK measures:** plain `gal`, `qt`, `pt`, `cup` and `floz` are US customary. Imperial measures use a `uk` prefix: `ukgal`, `ukqt`, `ukpt`, `ukfloz`.
- **Pints versus Pacific time:** `9 pt to et` is a time-zone conversion and `9 pt to cup` is volume. A complete time-zone query always wins.

## Currency

`100 usd to eur`, `£50 in dollars`, `€1.5k to yen`, `1,000 swiss francs to usd`, `usd 100 to gbp`, or just `100 usd` (converts to your Windows regional currency).

- Codes, symbols (`$ € £ ¥ ₹ ₩ ₺ ₪ ₱ ฿`, `us$`, `c$`, `a$`, `hk$`…) and everyday names (`dollars`, `euros`, `quid`, `yen`, `swiss francs`…) work on either side.
- `k`, `m` and `bn` scale amounts: `1.5k`, `2m`, `3bn`.
- `pounds` counts as sterling only when the other side is clearly a currency, so `10 pounds to kg` stays a mass conversion.
- Rates are the European Central Bank's daily euro reference rates: the euro plus the 29 currencies published on 22 September 2026. Currencies the ECB does not publish (such as the Bulgarian lev) are recognised and explained rather than guessed. Each answer shows the rate and its publication date.
- When Core is shown, it loads cached rates from `%APPDATA%\Pleiades\Core\v2\exchange-rates.xml`. It downloads a fresh file from `www.ecb.europa.eu` only when the cache is more than 12 hours old, and waits 15 minutes after a failed attempt. Nothing is downloaded while Core is hidden, and searching itself never uses the network.
- `--exchange-rates-file <path>` uses a fixed ECB file and never downloads. `--dry-run` never downloads unless `--test-network` is given.
- Reference rates are indicative mid-market rates, not the rates a bank or card will charge.

## Downloads, speeds and pace

| Query | Answer |
| --- | --- |
| `10 GB at 100 Mbps`, `download 4.7GB @ 50mbps`, `how long to download 1 TB over 1 Gbps` | Transfer time (ideal; real transfers add protocol overhead) |
| `10 GB in 10 min` | Speed needed, in bits per second and MB/s |
| `100 Mbps for 2 hours` | Data transferred, in SI and binary units |
| `5 km in 25 min`, `26.2 mi in 3:30:00` | Pace per km or mile, and speed |

Durations can be written `90 min`, `1h 30m`, `1 hour 30 minutes`, `1:30:00` or `25:00`.

## Percentages, tips and bills

| Query | Answer |
| --- | --- |
| `20% off 80`, `15 percent discount on $200` | Discounted price and saving |
| `20 is what % of 80`, `20 as a % of 80`, `what % of 80 is 20` | 25% |
| `% change from 50 to 75`, `percent change 80 to 60` | +50%, −25% |
| `increase 50 by 10%`, `50 decreased by 20%` | 55, 40 |
| `15 is 20% of what` | 75 |
| `tip 15% on 80`, `80 tip 20%`, `80 with 20% tip for 3 people` | Total, tip, and each share when split |
| `split 120 3 ways`, `split 100 by 3` | Each share |

The calculator keeps its existing meanings: `25% of 80` is 20, and `20 + 10%` is 20.1.

## Numbers, colours and time

| Query | Answer |
| --- | --- |
| `255 to hex`, `255 in binary`, `0xff to dec`, `0o777 to hex` | Converted base |
| `0xff`, `0b1111_0000` | Decimal and the other bases |
| `2024 to roman`, `MMXXIV to number`, `roman xlii` | Roman numerals (1–3999, canonical form only) |
| `#ff8800`, `#f80`, `rgb(255, 136, 0) to hex`, `hsl 210 50 40 to rgb` | HEX, RGB and HSL; a `to` target is listed first |
| `unix 1700000000`, `1700000000000 unix` (milliseconds), `0 to date` | Local time and UTC ISO 8601 |
| `unix now`, `now to unix` | Current Unix seconds and milliseconds |

## Screens, money and health

| Query | Answer |
| --- | --- |
| `1920x1080`, `aspect 1366 x 768` | Aspect ratio (familiar ratio when within 1%) and megapixels |
| `16:9 1440 wide`, `16:9 height 1080` | Matching size |
| `ppi 2560x1440 27in`, `27 inch 3840x2160 dpi` | Pixel density, dot pitch and visible size |
| `mortgage 250k at 4.5% for 25 years`, `loan 10000 at 7% over 36 months` | Monthly payment, total interest and total repaid |
| `compound 1000 at 5% for 10 years`, `… compounded monthly`, `1000 at 5% for 10 years` | Future value and interest earned |
| `bmi 70kg 175cm`, `bmi 154 lb 5ft 9in`, `bmi 11st 5'9"`, `bmi 70 175` | BMI and WHO adult category (not medical advice) |

Loan and savings figures exclude fees and taxes. Bare `WxH` is read as a resolution only when both sides are at least 100.
