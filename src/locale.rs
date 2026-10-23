//! Locale-sensitive computation using embedded ICU4X data, with no system ICU.
use crate::{
    epoch_ms, math,
    models::{has, Context, Results},
    re, require, Error, Result,
};
use icu_calendar::Gregorian;
use icu_datetime::{
    fieldsets::{zone, M},
    DateTimeFormatter, NoCalendarFormatter,
};
use icu_decimal::{
    input::{Decimal, FloatPrecision},
    CompactDecimalFormatter,
};
use icu_list::{
    options::{ListFormatterOptions, ListLength},
    ListFormatter,
};
use icu_locale::{
    names::{LanguageDisplay, LanguageIdentifierDisplayName, LanguageIdentifierDisplayNameOptions},
    Locale,
};
use icu_time::{zone::models::AtTime, TimeZoneInfo, ZonedDateTime};
use serde_json::{json, Value};
use std::collections::HashSet;

fn locale_error(error: impl std::fmt::Display) -> Error {
    Error::new("LOCALE_UNAVAILABLE", format!("ICU4X: {error}"))
}
fn default_locale() -> String {
    std::env::var("CF_GATEWAY_LOCALE").unwrap_or_else(|_| {
        sys_locale::get_locale()
            .filter(|value| !matches!(value.as_str(), "C" | "POSIX"))
            .unwrap_or_else(|| "en-US".into())
    })
}
/// The reference model's four locale operations, calculated from current operands.
pub(crate) fn format(
    locale: &str,
    zone_name: &str,
    timestamp: f64,
    language: &str,
    words: &[String],
    number: f64,
) -> Result<Value> {
    require(
        re(r"^[A-Za-z]{2,3}(?:[-_][A-Za-z0-9]{2,8})*$").is_match(locale)
            && re(r"^[A-Za-z_]+(?:/[A-Za-z_+-]+)*$").is_match(zone_name)
            && re(r"^[a-z]{2,3}-[A-Z]{2}$").is_match(language)
            && !words.is_empty()
            && words.len() <= 8
            && timestamp.is_finite()
            && number.is_finite(),
        "Locale operands",
    )?;
    let locale: Locale = locale.replace('_', "-").parse().map_err(locale_error)?;
    let language: Locale = language.parse().map_err(locale_error)?;
    let timezone: chrono_tz::Tz = zone_name.parse().map_err(locale_error)?;
    let utc = chrono::DateTime::from_timestamp_millis(timestamp as i64)
        .ok_or_else(|| locale_error("Timestamp out of range"))?;
    let local = utc.with_timezone(&timezone);
    let input: ZonedDateTime<Gregorian, TimeZoneInfo<AtTime>> = (&local).into();
    let month =
        DateTimeFormatter::try_new(locale.clone().into(), M::long()).map_err(locale_error)?;
    let zone = NoCalendarFormatter::try_new(locale.clone().into(), zone::SpecificLong)
        .map_err(locale_error)?;
    let month = month.format(&input).to_string();
    let zone = zone.format(&input.zone).to_string();
    let mut options = LanguageIdentifierDisplayNameOptions::default();
    options.language_display = Some(LanguageDisplay::Standard);
    let name =
        LanguageIdentifierDisplayName::try_new_heavy(locale.clone().into(), language.id, options)
            .map_err(locale_error)?;
    let list = ListFormatter::try_new_or(
        locale.clone().into(),
        ListFormatterOptions::default().with_length(ListLength::Wide),
    )
    .map_err(locale_error)?;
    let compact = CompactDecimalFormatter::try_new_long(locale.into(), Default::default())
        .map_err(locale_error)?;
    let number = Decimal::try_from_f64(number, FloatPrecision::RoundTrip).map_err(locale_error)?;
    Ok(
        json!({"mMAX8":format!("{month} {zone}"),"NzGy3":name.as_borrowed().to_string(),"FNvn2":list.format(words.iter()).to_string(),"HxIG7":compact.format(&number).to_string()}),
    )
}

