use crate::metadata::PhotoMetadata;
use chrono::Datelike;
use chrono::Timelike;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplatePart {
    Literal(String),
    Token(Token),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Date,
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    CameraMake,
    CameraModel,
    LensMake,
    LensModel,
    FilmSim,
    OrigName,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TemplateError {
    #[error("テンプレートが空です")]
    Empty,
    #[error("中括弧の対応が不正です")]
    UnbalancedBraces,
    #[error("未対応トークンです: {0}")]
    UnknownToken(String),
}

pub fn validate_template(input: &str) -> Result<(), TemplateError> {
    parse_template(input).map(|_| ())
}

pub fn parse_template(input: &str) -> Result<Vec<TemplatePart>, TemplateError> {
    if input.is_empty() {
        return Err(TemplateError::Empty);
    }

    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '{' => {
                if !literal.is_empty() {
                    parts.push(TemplatePart::Literal(std::mem::take(&mut literal)));
                }
                let mut token = String::new();
                let mut found_close = false;
                for next in chars.by_ref() {
                    if next == '}' {
                        found_close = true;
                        break;
                    }
                    if next == '{' {
                        return Err(TemplateError::UnbalancedBraces);
                    }
                    token.push(next);
                }
                if !found_close || token.is_empty() {
                    return Err(TemplateError::UnbalancedBraces);
                }
                parts.push(TemplatePart::Token(parse_token(&token)?));
            }
            '}' => return Err(TemplateError::UnbalancedBraces),
            _ => literal.push(ch),
        }
    }

    if !literal.is_empty() {
        parts.push(TemplatePart::Literal(literal));
    }

    if parts.is_empty() {
        return Err(TemplateError::Empty);
    }

    Ok(parts)
}

pub fn render_template(parts: &[TemplatePart], metadata: &PhotoMetadata) -> String {
    render_template_with_options(parts, metadata, true)
}

pub fn render_template_with_options(
    parts: &[TemplatePart],
    metadata: &PhotoMetadata,
    dedupe_same_maker: bool,
) -> String {
    let same_maker = same_maker(
        metadata.normalized_camera_make(),
        metadata.normalized_lens_make(),
    ) && dedupe_same_maker;

    let mut output = String::new();
    for part in parts {
        match part {
            TemplatePart::Literal(s) => output.push_str(&normalize_literal_connector(s)),
            TemplatePart::Token(token) => {
                let value = match token {
                    Token::Date => format_date(metadata),
                    Token::Year => format!("{:04}", metadata.date.year()),
                    Token::Month => format!("{:02}", metadata.date.month()),
                    Token::Day => format!("{:02}", metadata.date.day()),
                    Token::Hour => format!("{:02}", metadata.date.hour()),
                    Token::Minute => format!("{:02}", metadata.date.minute()),
                    Token::Second => format!("{:02}", metadata.date.second()),
                    Token::CameraMake => metadata
                        .normalized_camera_make()
                        .unwrap_or_default()
                        .to_string(),
                    Token::CameraModel => metadata
                        .camera_model
                        .as_deref()
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    Token::LensMake => {
                        if same_maker {
                            String::new()
                        } else {
                            metadata
                                .normalized_lens_make()
                                .unwrap_or_default()
                                .to_string()
                        }
                    }
                    Token::LensModel => metadata
                        .lens_model
                        .as_deref()
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    Token::FilmSim => metadata
                        .film_sim
                        .as_deref()
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    Token::OrigName => metadata.original_name.clone(),
                };
                output.push_str(&normalize_token_value(&value));
            }
        }
    }

    output
}

fn parse_token(token: &str) -> Result<Token, TemplateError> {
    match token {
        "date" => Ok(Token::Date),
        "year" => Ok(Token::Year),
        "month" => Ok(Token::Month),
        "day" => Ok(Token::Day),
        "hour" => Ok(Token::Hour),
        "minute" => Ok(Token::Minute),
        "second" => Ok(Token::Second),
        "camera_maker" => Ok(Token::CameraMake),
        "camera_model" => Ok(Token::CameraModel),
        "lens_maker" => Ok(Token::LensMake),
        "lens_model" => Ok(Token::LensModel),
        "film_sim" => Ok(Token::FilmSim),
        "orig_name" => Ok(Token::OrigName),
        other => Err(TemplateError::UnknownToken(other.to_string())),
    }
}

