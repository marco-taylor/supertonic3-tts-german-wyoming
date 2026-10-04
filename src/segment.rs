// Conservative sentence segmentation for German. Never split on commas or
// within abbreviations, decimals, times, dates, ordinals or unit expressions.
const ABBREVIATIONS: &[&str] = &[
    "dr", "prof", "dipl", "ing", "bzw", "ca", "usw", "etc", "ggf", "inkl", "zzgl", "nr", "str",
    "tel", "min", "std", "sek", "abs", "art", "bd", "vgl", "geb", "jan", "feb", "mär", "mrz",
    "apr", "jun", "jul", "aug", "sep", "sept", "okt", "nov", "dez", "kg", "km", "cm", "mm", "kwh",
    "kw", "eur",
];
pub struct Segmenter {
    pending: String,
    first: bool,
}
impl Segmenter {
    pub fn new() -> Self {
        Self {
            pending: String::new(),
            first: true,
        }
    }
    pub fn push(&mut self, text: &str) {
        self.pending.push_str(text);
    }
    pub fn drain(&mut self, finalize: bool) -> Vec<String> {
        let mut output = Vec::new();
        loop {
            let mut boundary = None;
            let chars: Vec<(usize, char)> = self.pending.char_indices().collect();
            for (i, &(offset, c)) in chars.iter().enumerate() {
                if !matches!(c, '.' | '?' | '!') {
                    continue;
                }
                if c == '.' {
                    let before = &self.pending[..offset];
                    let token = before
                        .rsplit(|ch: char| !ch.is_alphanumeric())
                        .next()
                        .unwrap_or("");
                    if token.chars().count() <= 1
                        || token.chars().last().is_some_and(|c| c.is_ascii_digit())
                        || ABBREVIATIONS.contains(&token.to_lowercase().as_str())
                    {
                        continue;
                    }
                }
                let mut j = i + 1;
                while j < chars.len()
                    && matches!(
                        chars[j].1,
                        '.' | '?' | '!' | '"' | '\'' | '”' | '“' | '»' | '«' | ')' | ']'
                    )
                {
                    j += 1;
                }
                let end = chars.get(j).map(|x| x.0).unwrap_or(self.pending.len());
                if j < chars.len() && !chars[j].1.is_whitespace() {
                    continue;
                }
                let next = self.pending[end..].trim_start().chars().next();
                if next.is_none() && !finalize {
                    continue;
                }
                // Lowercase continuation after a period is not a clear sentence boundary.
                if c == '.' && next.is_some_and(|c| c.is_lowercase()) {
                    continue;
                }
                let candidate = self.pending[..end].trim();
                let minimum = if self.first { 10 } else { 24 };
                if candidate.chars().count() >= minimum && candidate.split_whitespace().count() >= 2
                {
                    boundary = Some(end);
                    break;
                }
            }
            let Some(end) = boundary else { break };
            output.push(self.pending[..end].trim().to_string());
            self.pending = self.pending[end..].trim_start().to_string();
            self.first = false;
        }
        if finalize && !self.pending.trim().is_empty() {
            let tail = self.pending.trim().to_string();
            if tail.chars().count() < 10 && !output.is_empty() {
                let last = output.last_mut().unwrap();
                last.push(' ');
                last.push_str(&tail);
            } else {
                output.push(tail);
            }
            self.pending.clear();
            self.first = false;
        }
        output
    }
}
pub fn sentences(text: &str) -> Vec<String> {
    let mut s = Segmenter::new();
    s.push(text);
    s.drain(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn german_protected_expressions() {
        let text = "Dr. Müller zeigt z. B. am 1. Oktober um 18:30 Uhr 3,5 Grad und 2,5 kWh an. Alles ist bereit!";
        assert_eq!(
            sentences(text),
            vec![
                "Dr. Müller zeigt z. B. am 1. Oktober um 18:30 Uhr 3,5 Grad und 2,5 kWh an.",
                "Alles ist bereit!"
            ]
        );
        assert_eq!(
            sentences("Heute ist der 04.10.2026. Die Werte sind 1, 2, 3 und 4."),
            vec!["Heute ist der 04.10.2026. Die Werte sind 1, 2, 3 und 4."]
        );
    }
    #[test]
    fn greeting_first_and_short_fragments_grouped() {
        assert_eq!(
            sentences("Guten Morgen. Im Wohnzimmer sind es 21 Grad. Heute wird es sonnig."),
            vec![
                "Guten Morgen.",
                "Im Wohnzimmer sind es 21 Grad.",
                "Heute wird es sonnig."
            ]
        );
        assert_eq!(
            sentences("Ja. Gut. Das Licht ist eingeschaltet!"),
            vec!["Ja. Gut. Das Licht ist eingeschaltet!"]
        );
        assert_eq!(
            sentences("Ist das Licht eingeschaltet? Ja, das Licht ist eingeschaltet!"),
            vec![
                "Ist das Licht eingeschaltet?",
                "Ja, das Licht ist eingeschaltet!"
            ]
        );
    }
    #[test]
    fn fragmented_input_does_not_split_abbreviations() {
        let text = "Guten Morgen. Dr. Müller zeigt z. B. 2,5 kWh an. Alles ist bereit!";
        let mut s = Segmenter::new();
        let mut out = Vec::new();
        for c in text.chars() {
            s.push(&c.to_string());
            out.extend(s.drain(false));
        }
        out.extend(s.drain(true));
        assert_eq!(out, sentences(text));
        assert_eq!(out.join(" "), text);
    }
}

// Keep optional long phrases below the upstream byte limit without using its
// comma fallback. Numbers stay with their following unit/date/time word and
// abbreviation prefixes stay with their following word.
pub fn bounded(text: &str, limit: usize) -> Vec<String> {
    fn protected(word: &str) -> bool {
        let number = word.chars().any(|c| c.is_ascii_digit())
            && word
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | ':' | '+' | '-' | '%'));
        let stem = word.trim_end_matches('.');
        number
            || (word.ends_with('.')
                && (stem.chars().count() <= 1
                    || ABBREVIATIONS.contains(&stem.to_lowercase().as_str())))
    }
    let mut groups: Vec<String> = Vec::new();
    let mut attach = false;
    for word in text.split_whitespace() {
        if attach && !groups.is_empty() {
            let group = groups.last_mut().unwrap();
            group.push(' ');
            group.push_str(word);
        } else {
            groups.push(word.to_owned());
        }
        attach = protected(word);
    }
    let mut output = Vec::new();
    let mut current = String::new();
    for group in groups {
        if !current.is_empty() && current.len() + 1 + group.len() > limit {
            output.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&group);
    }
    if !current.is_empty() {
        output.push(current);
    }
    output
}