pub fn calculate(c: &Context<'_>, results: &mut Results) -> Result<()> {
    let tag = c.catalog.modules.iter().find_map(|m| {
        if c.response
            .between(m.start, m.end)
            .iter()
            .any(|l| l.value == "zJGo6")
        {
            math::seeds(c.response, m, c.plan).ok().map(|x| x.2)
        } else {
            None
        }
    });
    for m in &c.catalog.modules {
        let entered = epoch_ms();
        let literals = c.response.between(m.start, m.end);
        let values: HashSet<_> = literals.iter().map(|l| l.value.as_str()).collect();
        if !has(&values, &["XwYHI5", "Ecdf9", "yGyw0", "qTcMk3"]) {
            continue;
        }
        let tag = tag.ok_or_else(|| Error::new("INVALID_DATA", "Locale numeric tag"))?;
        let f = &c.plan.format;
        let raw = &c.response.raw;
        let mut numbers = Vec::new();
        let keys = f.instruction_keys(f.state_step);
        for position in m.start..m.end.saturating_sub(11) {
            for &(_, key) in keys.at(f, raw[position], position) {
                if f.byte(raw[position + c.plan.kind_slot()?], key) != tag {
                    continue;
                }
                let number =
                    f64::from_be_bytes(std::array::from_fn(|i| f.byte(raw[position + 3 + i], key)));
                if (1e11..=1e13).contains(&number) || (1e15..=1e18).contains(&number) {
                    numbers.push((position, number));
                }
            }
        }
        let (timestamp, amount, language, words) = operands(&literals, &numbers)?;
        let locale = default_locale();
        let zone = std::env::var("CF_GATEWAY_TIMEZONE").unwrap_or_else(|_| "Asia/Shanghai".into());
        let mut fields = format(&locale, &zone, timestamp, &language, &words, amount)?;
        fields["XwYHI5"] = json!("zh-CN");
        fields["Ecdf9"] = json!(["zh-CN", "zh", "en"]);
        fields["yGyw0"] = json!([locale]);
        fields["qTcMk3"] = json!("en-us");
        results.insert(&m.id, "native-locale", fields, entered);
    }
    Ok(())
}

pub(crate) fn operands(
    input: &[&crate::data::Literal],
    numbers: &[(usize, f64)],
) -> Result<(f64, f64, String, Vec<String>)> {
    let mut literals = input.to_vec();
    literals.sort_by_key(|l| l.start);
    let values = literals
        .iter()
        .map(|l| l.value.as_str())
        .collect::<HashSet<_>>();
    let after = |option: &str| -> Result<f64> {
        let anchors: Vec<_> = (0..literals.len().saturating_sub(2))
            .filter(|i| literals[*i].value == option && literals[*i + 1].value == "long")
            .collect();
        require(anchors.len() == 1, "Locale option group")?;
        let i = anchors[0] + 1;
        let mut values: Vec<_> = numbers
            .iter()
            .filter(|(p, _)| literals[i].start < *p && *p < literals[i + 1].start)
            .map(|(_, n)| *n)
            .collect();
        values.sort_by(f64::total_cmp);
        values.dedup();
        require(values.len() == 1, "Locale numeric operand")?;
        Ok(values[0])
    };
    let timestamp = after("timeZoneName")?;
    let amount = after("compactDisplay")?;
    let languages: Vec<_> = values
        .iter()
        .filter(|s| re(r"^[a-z]{2,3}-[A-Z]{2}$").is_match(s))
        .collect();
    let lists: Vec<_> = (0..literals.len().saturating_sub(2))
        .filter(|i| literals[*i].value == "disjunction")
        .collect();
    require(
        languages.len() == 1 && lists.len() == 1,
        "Locale language/list operands",
    )?;
    let i = lists[0];
    let text = &literals[i + 1].value;
    let separator = &literals[i + 2].value;
    require(
        re(r"^[A-Za-z0-9]{3,512}$").is_match(text) && re(r"^[A-Za-z]$").is_match(separator),
        "Locale list format",
    )?;
    let words = text.split(separator).map(String::from).collect::<Vec<_>>();
    require(
        (1..=8).contains(&words.len())
            && words.iter().all(|s| !s.is_empty())
            && (1e11..=1e13).contains(&timestamp)
            && (1e15..=1e18).contains(&amount),
        "Locale task operand bounds",
    )?;
    Ok((timestamp, amount, languages[0].to_string(), words))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_locale_data_matches_independent_vectors() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/locale-format-vectors.json"))
                .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 109);
        for case in cases {
            let args = &case["args"];
            let actual = format(
                args[0].as_str().unwrap(),
                args[1].as_str().unwrap(),
                args[2].as_f64().unwrap(),
                args[3].as_str().unwrap(),
                &serde_json::from_value::<Vec<String>>(args[4].clone()).unwrap(),
                args[5].as_f64().unwrap(),
            )
            .unwrap();
            assert_eq!(actual, case["expected"], "{args}");
        }
    }
    #[test]
    fn invalid_operands_fail_without_fallback_or_system_libraries() {
        let words = vec!["one".to_owned()];
        for (locale, zone, time, language, list, number) in [
            ("../invalid", "UTC", 0., "eo-UA", words.clone(), 1000.),
            ("en-US", "UTC", f64::INFINITY, "eo-UA", words.clone(), 1000.),
            ("en-US", "UTC", 0., "eo-UA", words.clone(), f64::NAN),
            ("en-US", "Not/AZone", 0., "eo-UA", words.clone(), 1000.),
            ("en-US", "UTC", 0., "invalid", words.clone(), 1000.),
            ("en-US", "UTC", 0., "eo-UA", vec![], 1000.),
            ("en-US", "UTC", 0., "eo-UA", vec!["one".into(); 9], 1000.),
            ("en-US", "UTC", 1e20, "eo-UA", words.clone(), 1000.),
        ] {
            assert!(format(locale, zone, time, language, &list, number).is_err());
        }
    }
}