fn same_maker(camera_make: Option<&str>, lens_make: Option<&str>) -> bool {
    match (camera_make, lens_make) {
        (Some(camera), Some(lens)) => camera.eq_ignore_ascii_case(lens),
        _ => false,
    }
}

fn format_date(metadata: &PhotoMetadata) -> String {
    let d = metadata.date;
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        d.year(),
        d.month(),
        d.day(),
        d.hour(),
        d.minute(),
        d.second()
    )
}

fn normalize_literal_connector(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch == '-' { '_' } else { ch })
        .collect()
}

fn normalize_token_value(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{MetadataSource, PhotoMetadata};
    use chrono::Local;
    use std::path::PathBuf;

    fn metadata() -> PhotoMetadata {
        PhotoMetadata {
            source: MetadataSource::JpgExif,
            date: Local::now(),
            camera_make: Some("FUJIFILM".to_string()),
            camera_model: Some("X-T5".to_string()),
            lens_make: Some("fujifilm".to_string()),
            lens_model: Some("XF33mmF1.4".to_string()),
            film_sim: Some("Classic Chrome".to_string()),
            original_name: "IMG_0001".to_string(),
            jpg_path: PathBuf::from("IMG_0001.JPG"),
        }
    }

    #[test]
    fn parse_template_ok() {
        let parsed = parse_template("{date}_{orig_name}").expect("must parse");
        assert!(!parsed.is_empty());
    }

    #[test]
    fn parse_template_invalid_unknown() {
        let err = parse_template("{foo}").expect_err("must fail");
        assert!(matches!(err, TemplateError::UnknownToken(_)));
    }

    #[test]
    fn parse_template_invalid_brace() {
        let err = parse_template("{date").expect_err("must fail");
        assert_eq!(err, TemplateError::UnbalancedBraces);
    }

    #[test]
    fn render_dedupes_lens_maker() {
        let parsed =
            parse_template("{camera_maker}_{lens_maker}_{lens_model}").expect("must parse");
        let rendered = render_template_with_options(&parsed, &metadata(), true);
        assert_eq!(rendered, "FUJIFILM__XF33mmF1.4");
    }

    #[test]
    fn render_keeps_lens_maker_when_dedupe_off() {
        let parsed =
            parse_template("{camera_maker}_{lens_maker}_{lens_model}").expect("must parse");
        let rendered = render_template_with_options(&parsed, &metadata(), false);
        assert_eq!(rendered, "FUJIFILM_fujifilm_XF33mmF1.4");
    }

    #[test]
    fn parse_template_rejects_legacy_make_tokens() {
        let err = parse_template("{camera_make}_{lens_make}")
            .expect_err("legacy token names must be rejected");
        assert!(matches!(err, TemplateError::UnknownToken(_)));
    }

    #[test]
    fn render_replaces_spaces_inside_tokens_with_hyphen() {
        let mut m = metadata();
        m.lens_model = Some("XF35mm F1.4 R".to_string());
        m.film_sim = Some("Classic Chrome".to_string());
        let parsed = parse_template("{lens_model}_{film_sim}").expect("must parse");
        let rendered = render_template_with_options(&parsed, &m, true);
        assert_eq!(rendered, "XF35mm-F1.4-R_Classic-Chrome");
    }

    #[test]
    fn render_normalizes_literal_separator_to_underscore() {
        let parsed = parse_template("{date} - {orig_name}").expect("must parse");
        let rendered = render_template_with_options(&parsed, &metadata(), true);
        assert!(rendered.contains("_"));
        assert!(!rendered.contains(" - "));
    }

    #[test]
    fn render_supports_split_date_tokens() {
        let parsed = parse_template("{year}{month}{day}{hour}{minute}{second}_{orig_name}")
            .expect("must parse");
        let rendered = render_template_with_options(&parsed, &metadata(), true);
        assert!(rendered.ends_with("_IMG_0001"));
        assert_eq!(rendered.len(), 14 + "_IMG_0001".len());
    }
}