#[cfg(test)]
mod bounded_tests {
    use super::*;
    #[test]
    fn long_phrases_preserve_german_tokens_at_boundaries() {
        let tokens = "Dr. Müller zeigt z. B. am 1. Oktober um 18:30 Uhr 3,5 Grad und 2,5 kWh an.";
        let text = (0..12).map(|_| tokens).collect::<Vec<_>>().join(" ");
        let chunks = bounded(&text, 300);
        assert_eq!(chunks.join(" "), text);
        assert!(chunks.iter().all(|c| c.len() <= 300));
        for expression in [
            "Dr. Müller",
            "z. B.",
            "1. Oktober",
            "18:30 Uhr",
            "3,5 Grad",
            "2,5 kWh",
        ] {
            assert_eq!(
                chunks
                    .iter()
                    .map(|c| c.matches(expression).count())
                    .sum::<usize>(),
                12
            );
        }
    }
    #[test]
    fn short_number_sequences_are_kept_together() {
        assert_eq!(
            bounded("Alles ist bereit. Die Werte sind 1, 2, 3 und 4,5 kWh.", 25),
            vec![
                "Alles ist bereit. Die",
                "Werte sind 1, 2, 3 und",
                "4,5 kWh."
            ]
        );
    }
}

#[cfg(test)]
mod normalization_tests {
    #[test]
    fn normalize_before_sentence_segmentation() {
        let text = "Guten Morgen. Es sind 21,5 °C. Um 18:30 Uhr kommt Dr. Müller am 04.10.2026. Alles ist bereit!";
        let normalized = crate::normalize::apply(text, true, "de");
        let expected = super::sentences(&normalized);
        let mut normalizer = crate::normalize::Normalizer::new(true, "de");
        let mut segmenter = super::Segmenter::new();
        let mut actual = Vec::new();
        for c in text.chars() {
            segmenter.push(&normalizer.push(&c.to_string(), false));
            actual.extend(segmenter.drain(false));
        }
        segmenter.push(&normalizer.push("", true));
        actual.extend(segmenter.drain(true));
        assert_eq!(actual, expected);
        assert!(
            actual
                .iter()
                .any(|s| s.contains("einundzwanzig Komma fünf Grad Celsius"))
        );
        assert!(actual.iter().any(|s| s.contains("achtzehn Uhr dreißig")));
        assert!(
            actual
                .iter()
                .any(|s| s.contains("vierten Oktober zweitausendsechsundzwanzig"))
        );
    }
}
