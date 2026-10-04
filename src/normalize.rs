//! Conservative German sensor-value normalization. No regexes, floats or model changes.
use std::{borrow::Cow, collections::VecDeque};

pub fn language(code: &str) -> &str {
    if code
        .split(['-', '_'])
        .next()
        .is_some_and(|s| s.eq_ignore_ascii_case("de"))
    {
        "de"
    } else {
        code
    }
}
pub fn enabled(flag: bool, code: &str) -> bool {
    flag && language(code) == "de"
}

fn small(n: u64) -> &'static str {
    [
        "null",
        "eins",
        "zwei",
        "drei",
        "vier",
        "fünf",
        "sechs",
        "sieben",
        "acht",
        "neun",
        "zehn",
        "elf",
        "zwölf",
        "dreizehn",
        "vierzehn",
        "fünfzehn",
        "sechzehn",
        "siebzehn",
        "achtzehn",
        "neunzehn",
    ][n as usize]
}
fn cardinal(n: u64) -> String {
    if n < 20 {
        return small(n).into();
    }
    if n < 100 {
        let tens = [
            "", "", "zwanzig", "dreißig", "vierzig", "fünfzig", "sechzig", "siebzig", "achtzig",
            "neunzig",
        ][(n / 10) as usize];
        return if n % 10 == 0 {
            tens.into()
        } else {
            format!(
                "{}und{tens}",
                if n % 10 == 1 { "ein" } else { small(n % 10) }
            )
        };
    }
    if n < 1_000_000 {
        let (scale, word) = if n < 1000 {
            (100, "hundert")
        } else {
            (1000, "tausend")
        };
        let head = if n / scale == 1 {
            "ein".into()
        } else {
            cardinal(n / scale)
        };
        return format!(
            "{head}{word}{}",
            if n % scale == 0 {
                String::new()
            } else {
                cardinal(n % scale)
            }
        );
    }
    let (scale, singular, plural) = if n < 1_000_000_000 {
        (1_000_000, "Million", "Millionen")
    } else {
        (1_000_000_000, "Milliarde", "Milliarden")
    };
    let head = if n / scale == 1 {
        format!("eine {singular}")
    } else {
        format!("{} {plural}", cardinal(n / scale))
    };
    if n % scale == 0 {
        head
    } else {
        format!("{head} {}", cardinal(n % scale))
    }
}
fn ordinal(n: u64, ending: &str) -> String {
    let stem = match n {
        1 => "erst".into(),
        3 => "dritt".into(),
        7 => "siebt".into(),
        8 => "acht".into(),
        _ => format!("{}{}", cardinal(n), if n < 20 { "t" } else { "st" }),
    };
    format!("{stem}{ending}")
}
fn unit(s: &str) -> Option<&'static str> {
    Some(match s {
        "%" => "Prozent",
        "°C" => "Grad Celsius",
        "°F" => "Grad Fahrenheit",
        "€" | "EUR" => "Euro",
        "W" => "Watt",
        "kW" => "Kilowatt",
        "Wh" => "Wattstunden",
        "kWh" => "Kilowattstunden",
        "V" => "Volt",
        "A" => "Ampere",
        "km" => "Kilometer",
        "m" => "Meter",
        "cm" => "Zentimeter",
        "mm" => "Millimeter",
        "Prozent" => "Prozent",
        "Grad" => "Grad",
        "Celsius" => "Celsius",
        "Fahrenheit" => "Fahrenheit",
        "Euro" => "Euro",
        "Watt" => "Watt",
        "Kilowatt" => "Kilowatt",
        "Wattstunden" => "Wattstunden",
        "Kilowattstunden" => "Kilowattstunden",
        "Volt" => "Volt",
        "Ampere" => "Ampere",
        "Kilometer" => "Kilometer",
        "Meter" => "Meter",
        "Zentimeter" => "Zentimeter",
        "Millimeter" => "Millimeter",
        _ => return None,
    })
}
fn singular(s: &str) -> &str {
    match s {
        "Wattstunden" => "Wattstunde",
        "Kilowattstunden" => "Kilowattstunde",
        _ => s,
    }
}
fn number(s: &str) -> Option<(bool, u64, Option<&str>)> {
    let (negative, s) = if let Some(s) = s.strip_prefix('-').or_else(|| s.strip_prefix('−')) {
        (true, s)
    } else {
        (false, s.strip_prefix('+').unwrap_or(s))
    };
    let mut pieces = s.split(',');
    let integer = pieces.next()?;
    let fraction = pieces.next();
    if pieces.next().is_some()
        || integer.is_empty()
        || integer.len() > 12
        || !integer.bytes().all(|c| c.is_ascii_digit())
        || (integer.len() > 1 && integer.starts_with('0'))
        || fraction
            .is_some_and(|s| s.is_empty() || s.len() > 12 || !s.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some((negative, integer.parse().ok()?, fraction))
}
fn say_number(negative: bool, n: u64, fraction: Option<&str>, units: Option<&str>) -> String {
    let sign = if negative { "minus " } else { "" };
    if units == Some("Euro") && fraction.is_some_and(|s| s.len() <= 2) {
        let f = fraction.unwrap();
        let cents = f.parse::<u64>().unwrap() * if f.len() == 1 { 10 } else { 1 };
        let euros = if n == 1 { "ein".into() } else { cardinal(n) };
        if cents == 0 {
            return format!("{sign}{euros} Euro");
        }
        let cent = if cents == 1 {
            "ein".into()
        } else {
            cardinal(cents)
        };
        return if n == 0 {
            format!("{sign}{cent} Cent")
        } else {
            format!("{sign}{euros} Euro und {cent} Cent")
        };
    }
    let mut output = format!(
        "{sign}{}",
        if n == 1 && fraction.is_none() && units.is_some() {
            if matches!(units, Some("Wattstunden" | "Kilowattstunden")) {
                "eine".into()
            } else {
                "ein".into()
            }
        } else {
            cardinal(n)
        }
    );
    if let Some(f) = fraction {
        output.push_str(" Komma");
        for c in f.bytes() {
            output.push(' ');
            output.push_str(small((c - b'0') as u64));
        }
    }
    if let Some(u) = units {
        output.push(' ');
        output.push_str(if n == 1 && fraction.is_none() {
            singular(u)
        } else {
            u
        });
    }
    output
}
// Only trim enclosing/sentence punctuation; digits remain an indivisible token.
fn core(token: &str) -> (&str, &str, &str) {
    let start = token
        .find(|c: char| !matches!(c, '(' | '[' | '{' | '"' | '\'' | '„' | '“' | '«'))
        .unwrap_or(token.len());
    let end = token
        .trim_end_matches([
            '.', ',', '!', '?', ';', ')', ']', '}', '"', '\'', '”', '“', '»',
        ])
        .len()
        .max(start);
    (&token[..start], &token[start..end], &token[end..])
}
fn protected_context(recent: &VecDeque<String>) -> bool {
    recent
        .iter()
        .rev()
        .find(|w| {
            !["ist", "beträgt", "lautet", "der", "die", "das"].contains(&w.to_lowercase().as_str())
        })
        .is_some_and(|w| {
            [
                "port",
                "portnummer",
                "tcp-port",
                "udp-port",
                "id",
                "ids",
                "kennung",
                "pin",
                "code",
                "version",
                "modell",
                "datei",
                "seriennummer",
                "telefonnummer",
            ]
            .contains(&w.trim_end_matches(':').to_lowercase().as_str())
        })
}
fn date_ending(recent: &VecDeque<String>) -> &'static str {
    let previous = recent.back().map(|s| s.to_lowercase()).unwrap_or_default();
    if ["am", "vom", "zum", "den"].contains(&previous.as_str()) {
        "en"
    } else if ["der", "die"].contains(&previous.as_str()) {
        "e"
    } else {
        "er"
    }
}

fn named_date(s: &str, suffix: &str, month: &str, recent: &VecDeque<String>) -> Option<String> {
    if suffix != "." || s.is_empty() || s.len() > 2 || !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let months = [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ];
    let index = months.iter().position(|m| *m == month)?;
    let day = s.parse::<u64>().ok()?;
    if day == 0 || day > [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][index] {
        return None;
    }
    Some(format!("{} {month}", ordinal(day, date_ending(recent))))
}
fn date(s: &str, recent: &VecDeque<String>) -> Option<String> {
    let v = s.split('.').collect::<Vec<_>>();
    if v.len() != 3
        || v[0].len() > 2
        || v[1].len() > 2
        || v[2].len() != 4
        || v.iter()
            .any(|x| x.is_empty() || !x.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let d = v[0].parse::<u64>().ok()?;
    let m = v[1].parse::<usize>().ok()?;
    let y = v[2].parse::<u64>().ok()?;
    if !(1..=12).contains(&m) || !(1000..=9999).contains(&y) {
        return None;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if d == 0 || d > days[m - 1] {
        return None;
    }
    let ending = date_ending(recent);
    let months = [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ];
    Some(format!(
        "{} {} {}",
        ordinal(d, ending),
        months[m - 1],
        if (1100..2000).contains(&y) {
            format!(
                "{}hundert{}",
                cardinal(y / 100),
                if y % 100 == 0 {
                    String::new()
                } else {
                    cardinal(y % 100)
                }
            )
        } else {
            cardinal(y)
        }
    ))
}
fn time(s: &str) -> Option<String> {
    let v = s.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&v.len())
        || v[0].len() > 2
        || v[1].len() != 2
        || v.iter().any(|s| !s.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let h = v[0].parse::<u64>().ok()?;
    let m = v[1].parse::<u64>().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    let mut output = format!("{} Uhr", if h == 1 { "ein".into() } else { cardinal(h) });
    if m != 0 {
        output.push(' ');
        output.push_str(&cardinal(m));
    }
    if v.len() == 3 {
        let seconds = v[2].parse::<u64>().ok()?;
        if v[2].len() != 2 || seconds > 59 {
            return None;
        }
        output.push_str(&format!(
            " und {} {}",
            if seconds == 1 {
                "eine".into()
            } else {
                cardinal(seconds)
            },
            if seconds == 1 { "Sekunde" } else { "Sekunden" }
        ));
    }
    Some(output)
}

/// Carry incomplete tokens across streamed text events BEFORE sentence segmentation.
pub struct Normalizer {
    active: bool,
    pending: String,
    recent: VecDeque<String>,
}
impl Normalizer {
    pub fn new(flag: bool, code: &str) -> Self {
        Self {
            active: enabled(flag, code),
            pending: String::new(),
            recent: VecDeque::new(),
        }
    }
    pub fn push(&mut self, text: &str, finalize: bool) -> String {
        if !self.active {
            return text.into();
        }
        self.pending.push_str(text);
        let mut out = String::new();
        let mut consumed = 0;
        loop {
            let tail = &self.pending[consumed..];
            let spaces = tail.len() - tail.trim_start_matches(char::is_whitespace).len();
            let start = consumed + spaces;
            let tail = &self.pending[start..];
            if tail.is_empty() {
                out.push_str(&self.pending[consumed..]);
                consumed = self.pending.len();
                break;
            }
            let end = start + tail.find(char::is_whitespace).unwrap_or(tail.len());
            if end == self.pending.len() && !finalize {
                break;
            }
            let token = &self.pending[start..end];
            let (prefix, c, suffix) = core(token);
            let next_start = end + self.pending[end..].len()
                - self.pending[end..]
                    .trim_start_matches(char::is_whitespace)
                    .len();
            let next_end = next_start
                + self.pending[next_start..]
                    .find(char::is_whitespace)
                    .unwrap_or(self.pending.len() - next_start);
            let next = &self.pending[next_start..next_end];
            let (np, nc, ns) = core(next);
            // Numbers need one complete lookahead token to join units/Uhr safely.
            let numeric = c
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '−' | '+'));
            if numeric && !finalize && (next.is_empty() || next_end == self.pending.len()) {
                break;
            }
            let mut finish = end;
            let mut replacement = None;
            let mut final_suffix = suffix;
            if numeric && !protected_context(&self.recent) {
                if np.is_empty() && named_date(c, suffix, nc, &self.recent).is_some() {
                    replacement = named_date(c, suffix, nc, &self.recent);
                    finish = next_end;
                    final_suffix = ns;
                } else if let Some(value) = date(c, &self.recent) {
                    replacement = Some(value);
                } else if let Some(value) = time(c) {
                    replacement = Some(value);
                    if nc == "Uhr" && np.is_empty() && suffix.is_empty() {
                        finish = next_end;
                        final_suffix = ns;
                    }
                } else {
                    let mut numeric_part = c;
                    let mut units = None;
                    // Attached symbols must match in full: F1/N100/IDs/URLs/files stay opaque.
                    for u in [
                        "kWh", "Wh", "kW", "°C", "°F", "km", "cm", "mm", "EUR", "W", "V", "A", "m",
                        "%", "€",
                    ] {
                        if let Some(n) = c.strip_suffix(u) {
                            if number(n).is_some() {
                                numeric_part = n;
                                units = unit(u);
                                break;
                            }
                        }
                    }
                    if let Some((negative, n, f)) = number(numeric_part) {
                        if units.is_none() && np.is_empty() && suffix.is_empty() {
                            if let Some(u) = unit(nc) {
                                units = Some(u);
                                finish = next_end;
                                final_suffix = ns;
                            }
                        }
                        replacement = Some(say_number(negative, n, f, units));
                    }
                }
            }
            out.push_str(&self.pending[consumed..start]);
            if let Some(value) = replacement {
                out.push_str(prefix);
                out.push_str(&value);
                out.push_str(final_suffix);
            } else {
                out.push_str(token);
            }
            // Original lexical context, never normalized digits, protects ports and IDs.
            if final_suffix.contains(['.', '!', '?', ';']) {
                self.recent.clear();
            } else {
                self.recent.push_back(c.to_owned());
                while self.recent.len() > 3 {
                    self.recent.pop_front();
                }
            }
            consumed = finish;
        }
        self.pending.drain(..consumed);
        out
    }
}
pub fn apply<'a>(text: &'a str, flag: bool, code: &str) -> Cow<'a, str> {
    if !enabled(flag, code) || !text.bytes().any(|c| c.is_ascii_digit()) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(Normalizer::new(true, code).push(text, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sensor_rules() {
        for (input, want) in [
            (
                "0 1 12 16 17 21 100 850 1000 2026",
                "null eins zwölf sechzehn siebzehn einundzwanzig einhundert achthundertfünfzig eintausend zweitausendsechsundzwanzig",
            ),
            ("21,5 °C", "einundzwanzig Komma fünf Grad Celsius"),
            ("-3,2 °C", "minus drei Komma zwei Grad Celsius"),
            ("−3,2°C", "minus drei Komma zwei Grad Celsius"),
            ("65 %", "fünfundsechzig Prozent"),
            ("32 °F", "zweiunddreißig Grad Fahrenheit"),
            ("18:30 Uhr", "achtzehn Uhr dreißig"),
            ("01:00 Uhr", "ein Uhr"),
            ("00:05:01", "null Uhr fünf und eine Sekunde"),
            ("04.10.2026", "vierter Oktober zweitausendsechsundzwanzig"),
            (
                "am 04.10.2026",
                "am vierten Oktober zweitausendsechsundzwanzig",
            ),
            (
                "der 04.10.2026",
                "der vierte Oktober zweitausendsechsundzwanzig",
            ),
            ("12,4 kWh", "zwölf Komma vier Kilowattstunden"),
            ("850 W", "achthundertfünfzig Watt"),
            ("2,3 kW", "zwei Komma drei Kilowatt"),
            ("2 Wh", "zwei Wattstunden"),
            ("1 kWh", "eine Kilowattstunde"),
            ("1 Wh", "eine Wattstunde"),
            ("21,5, Grad", "einundzwanzig Komma fünf, Grad"),
            ("21,5 (°C)", "einundzwanzig Komma fünf (°C)"),
            (
                "Port 8881; Temperatur 21,5 °C",
                "Port 8881; Temperatur einundzwanzig Komma fünf Grad Celsius",
            ),
            ("230 V", "zweihundertdreißig Volt"),
            ("4,5 A", "vier Komma fünf Ampere"),
            ("12,7 km", "zwölf Komma sieben Kilometer"),
            ("1 m 2 cm 1 mm", "ein Meter zwei Zentimeter ein Millimeter"),
            ("49,99 €", "neunundvierzig Euro und neunundneunzig Cent"),
            ("1,01 Euro", "ein Euro und ein Cent"),
            ("0,99 €", "neunundneunzig Cent"),
            ("0,05 V", "null Komma null fünf Volt"),
            ("-12", "minus zwölf"),
            ("1000000", "eine Million"),
            ("1000000000", "eine Milliarde"),
        ] {
            assert_eq!(apply(input, true, "de"), want, "{input}");
        }
    }
    #[test]
    fn named_dates_and_years() {
        for (a, b) in [
            ("am 1. Oktober", "am ersten Oktober"),
            ("der 7. Mai", "der siebte Mai"),
            ("03.10.1990", "dritter Oktober neunzehnhundertneunzig"),
        ] {
            assert_eq!(apply(a, true, "de"), b);
        }
    }
    #[test]
    fn ha_answers() {
        for (input, want) in [
            (
                "Im Wohnzimmer sind es 21,5 Grad.",
                "Im Wohnzimmer sind es einundzwanzig Komma fünf Grad.",
            ),
            (
                "Draußen sind es -3,2 Grad Celsius.",
                "Draußen sind es minus drei Komma zwei Grad Celsius.",
            ),
            (
                "Die Luftfeuchtigkeit beträgt 65 Prozent.",
                "Die Luftfeuchtigkeit beträgt fünfundsechzig Prozent.",
            ),
            ("Es ist 18:30 Uhr.", "Es ist achtzehn Uhr dreißig."),
            (
                "Der heutige Energieverbrauch beträgt 12,4 Kilowattstunden.",
                "Der heutige Energieverbrauch beträgt zwölf Komma vier Kilowattstunden.",
            ),
            (
                "Die aktuelle Leistung beträgt 850 Watt.",
                "Die aktuelle Leistung beträgt achthundertfünfzig Watt.",
            ),
            (
                "Die Batteriespannung beträgt 12,7 Volt.",
                "Die Batteriespannung beträgt zwölf Komma sieben Volt.",
            ),
            (
                "Der Preis beträgt 49,99 Euro.",
                "Der Preis beträgt neunundvierzig Euro und neunundneunzig Cent.",
            ),
        ] {
            assert_eq!(apply(input, true, "de"), want);
        }
    }
    #[test]
    fn technical_tokens_unchanged() {
        for text in [
            "203.0.113.42",
            "1.24.2",
            "http://203.0.113.42:8881",
            "F1 M5 N100",
            "sensor.temp_21",
            "abc123 123abc",
            "12.json",
            "2026-10-04.log",
            "ID: 12345",
            "Port 8881",
            "Der Port ist 10200.",
            "Version 123",
            "PIN 0123",
            "00042",
            "1234567890123456",
            "https://example.de/21,5",
            "a1b2-c3",
            "31.02.2026",
            "25:99 Uhr",
        ] {
            assert_eq!(apply(text, true, "de"), text, "{text}");
        }
    }
    #[test]
    fn bypass_and_no_numbers() {
        let text = "Guten Morgen. Das Licht ist eingeschaltet!";
        assert!(matches!(apply(text, true, "de"), Cow::Borrowed(_)));
        assert_eq!(apply("21,5 °C", false, "de"), "21,5 °C");
        assert_eq!(apply("21,5 °C", true, "en"), "21,5 °C");
        assert_eq!(
            apply("21,5 °C", true, "de-DE"),
            "einundzwanzig Komma fünf Grad Celsius"
        );
    }
    #[test]
    fn every_utf8_character_and_every_split_matches_full() {
        for text in [
            "Guten Morgen. Es sind 21,5 °C. Um 18:30 Uhr kosten 12,4 kWh 49,99 €. Dr. Müller kommt am 04.10.2026 und am 1. Oktober.",
            "Port 8881. URL http://203.0.113.42:8881 und N100 bleiben erhalten.",
        ] {
            let expected = apply(text, true, "de");
            let mut n = Normalizer::new(true, "de");
            let mut out = String::new();
            for c in text.chars() {
                out.push_str(&n.push(&c.to_string(), false));
            }
            out.push_str(&n.push("", true));
            assert_eq!(out, expected);
            for (i, _) in text.char_indices() {
                let mut n = Normalizer::new(true, "de");
                let mut out = n.push(&text[..i], false);
                out.push_str(&n.push(&text[i..], false));
                out.push_str(&n.push("", true));
                assert_eq!(out, expected, "split {i}");
            }
        }
    }
    #[test]
    fn punctuation_and_spacing() {
        assert_eq!(
            apply("  (21,5°C),  65%!?\n1mm.", true, "de"),
            "  (einundzwanzig Komma fünf Grad Celsius),  fünfundsechzig Prozent!?\nein Millimeter."
        );
    }
}
